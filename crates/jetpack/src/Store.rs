//! Jetpack state + store roots (D-JPK12; hangar per unified ecosystem U2).
//!
//! End-state roots are user-owned by default: Linux uses
//! `$XDG_DATA_HOME/jet` (or `~/.local/share/jet`), macOS uses
//! `~/Library/Application Support/Jet`, and Windows uses
//! `%LOCALAPPDATA%/Jet`; each holds the content-addressed **Hangar**.
//! Jetpack *owns* the lifecycle even when the Nix provider realizes bytes into
//! `/nix/store` — a Jetpack hangar entry is a small metadata record under our
//! root that points at the realized output.
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

use crate::SHA256;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(target_os = "linux")]
use std::process::{Child, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod Producer;
pub use Producer::*;
mod Cache;
pub use Cache::*;
mod Archive;
pub use Archive::*;
mod Nar;
pub use Nar::*;
mod Broker;
pub use Broker::*;

fn list_unlocked(roots: &Roots) -> Vec<StoreEntry> {
    jet_pkg_model::Store::list(roots)
}

/// Inspect package projections without taking a lock or replaying journals.
/// Health/reporting paths use this so observation cannot create or repair
/// store state.
pub(crate) fn list_read_only(roots: &Roots) -> Vec<StoreEntry> {
    list_unlocked(roots)
}

pub fn list_checked(roots: &Roots) -> std::io::Result<Vec<StoreEntry>> {
    super::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        Closure::recover_closure_journal_unlocked(roots)?;
        Ok(list_unlocked(roots))
    })
}

/// Read package records after replaying committed closure projections.
/// Corrupt WAL fails closed as an empty list for compatibility with the
/// historical infallible listing API; integrity-sensitive callers use
/// `list_checked`.
pub fn list(roots: &Roots) -> Vec<StoreEntry> {
    list_checked(roots).unwrap_or_default()
}

/// Move a pre-D-ECO-HANGARPATH1 user Hangar into the native per-user data
/// path. The old tree stays in place, so an operator can roll back by removing
/// the new tree and restoring the old resolver. A staging tree makes a crash
/// visible instead of presenting a partial migration as a live Hangar.
pub fn migrate_legacy_hangar(roots: &Roots) -> std::io::Result<bool> {
    if !roots.dev_mode {
        return Ok(false);
    }
    let destination = roots.hangar_dir();
    if fs::symlink_metadata(&destination).is_ok() {
        return Ok(false);
    }
    let legacy_source = legacy_user_hangar_dir();
    let mut sources = vec![legacy_source.clone(), PathBuf::from(crate::Syntax::HANGAR_DIR)];
    sources.dedup();
    let Some(source) = sources
        .into_iter()
        .find(|candidate| fs::symlink_metadata(candidate).is_ok())
    else {
        return Ok(false);
    };
    if source == destination {
        return Ok(false);
    }
    let parent = destination
        .parent()
        .ok_or_else(|| std::io::Error::other("native Hangar path has no parent"))?;
    let stage = parent.join(format!(
        ".{}-migration.partial",
        destination
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("hangar")
    ));
    let migrate_unlocked = || {
        if fs::symlink_metadata(&destination).is_ok() {
            return Ok(false);
        }
        if fs::symlink_metadata(&stage).is_ok() {
            return Err(std::io::Error::other(format!(
                "incomplete Hangar migration remains at `{}`; inspect or remove it before retrying",
                stage.display()
            )));
        }
        fs::create_dir_all(parent)?;
        copy_migration_tree(&source, &stage, &source)?;
        sync_store_directory(&stage)?;
        fs::rename(&stage, &destination)?;
        sync_store_directory(parent)?;
        Ok(true)
    };
    super::RuntimePolicy::with_lock(&roots.root, "hangar-migration", || {
        if source == legacy_source {
            super::RuntimePolicy::with_lock(
                &legacy_user_root(),
                "hangar-migration-source",
                migrate_unlocked,
            )
        } else {
            migrate_unlocked()
        }
    })
}

fn copy_migration_tree(source: &Path, destination: &Path, root: &Path) -> std::io::Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() {
        let target = fs::read_link(source)?;
        if target.is_absolute() {
            if !target.starts_with("/nix/store") {
                return Err(std::io::Error::other(format!(
                    "legacy Hangar symlink `{}` escapes the approved compatibility root",
                    source.display()
                )));
            }
        } else if !relative_target_stays_in_root(source, &target, root) {
            return Err(std::io::Error::other(format!(
                "legacy Hangar symlink `{}` escapes its migration root",
                source.display()
            )));
        }
        create_migration_symlink(&target, destination, source)?;
        return Ok(());
    }
    if metadata.is_dir() {
        fs::create_dir(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy_migration_tree(&entry.path(), &destination.join(entry.file_name()), root)?;
        }
        fs::set_permissions(destination, metadata.permissions())?;
        return Ok(());
    }
    if metadata.is_file() {
        fs::copy(source, destination)?;
        fs::set_permissions(destination, metadata.permissions())?;
        return Ok(());
    }
    Err(std::io::Error::other(format!(
        "legacy Hangar contains unsupported node `{}`",
        source.display()
    )))
}

fn relative_target_stays_in_root(link: &Path, target: &Path, root: &Path) -> bool {
    let Some(parent) = link.parent() else {
        return false;
    };
    let Ok(relative_parent) = parent.strip_prefix(root) else {
        return false;
    };
    let mut normalized = root.join(relative_parent);
    for component in target.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(name) => normalized.push(name),
            std::path::Component::ParentDir => {
                if normalized == root || !normalized.pop() {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}

fn create_migration_symlink(target: &Path, destination: &Path, source: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        // `source` only tells Windows whether to make a dir or file symlink.
        let _ = source;
        std::os::unix::fs::symlink(target, destination)
    }
    #[cfg(windows)]
    {
        let target_is_dir = fs::metadata(source)
            .map(|metadata| metadata.is_dir())
            .unwrap_or(false);
        if target_is_dir {
            std::os::windows::fs::symlink_dir(target, destination)
        } else {
            std::os::windows::fs::symlink_file(target, destination)
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (target, destination, source);
        Err(std::io::Error::other(
            "legacy Hangar migration needs symlink support on this host",
        ))
    }
}

/// Refresh the immutable producer facts after the Nix provider publishes the
/// realization it just recorded in the project lock. The lock digest is part
/// of the Nix action key, so leaving the pre-publication digest in the closure
/// record would let a later replay accept stale provenance.
pub(crate) fn refresh_nix_lock_digest(
    roots: &Roots,
    entry: &StoreEntry,
    lock_digest: &str,
) -> std::io::Result<StoreEntry> {
    if lock_digest.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "cannot refresh a Nix producer with an empty project lock digest",
        ));
    }
    let mut producer = ProducerRecord::decode(&entry.producer_record)
        .map_err(std::io::Error::other)?;
    if producer.provider != "nix" {
        return Ok(entry.clone());
    }
    producer
        .facts
        .insert("nix.lock.digest".to_string(), lock_digest.to_string());
    producer.plan = crate::Comptime::Build::BuildPlanReplay::from_facts(producer.facts.clone())
        .map_err(std::io::Error::other)?;

    let mut refreshed = entry.clone();
    refreshed.producer_record = producer.encode();
    super::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        Closure::recover_closure_journal_unlocked(roots)?;
        Closure::register_entry_unlocked(roots, &refreshed)?;
        Ok(refreshed)
    })
}

fn canonical_producer(
    provider: &str,
    immutable_source: &str,
    source_digest: &str,
    identity: &CacheIdentity,
    mut facts: BTreeMap<String, String>,
) -> std::io::Result<String> {
    facts.insert("action.recipe".into(), identity.recipe_fingerprint.clone());
    facts.insert("closure.authority".into(), "hangar-cas".into());
    let plan = crate::Comptime::Build::BuildPlanReplay::from_facts(facts.clone())
        .map_err(std::io::Error::other)?;
    ProducerRecord::new(
        provider,
        immutable_source,
        source_digest,
        plan,
        "jetpack-std-provider",
        format!(
            "policy={}\nplatform={}",
            identity.policy_fingerprint, identity.platform
        ),
        facts,
    )
    .map(|record| record.encode())
    .map_err(std::io::Error::other)
}

const BUILD_SCRATCH_DIR: &str = "build-scratch";
const AUTO_CLEAN_STAMP: &str = ".last-auto-clean";
const STALE_AFTER: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const AUTO_CLEAN_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

/// Validate then seal a locally produced output before it becomes reusable.
/// Files keep executable bits but lose all write bits; directories are sealed
/// bottom-up. Canonical archive validation rejects external hardlinks first.
pub fn seal_local_output(path: &Path) -> std::io::Result<()> {
    super::Envelope::try_output_hash_of(&path.to_string_lossy())
        .map_err(std::io::Error::other)?;
    seal_node(path)
}

fn seal_node(path: &Path) -> std::io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Ok(());
    }
    if meta.is_dir() {
        for entry in fs::read_dir(path)? {
            seal_node(&entry?.path())?;
        }
    }
    let mut permissions = meta.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        permissions.set_mode(permissions.mode() & !0o222);
    }
    #[cfg(not(unix))]
    permissions.set_readonly(true);
    fs::set_permissions(path, permissions)
}

fn fsync_tree(path: &Path) -> std::io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Ok(());
    }
    if meta.is_dir() {
        for entry in fs::read_dir(path)? {
            fsync_tree(&entry?.path())?;
        }
    }
    sync_store_node(path, meta.is_dir())
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
        let staging = Ingest::recover_hangar_staging_unlocked(roots)?;
        let closure = Closure::recover_closure_journal_unlocked(roots)?;
        Ok(staging + closure)
    })
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
    theme.error_coded(
        "E2604",
        &failure.what(),
        &failure.why(),
        failure.fix(),
    );
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
        let entry = StoreEntry {
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
            producer_record: canonical_producer(
                "store-record",
                &format!("cas:{}", cache_identity.source_fingerprint),
                &envelope.output_hash,
                cache_identity,
                BTreeMap::from([("reference".into(), reference.to_string())]),
            )?,
            realized_at,
            last_used_at: now,
        };
        let created_dir = !dir.exists();
        let gc_root = dir.join(NIX_GC_ROOT);
        let had_gc_root = fs::symlink_metadata(&gc_root).is_ok();
        fs::create_dir_all(&dir)?;
        let registration = (|| {
            pin_nix_gc_root(&dir, &out)?;
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
        ProducerRecord::decode(&realized.producer.encode()).map_err(std::io::Error::other)?;
        let (out, bin, rlib) = canonicalize_local_output_unlocked(
            roots,
            &realized.out,
            &realized.bin,
            &realized.rlib,
            &realized.envelope.output_hash,
        )?;
        let mut named_outputs = BTreeMap::new();
        for (name, path) in &realized.named_outputs {
            let digest = if name == "out" {
                realized.envelope.output_hash.clone()
            } else {
                super::Envelope::try_output_hash_of(path).map_err(std::io::Error::other)?
            };
            named_outputs.insert(name.clone(), digest);
        }
        named_outputs.insert("out".into(), realized.envelope.output_hash.clone());
        let id = entry_id(
            &realized.name,
            &realized.version,
            &realized.reference,
            &out,
        );
        let dir = roots.hangar_dir().join(&id);
        let now = now_secs();
        let realized_at = read_meta(&dir).and_then(|meta| meta.realized_at).unwrap_or(now);
        let entry = StoreEntry {
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
            producer_record: realized.producer.encode(),
            realized_at,
            last_used_at: now,
        };
        let created_dir = !dir.exists();
        let gc_root = dir.join(NIX_GC_ROOT);
        let had_gc_root = fs::symlink_metadata(&gc_root).is_ok();
        fs::create_dir_all(&dir)?;
        let registration = (|| {
            pin_nix_gc_root(&dir, &out)?;
            register_entry_unlocked(roots, &entry)
        })();
        if let Err(error) = registration {
            Closure::rollback_registration_dir(&dir, created_dir, had_gc_root)?;
            return Err(error);
        }
        Ok(entry)
    })
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
    fs::create_dir_all(&objects).map_err(|error| {
        std::io::Error::new(error.kind(), format!("creating canonical object directory: {error}"))
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
                std::io::Error::new(error.kind(), format!("removing duplicate provider output: {error}"))
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
            std::io::Error::new(error.kind(), format!("publishing canonical provider output: {error}"))
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
            std::io::Error::new(error.kind(), format!("syncing canonical object directory: {error}"))
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

/// Keep every live Nix compatibility output reachable until JP11 imports its
/// closure into Hangar. A root on the top-level output protects its transitive
/// Nix closure. Missing fixture paths are not roots and remain readable only as
/// metadata.
fn pin_nix_gc_root(entry_dir: &Path, out: &str) -> std::io::Result<()> {
    let out_path = Path::new(out);
    if !out_path.starts_with("/nix/store") || !out_path.exists() {
        return Ok(());
    }
    pin_nix_gc_root_with(entry_dir, out_path, Path::new("nix-store"))
}

/// Startup migration for records written before JP0. Every real Nix output is
/// rooted before any command may consume or clean Hangar state.
pub fn migrate_nix_gc_roots(roots: &Roots) -> std::io::Result<usize> {
    migrate_nix_gc_roots_with(roots, Path::new("/nix/store"), Path::new("nix-store"))
}

fn migrate_nix_gc_roots_with(
    roots: &Roots,
    store_prefix: &Path,
    nix_store: &Path,
) -> std::io::Result<usize> {
    super::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        Closure::recover_closure_journal_unlocked(roots)?;
        let mut rooted = 0;
        for entry in list_unlocked(roots) {
            let out = Path::new(&entry.out);
            if !out.starts_with(store_prefix) || !out.exists() {
                continue;
            }
            let entry_dir = roots.hangar_dir().join(&entry.id);
            let root = entry_dir.join(NIX_GC_ROOT);
            if root.exists()
                && fs::canonicalize(&root).ok() == fs::canonicalize(out).ok()
            {
                continue;
            }
            if fs::symlink_metadata(&root).is_ok() {
                fs::remove_file(&root)?;
            }
            pin_nix_gc_root_with(&entry_dir, out, nix_store)?;
            rooted += 1;
        }
        Ok(rooted)
    })
}

fn pin_nix_gc_root_with(entry_dir: &Path, out: &Path, nix_store: &Path) -> std::io::Result<()> {
    let root = entry_dir.join(NIX_GC_ROOT);
    let output = Command::new(nix_store)
        .arg("--add-root")
        .arg(&root)
        .arg("--indirect")
        .arg("--realise")
        .arg(out)
        .output()?;
    if !output.status.success() {
        return Err(std::io::Error::other(format!(
            "could not create durable Nix GC root for `{}`: {}",
            out.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    if !root.exists() {
        return Err(std::io::Error::other(format!(
            "nix-store reported success but did not create GC root `{}`",
            root.display()
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
        .map(|metadata| !metadata.file_type().is_symlink() && (metadata.is_file() || metadata.is_dir()))
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
        && entry.cache_identity.policy_fingerprint == expectation.identity.policy_fingerprint;
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
            return Err(std::io::Error::other("verified tool lease has no executables"));
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
        let (_, file) = self
            .executables
            .iter()
            .find(|(member, _)| member == name)?;
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
            return Err(std::io::Error::other("leased consumer path contains parent traversal"));
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
                    (name.clone(), PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd())))
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
        return Err(std::io::Error::other("profile executable is not a regular file"));
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
        .find(|entry| {
            entry.reference == reference && entry.envelope.output_hash == output_hash
        })
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
            return Err(std::io::Error::other("profile executable changed while opening"));
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
            return Err(std::io::Error::other("profile projection changed while opening"));
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
        let _ = make_tree_writable_for_removal(&self.snapshot_root);
        let _ = fs::remove_dir_all(&self.snapshot_root);
    }
}

pub fn find_verified_by_reference(
    roots: &Roots,
    reference: &str,
    expectation: &CacheExpectation,
) -> std::io::Result<Option<VerifiedCacheHit>> {
    let _global = super::RuntimePolicy::acquire_lock(&roots.root, "hangar")?;
    let graph = Closure::closure_graph_structure_unlocked(roots)?;
    let entry = list_unlocked(roots)
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
    fs::create_dir_all(&leases)?;
    let snapshot_root = leases.join(format!(
        "{}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed),
        entry.id
    ));
    if snapshot_root.exists() {
        make_tree_writable_for_removal(&snapshot_root)?;
        fs::remove_dir_all(&snapshot_root)?;
    }
    if !Path::new(&entry.out).exists() {
        fs::create_dir_all(&snapshot_root)?;
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
            _wrapper_dir_handle: None,
        });
    }
    let mut hardlinks = BTreeMap::new();
    copy_snapshot_node(Path::new(&entry.out), &snapshot_root, &mut hardlinks)?;
    let digest = super::Envelope::try_output_hash_of(&snapshot_root.to_string_lossy())
        .map_err(std::io::Error::other)?;
    if digest != entry.envelope.output_hash {
        make_tree_writable_for_removal(&snapshot_root)?;
        fs::remove_dir_all(&snapshot_root)?;
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
        .then(|| Path::new(&entry.bin).strip_prefix(&entry.out).ok().map(PathBuf::from))
        .flatten();
    let executables = open_snapshot_executables(&snapshot_root, bin_relative.as_deref())?;
    let wrappers = create_exec_wrappers(&snapshot_root, &executables)?;
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
        _wrapper_dir_handle: wrappers.map(|wrapper| wrapper.directory),
    })
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
        .args(["--user", "--map-root-user", "--mount", "--propagation", "private"])
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
        return Err(wrapper_keeper_error(child, "creating private executable mount"));
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
        return Err(wrapper_keeper_error(child, "sealing private executable mount"));
    }
    let directory = fs::File::open(&root)?;
    clear_close_on_exec(&directory)?;
    let root = PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd()));
    drop(input);
    let status = child.wait()?;
    if !status.success() {
        return Err(std::io::Error::other("private executable mount keeper failed"));
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
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, format!("required system tool `{name}` not found")))
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
    // Data-only outputs need no named executable handoff. Keep them usable on
    // every tier-1 host while the protected service handles executable leases.
    if executables.is_empty() {
        return Ok(None);
    }
    Err(std::io::Error::other(
        "immutable executable PATH handoff is unavailable on this platform",
    ))
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
            return Err(std::io::Error::other("cache symlink snapshots need platform support"));
        }
    } else {
        return Err(std::io::Error::other("special file in cache snapshot"));
    }
    Ok(())
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

fn make_tree_writable_for_removal(path: &Path) -> std::io::Result<()> {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    if meta.is_dir() {
        let mut permissions = meta.permissions();
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions)?;
        for entry in fs::read_dir(path)? {
            make_tree_writable_for_removal(&entry?.path())?;
        }
    }
    Ok(())
}

fn hook_fact_mismatch(name: &str, expected: &str, actual: Option<&str>) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!(
            "adapter build hook provenance mismatch for `{name}`: expected `{expected}`, got `{}`",
            actual.unwrap_or("<missing>")
        ),
    )
}

fn validate_adapter_hook_producer(
    producer: &ProducerRecord,
    plan: &jet_env_model::ModuleEval::AdapterPlan,
    expectation: &CacheExpectation,
) -> std::io::Result<()> {
    let jet_env_model::ModuleEval::AdapterRecipe::Build(recipe) = &plan.recipe else {
        return Ok(());
    };
    if producer.provider != "adapter" {
        return Err(hook_fact_mismatch(
            "provider",
            "adapter",
            Some(&producer.provider),
        ));
    }
    if producer.source_digest != expectation.identity.source_fingerprint {
        return Err(hook_fact_mismatch(
            "source-digest",
            &expectation.identity.source_fingerprint,
            Some(&producer.source_digest),
        ));
    }
    if producer.facts.get("adapter.source").map(String::as_str) != Some(plan.source.as_str()) {
        return Err(hook_fact_mismatch(
            "provider-source",
            &plan.source,
            producer.facts.get("adapter.source").map(String::as_str),
        ));
    }
    let identity = super::Provider::adapter_action_identity(
        plan,
        recipe,
        &expectation.identity.source_fingerprint,
        &expectation.identity.platform,
    );
    if producer.facts.get("build.identity").map(String::as_str) != Some(identity.as_str()) {
        return Err(hook_fact_mismatch(
            "build.identity",
            &identity,
            producer.facts.get("build.identity").map(String::as_str),
        ));
    }
    let capabilities = recipe.declared_capabilities().join(",");
    if producer.facts.get("build.capabilities").map(String::as_str)
        != Some(capabilities.as_str())
    {
        return Err(hook_fact_mismatch(
            "build.capabilities",
            &capabilities,
            producer.facts.get("build.capabilities").map(String::as_str),
        ));
    }
    Ok(())
}

fn validate_cached_adapter_hook(
    entry: &StoreEntry,
    plan: &jet_env_model::ModuleEval::AdapterPlan,
    expectation: &CacheExpectation,
) -> std::io::Result<()> {
    if !matches!(
        &plan.recipe,
        jet_env_model::ModuleEval::AdapterRecipe::Build(_)
    ) {
        return Ok(());
    }
    if entry.cache_identity != expectation.identity {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "adapter cache identity is not the exact build-hook identity",
        ));
    }
    let producer = ProducerRecord::decode(&entry.producer_record)
        .map_err(std::io::Error::other)?;
    validate_adapter_hook_producer(&producer, plan, expectation)
}

fn bind_adapter_hook_identity(
    realized: &mut super::Provider::Realized,
    plan: &jet_env_model::ModuleEval::AdapterPlan,
    expectation: &CacheExpectation,
    ctx: &super::Provider::Ctx<'_>,
) -> std::io::Result<()> {
    if !matches!(
        &plan.recipe,
        jet_env_model::ModuleEval::AdapterRecipe::Build(_)
    ) {
        return Ok(());
    }
    validate_adapter_hook_producer(&realized.producer, plan, expectation)?;
    let expected = super::Provider::adapter_cache_identity(
        &expectation.identity.source_fingerprint,
        realized
            .producer
            .facts
            .get("build.identity")
            .ok_or_else(|| hook_fact_mismatch("build.identity", "exact subject", None))?,
        ctx,
    );
    if expected != expectation.identity {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "adapter cache expectation is not derived from the exact build-hook subject",
        ));
    }
    for (name, expected, actual) in [
        (
            "source-fingerprint",
            &expectation.identity.source_fingerprint,
            &realized.cache_identity.source_fingerprint,
        ),
        (
            "policy-fingerprint",
            &expectation.identity.policy_fingerprint,
            &realized.cache_identity.policy_fingerprint,
        ),
        (
            "platform",
            &expectation.identity.platform,
            &realized.cache_identity.platform,
        ),
    ] {
        if expected != actual {
            return Err(hook_fact_mismatch(
                name,
                expected.as_str(),
                Some(actual.as_str()),
            ));
        }
    }
    realized.cache_identity = expected;
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
            plan,
            expectation,
            ..
        } => (
            format!("adapt:{}:{}", plan.name, plan.source),
            Some((*expectation).clone()),
        ),
    };

    if let (Some(candidate), Some(expectation)) =
        (find_by_reference(roots, &reference), expectation.as_ref())
    {
        match find_verified_by_reference(roots, &reference, expectation)
            .map_err(RealizeError::Store)?
        {
            Some(hit) => {
                if let RealizeRequest::Adapter { plan, .. } = &request {
                    validate_cached_adapter_hook(&hit.entry, plan, expectation)
                        .map_err(RealizeError::Store)?;
                }
                return Ok(VerifiedRealization {
                    entry: hit.entry,
                    source_state: super::Provider::SourceState::Cached,
                    lease: hit.lease,
                });
            }
            None => {
                let proof = verify_cache_entry(roots, &candidate, &reference, expectation);
                let mut failure = integrity_failure(roots, &candidate, expectation, proof);
                if let Err(error) = quarantine_invalid_entry(roots, &candidate, expectation) {
                    failure.actual = format!("{}; quarantine failed: {error}", failure.actual);
                }
                return Err(RealizeError::Integrity(failure));
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
                return Ok(VerifiedRealization {
                    entry: hit.entry,
                    source_state: super::Provider::SourceState::Cached,
                    lease: hit.lease,
                });
            }
        }
    }

    let realized = match request {
        RealizeRequest::Package { spec, table } => {
            super::Provider::realize(spec, table, ctx).map_err(RealizeError::Provider)?
        }
        RealizeRequest::Adapter {
            plan,
            table,
            expectation,
        } => {
            let (tools, _dependency_leases) = realize_adapter_tools(roots, ctx, plan, table)?;
            let mut realized = super::Provider::realize_adapter(plan, ctx, expectation, &tools)
                .map_err(RealizeError::Provider)?;
            bind_adapter_hook_identity(&mut realized, plan, expectation, ctx)
                .map_err(RealizeError::Store)?;
            realized
        }
    };
    // The provider snapshots its Nix lock facts before producing bytes. Check
    // that snapshot again immediately before the Store registration so a
    // concurrent lock edit cannot detach the producer record from the output
    // that is about to become reusable.
    super::Provider::validate_nix_lock_before_store(ctx, &realized)
        .map_err(RealizeError::Provider)?;
    let mut entry = record_realized_mode(roots, &realized).map_err(RealizeError::Store)?;
    entry = super::Provider::record_nix_lock_after_store(ctx, roots, &entry)
        .map_err(RealizeError::Provider)?;
    promote_shared_entry(roots, &entry).map_err(RealizeError::Store)?;
    let lease = snapshot_lease(roots, &entry).map_err(RealizeError::Store)?;
    Ok(VerifiedRealization {
        entry,
        source_state: realized.source_state,
        lease,
    })
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
        let spec = if dependency.source.is_empty()
            || dependency.source == crate::Syntax::DEFAULT_SOURCE
        {
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
        let realized = realize_verified(
            roots,
            ctx,
            RealizeRequest::Package {
                spec: &spec,
                table,
            },
        )?;
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
        return root.exists()
            && fs::canonicalize(&root).ok() == fs::canonicalize(out).ok();
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

/// One line of `jet hangar du` output: a realized object, its on-disk size, and
/// whether it was built from source (vs substituted/nix).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuEntry {
    pub id: String,
    pub name: String,
    pub bytes: u64,
    /// True when the A4 provenance shows a first-party source build.
    pub source_built: bool,
}

/// D-JPK-GC1 / U22: honest per-object disk usage. Sizes each realized object's
/// output tree (source-built objects live under the hangar; nix outputs live in
/// `/nix/store` and size 0 here since Jetpack doesn't own those bytes). A
/// source-built object is counted honestly, envelope and all.
pub fn du(roots: &Roots) -> Vec<DuEntry> {
    list(roots)
        .into_iter()
        .map(|e| {
            let bytes = dir_size(std::path::Path::new(&e.out));
            let source_built = e.envelope.provenance.contains("core-");
            DuEntry {
                id: e.id,
                name: e.name,
                bytes,
                source_built,
            }
        })
        .collect()
}

/// Total bytes on disk of a directory tree (0 if it isn't a local dir, e.g. a
/// `/nix/store` path Jetpack references but does not own).
fn dir_size(path: &std::path::Path) -> u64 {
    if !path.is_dir() {
        return 0;
    }
    let mut total = 0u64;
    if let Ok(rd) = fs::read_dir(path) {
        for ent in rd.flatten() {
            let p = ent.path();
            if p.is_dir() {
                total += dir_size(&p);
            } else if let Ok(meta) = p.metadata() {
                total += meta.len();
            }
        }
    }
    total
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CleanReport {
    pub removed_objects: usize,
    pub removed_bytes: u64,
    pub swept_tmp: usize,
    pub swept_tmp_bytes: u64,
    pub optimized_files: usize,
    pub optimized_bytes: u64,
}

impl CleanReport {
    pub fn is_empty(&self) -> bool {
        self.removed_objects == 0
            && self.removed_bytes == 0
            && self.swept_tmp == 0
            && self.swept_tmp_bytes == 0
            && self.optimized_files == 0
            && self.optimized_bytes == 0
    }
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
    let id = Lifecycle::RootId::new(format!(
        "profile-generation:{owner}:{profile}:{generation}"
    ))?;
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
    let expected_targets = targets.iter().cloned().collect::<BTreeSet<_>>();
    let snapshot = Lifecycle::snapshot(roots)?;
    let (incarnation, resume) = match snapshot.roots.get(&id) {
        Some(root) => {
            if root.identity.kind != Lifecycle::RootKind::ExternalConsumer
                || root.identity.producer.as_str() != consumer
                || root.phase == Lifecycle::RootPhase::Tombstoned
            {
                return Err(std::io::Error::other(
                    "external consumer root disagrees with immutable identity",
                ));
            }
            if root.identity.witness.as_str() == witness && root.targets == expected_targets {
                if root.phase == Lifecycle::RootPhase::Committed {
                    return Ok(None);
                }
                (root.identity.incarnation.get(), true)
            } else {
                return Err(std::io::Error::other(
                    "external consumer root cannot replace an immutable publication",
                ));
            }
        }
        None => (1, false),
    };
    let witness = Lifecycle::RootWitness::new(witness)?;
    let incarnation = Lifecycle::Incarnation::new(incarnation)?;
    if !resume {
        let identity = Lifecycle::RootIdentity::new(
            Lifecycle::RootKind::ExternalConsumer,
            id.clone(),
            Lifecycle::ProducerId::new(consumer)?,
            incarnation,
            witness.clone(),
        );
        Lifecycle::prepare(
            roots,
            identity,
            targets,
            Lifecycle::LifecycleTimestamp::from_unix_seconds(at),
        )?;
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

/// A manually retained Hangar closure, projected for the external-root CLI
/// and consumers. The lifecycle WAL remains the source of truth; this is only
/// the typed Store view of one committed root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExternalRootView {
    pub(crate) label: String,
    pub(crate) reference: String,
    pub(crate) etag: String,
    pub(crate) closure_size: usize,
    pub(crate) prepared: bool,
    pub(crate) expires_at: Option<u64>,
}

#[derive(Debug)]
pub(crate) enum ExternalRootError {
    Conflict {
        label: String,
        expected: Option<String>,
        current: Option<String>,
    },
    ReferenceNotFound(String),
    Store(std::io::Error),
}

impl std::fmt::Display for ExternalRootError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Conflict {
                label,
                expected,
                current,
            } => {
                write!(
                    f,
                    "external root `{label}` changed before the request applied (expected {:?}, current {:?})",
                    expected,
                    current
                )
            }
            Self::ReferenceNotFound(reference) => {
                write!(f, "no Hangar entry matches `{reference}`")
            }
            Self::Store(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for ExternalRootError {}

fn validate_external_root_label(label: &str) -> std::io::Result<()> {
    if label.is_empty()
        || label.len() > 128
        || label == "."
        || label == ".."
        || label.contains('/')
        || label.contains('\\')
        || label.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "external root label must be one safe path component",
        ));
    }
    Ok(())
}

fn manual_root_id(
    principal: &Lifecycle::ProducerId,
    label: &str,
) -> std::io::Result<Lifecycle::RootId> {
    Lifecycle::RootId::new(format!("manual:{}:{label}", principal.as_str()))
}

fn manual_root_witness(
    principal: &Lifecycle::ProducerId,
    label: &str,
    reference: &str,
    targets: &[String],
) -> std::io::Result<Lifecycle::RootWitness> {
    let mut canonical = String::from("jet-manual-root-v1\n");
    for value in [principal.as_str(), label, reference] {
        canonical.push_str(&value.len().to_string());
        canonical.push('\n');
        canonical.push_str(value);
        canonical.push('\n');
    }
    for target in targets {
        canonical.push_str(target);
        canonical.push('\n');
    }
    Lifecycle::RootWitness::new(format!(
        "sha256-{}",
        SHA256::sha256_hex(canonical.as_bytes())
    ))
}

fn external_root_targets(
    roots: &Roots,
    reference: &str,
) -> Result<Vec<String>, ExternalRootError> {
    let entry = list_checked(roots)
        .map_err(ExternalRootError::Store)?
        .into_iter()
        .find(|entry| entry.reference == reference)
        .ok_or_else(|| ExternalRootError::ReferenceNotFound(reference.to_string()))?;
    let mut targets = Closure::closure_of(roots, &entry.envelope.output_hash)
        .map_err(ExternalRootError::Store)?;
    targets.sort();
    targets.dedup();
    if targets.is_empty() {
        return Err(ExternalRootError::Store(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Hangar entry `{reference}` has no closure objects"),
        )));
    }
    Ok(targets)
}

pub(crate) fn external_root_closure_size(
    roots: &Roots,
    reference: &str,
) -> Result<usize, ExternalRootError> {
    Ok(external_root_targets(roots, reference)?.len())
}

fn external_root_view(root: &Lifecycle::LifecycleRoot) -> std::io::Result<ExternalRootView> {
    let label = root.metadata.label.clone().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "manual lifecycle root has no label metadata",
        )
    })?;
    let reference = root.metadata.reference.clone().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "manual lifecycle root has no reference metadata",
        )
    })?;
    Ok(ExternalRootView {
        label,
        reference,
        etag: root.etag().render(),
        closure_size: root.targets.len(),
        prepared: root.phase == Lifecycle::RootPhase::Prepared,
        expires_at: root.metadata.expires_at.map(|value| value.get()),
    })
}

fn map_external_root_error(label: &str, error: std::io::Error) -> ExternalRootError {
    if let Some(conflict) = error
        .get_ref()
        .and_then(|cause| cause.downcast_ref::<Lifecycle::CasConflict>())
    {
        return ExternalRootError::Conflict {
            label: label.to_string(),
            expected: conflict.expected.clone(),
            current: conflict.current.clone(),
        };
    }
    ExternalRootError::Store(error)
}

/// Atomically create or replace one manually retained Hangar closure.
/// Lifecycle owns the typed identity, CAS check, expiry metadata, and
/// prepare/commit journal sequence while this Store adapter resolves the
/// reference to the complete closure under Hangar authority.
pub(crate) fn register_external_root_at(
    roots: &Roots,
    principal: &str,
    label: &str,
    reference: &str,
    expires_at: Option<u64>,
    expected_etag: Option<&str>,
    at: u64,
) -> Result<ExternalRootView, ExternalRootError> {
    validate_external_root_label(label).map_err(ExternalRootError::Store)?;
    let producer = Lifecycle::ProducerId::new(principal.to_string())
        .map_err(ExternalRootError::Store)?;
    let metadata = Lifecycle::RootMetadata::manual(
        label,
        reference,
        expires_at.map(Lifecycle::LifecycleTimestamp::from_unix_seconds),
    )
    .map_err(ExternalRootError::Store)?;
    let id = manual_root_id(&producer, label).map_err(ExternalRootError::Store)?;
    let mut targets = external_root_targets(roots, reference)?;
    let witness = manual_root_witness(&producer, label, reference, &targets)
        .map_err(ExternalRootError::Store)?;
    let update = Lifecycle::RootUpdate {
        identity: Lifecycle::RootIdentity::new(
            Lifecycle::RootKind::Manual,
            id.clone(),
            producer,
            Lifecycle::Incarnation::new(1).map_err(ExternalRootError::Store)?,
            witness,
        ),
        targets: std::mem::take(&mut targets),
        metadata,
        expected_etag: expected_etag.map(str::to_string),
        at: Lifecycle::LifecycleTimestamp::from_unix_seconds(at),
    };
    let snapshot = Lifecycle::atomic_update(roots, update)
        .map_err(|error| map_external_root_error(label, error))?;
    let root = snapshot.roots.get(&id).ok_or_else(|| {
        ExternalRootError::Store(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("manual lifecycle root `{label}` disappeared after update"),
        ))
    })?;
    external_root_view(root).map_err(ExternalRootError::Store)
}

pub(crate) fn list_external_roots(
    roots: &Roots,
    principal: &str,
) -> Result<Vec<ExternalRootView>, ExternalRootError> {
    let producer = Lifecycle::ProducerId::new(principal.to_string())
        .map_err(ExternalRootError::Store)?;
    Lifecycle::snapshot(roots)
        .map_err(ExternalRootError::Store)?
        .roots
        .values()
        .filter(|root| {
            root.identity.kind == Lifecycle::RootKind::Manual
                && root.identity.producer == producer
                && root.phase != Lifecycle::RootPhase::Tombstoned
        })
        .map(external_root_view)
        .collect::<Result<Vec<_>, _>>()
        .map_err(ExternalRootError::Store)
}

pub(crate) fn unregister_external_root_at(
    roots: &Roots,
    principal: &str,
    label: &str,
    expected_etag: &str,
    at: u64,
) -> Result<(), ExternalRootError> {
    validate_external_root_label(label).map_err(ExternalRootError::Store)?;
    let producer = Lifecycle::ProducerId::new(principal.to_string())
        .map_err(ExternalRootError::Store)?;
    let id = manual_root_id(&producer, label).map_err(ExternalRootError::Store)?;
    let snapshot = Lifecycle::snapshot(roots).map_err(ExternalRootError::Store)?;
    let Some(root) = snapshot.roots.get(&id) else {
        return Err(ExternalRootError::Store(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("external root `{label}` was not found"),
        )));
    };
    if root.identity.kind != Lifecycle::RootKind::Manual
        || root.identity.producer != producer
    {
        return Err(ExternalRootError::Store(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("external root `{label}` is owned by another producer"),
        )));
    }
    Lifecycle::atomic_remove(
        roots,
        &id,
        expected_etag,
        Lifecycle::LifecycleTimestamp::from_unix_seconds(at),
    )
    .map_err(|error| map_external_root_error(label, error))?;
    Ok(())
}

pub(crate) fn reconcile_profile_generation_root(
    roots: &Roots,
    owner: &str,
    profile: &str,
    generation: u64,
    witness: &str,
    mut targets: Vec<String>,
    at: u64,
) -> std::io::Result<Option<PreparedProfileGenerationRoot>> {
    targets.sort();
    targets.dedup();
    let id = Lifecycle::RootId::new(format!(
        "profile-generation:{owner}:{profile}:{generation}"
    ))?;
    let expected_targets = targets.iter().cloned().collect::<BTreeSet<_>>();
    if let Some(root) = Lifecycle::snapshot(roots)?.roots.get(&id) {
        if root.identity.kind != Lifecycle::RootKind::ProfileGeneration
            || root.identity.producer.as_str() != "jetpack-profile-generation"
            || root.identity.incarnation.get() != 1
            || root.identity.witness.as_str() != witness
            || root.targets != expected_targets
            || root.phase == Lifecycle::RootPhase::Tombstoned
        {
            return Err(std::io::Error::other(
                "generation root disagrees with immutable metadata",
            ));
        }
        if root.phase == Lifecycle::RootPhase::Committed {
            return Ok(None);
        }
        return Ok(Some(PreparedProfileGenerationRoot {
            id,
            incarnation: Lifecycle::Incarnation::new(1)?,
            witness: Lifecycle::RootWitness::new(witness)?,
        }));
    }
    let prepared = prepare_profile_generation_root(
        roots,
        owner,
        profile,
        generation,
        witness,
        targets,
        at,
    )?;
    Ok(Some(prepared))
}

fn live_roots_unlocked(roots: &Roots) -> std::io::Result<LiveRoots> {
    let mut live = current_lock_roots();
    let lifecycle = Lifecycle::protected_targets_unlocked(roots)?;
    let graph = Closure::lifecycle_closure_graph_unlocked(roots)?;
    for target in lifecycle {
        live.output_hashes.extend(graph.closure(&target));
    }
    Ok(live)
}

/// D-JPK-GC1=B / U22: collect only unreferenced stale hangar objects, sweep
/// orphan build scratch, then optimize duplicate Jet-owned files. Lockfile
/// reachable entries and unknown legacy records are retained.
pub fn clean_plan(roots: &Roots) -> std::io::Result<CleanReport> {
    let store = roots.hangar_dir();
    if !store.exists() {
        return Ok(CleanReport::default());
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
    Ok(report)
}

pub fn clean(roots: &Roots) -> std::io::Result<CleanReport> {
    super::RuntimePolicy::with_lock(&roots.root, "hangar", || clean_unlocked(roots))
}

fn clean_unlocked(roots: &Roots) -> std::io::Result<CleanReport> {
    let store = roots.hangar_dir();
    fs::create_dir_all(&store)?;
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
        fs::create_dir_all(&hangar)?;
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
    let Ok(rd) = fs::read_dir(&root) else {
        return Ok(report);
    };
    for ent in rd.flatten() {
        let path = ent.path();
        if super::Provider::active_tmp_marker_is_live(&path) {
            continue;
        }
        report.swept_tmp += 1;
        report.swept_tmp_bytes += dir_size(&path);
    }
    Ok(report)
}

fn sweep_build_scratch(hangar: &Path) -> std::io::Result<CleanReport> {
    let root = hangar.join(BUILD_SCRATCH_DIR);
    let mut report = CleanReport::default();
    let Ok(rd) = fs::read_dir(&root) else {
        return Ok(report);
    };
    for ent in rd.flatten() {
        let path = ent.path();
        if super::Provider::active_tmp_marker_is_live(&path) {
            continue;
        }
        let bytes = dir_size(&path);
        if path.is_dir() {
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
    if !objects.is_dir() {
        return Ok(report);
    }
    fs::create_dir_all(&cas)?;
    for ent in fs::read_dir(&objects)?.flatten() {
        let path = ent.path();
        let name = ent.file_name().to_string_lossy().into_owned();
        if !path.is_dir() || name.ends_with(PARTIAL_SUFFIX) {
            continue;
        }
        make_tree_writable_for_removal(&path)?;
        for file in files_under(&path) {
            let Ok(meta) = fs::metadata(&file) else {
                continue;
            };
            if !meta.is_file() || meta.len() == 0 {
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
            if !cas_file.exists() {
                let tmp = cas.join(format!("{digest}.partial"));
                fs::write(&tmp, &bytes)?;
                fs::set_permissions(&tmp, meta.permissions())?;
                fs::rename(&tmp, &cas_file)?;
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
    let hangar = roots.hangar_dir();
    let allow = !entry.platform_artifact_kind.is_empty();
    let graph = closure_graph(roots).map_err(|error| IngestError::Invalid(error.to_string()))?;
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
            IngestError::Invalid(format!("closure graph output `{name}` is missing `{expected}`"))
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
    let tmp = file.with_extension(format!("jet-dedup-{}", std::process::id()));
    fs::rename(file, &tmp)?;
    match fs::hard_link(first, file) {
        Ok(()) => {
            let _ = fs::remove_file(&tmp);
            Ok(())
        }
        Err(e) => {
            let _ = fs::rename(&tmp, file);
            Err(e)
        }
    }
}

fn object_dirs(hangar: &Path) -> std::io::Result<Vec<fs::DirEntry>> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(hangar) {
        for ent in rd.flatten() {
            let path = ent.path();
            let name = ent.file_name().to_string_lossy().into_owned();
            let reserved = name == BUILD_SCRATCH_DIR
                || name == STAGE_DIR
                || name == OBJECTS_DIR
                || name == CAS_DIR
                || name == REFERRERS_DIR
                || name == "lifecycle-db"
                || name == "closure-db"
                || name == "quarantine"
                || name.starts_with('.');
            if path.is_dir() && !reserved {
                out.push(ent);
            }
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
            if p.is_dir() {
                out.extend(files_under(&p));
            } else {
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
#[allow(dead_code)]
pub(crate) mod Lifecycle;
#[cfg(test)]
mod Tests;
