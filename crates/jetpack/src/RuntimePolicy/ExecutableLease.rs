//! Shared authenticated executable-lease protocol.
//!
//! The platform launchers are deliberately outside this module.  This file
//! owns the wire contract and the durable lease state they all consume:
//! authenticated bounded requests, kernel-backed ownership, immutable
//! complete generations, and fail-closed recovery.  A platform service can
//! handle the same frame without resolving a package or consulting a
//! provider.

use super::{lock_state, LockState};
use crate::TrustRoot::{os_random_bytes, TrustKey};
use crate::SHA256;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::io::{self, ErrorKind, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const PROTOCOL_HEADER: &str = "JET-EXECUTABLE-LEASE/1";
const SERVICE_DIR: &str = "lease-service";
const AUTH_KEY_FILE: &str = "auth.key";
const RECORDS_DIR: &str = "leases";
const RECEIPT_FILE: &str = "receipt";
const COMPLETE_FILE: &str = "complete";
const LEASE_STATE_SCOPE: &str = "lease-state";
const MAX_FRAME_BYTES: usize = 64 * 1024;
const MAX_FIELD_BYTES: usize = 16 * 1024;
const MAX_MEMBERS: usize = 4096;
const MAX_CONTAINER_NAME: usize = 256;
const OWNER_SCOPE_PREFIX: &str = "executable-lease-owner-";
const OWNER_LOCK_SCOPE: &str = "owner";

static SEQUENCE: AtomicU64 = AtomicU64::new(0);
static PUBLICATION_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static RECLAIM_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Operation {
    Acquire,
    Replace,
    Rollback,
    Release,
}

impl Operation {
    fn as_str(self) -> &'static str {
        match self {
            Self::Acquire => "acquire",
            Self::Replace => "replace",
            Self::Rollback => "rollback",
            Self::Release => "release",
        }
    }

    fn parse(value: &str) -> io::Result<Self> {
        match value {
            "acquire" => Ok(Self::Acquire),
            "replace" => Ok(Self::Replace),
            "rollback" => Ok(Self::Rollback),
            "release" => Ok(Self::Release),
            _ => Err(invalid("unknown executable-lease operation")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LeaseMember {
    pub(crate) name: String,
    pub(crate) digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LeaseRequest {
    operation: Operation,
    key_id: String,
    request_id: String,
    lease_id: String,
    generation: u64,
    owner_pid: u32,
    owner_scope: String,
    package: String,
    version: String,
    reference: String,
    output_digest: String,
    previous_output_digest: String,
    snapshot_rel: String,
    members: Vec<LeaseMember>,
    nonce: String,
    mac: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LeaseReceipt {
    pub(crate) lease_id: String,
    pub(crate) generation: u64,
    pub(crate) owner_pid: u32,
    pub(crate) owner_scope: String,
    pub(crate) package: String,
    pub(crate) version: String,
    pub(crate) reference: String,
    pub(crate) output_digest: String,
    pub(crate) snapshot_rel: String,
    pub(crate) members: Vec<LeaseMember>,
    pub(crate) mac: String,
}

pub(crate) struct ExecutableLeaseProtocol {
    root: PathBuf,
    key: TrustKey,
}

impl ExecutableLeaseProtocol {
    pub(crate) fn open(root: &Path) -> io::Result<Self> {
        ensure_real_directory(root, "Jetpack root")?;
        let service = root.join(SERVICE_DIR);
        ensure_real_directory(&service, "executable lease service state")?;
        let key_bytes = load_or_create_key(&service)?;
        let key = TrustKey::from_secret(key_bytes).map_err(io::Error::other)?;
        Ok(Self {
            root: root.to_path_buf(),
            key,
        })
    }

    /// Serialize the short lease-state transitions independently of the
    /// store's broader hangar lock. The service lock is the common boundary
    /// for owner acquisition, publication, release, and recovery, including
    /// callers that already hold a hangar lock themselves.
    fn with_lease_state_lock<T>(
        &self,
        operation: impl FnOnce() -> io::Result<T>,
    ) -> io::Result<T> {
        super::with_lock(&self.root.join(SERVICE_DIR), LEASE_STATE_SCOPE, operation)
    }

    /// Hold this lock for the lifetime of the process handoff.  Kernel lock
    /// ownership, rather than a PID or timestamp, is the stale-lease fact.
    pub(crate) fn acquire_owner(&self, lease_id: &str) -> io::Result<super::FileLock> {
        self.with_lease_state_lock(|| {
            let record = self.record_dir(lease_id)?;
            super::acquire_lease_lock(&record, OWNER_LOCK_SCOPE)
        })
    }

    pub(crate) fn owner_lock_path(&self, lease_id: &str) -> io::Result<PathBuf> {
        let record = self.existing_record_dir(lease_id)?;
        Ok(super::lock_path_for_scope(&record, OWNER_LOCK_SCOPE))
    }

    pub(crate) fn new_lease_id(&self) -> io::Result<String> {
        random_id(16)
    }

    pub(crate) fn owner_scope(lease_id: &str) -> io::Result<String> {
        owner_scope(lease_id)
    }

    pub(crate) fn prepare_snapshot(
        &self,
        lease_id: &str,
        owner_scope: &str,
        snapshot_root: &Path,
        package: &str,
        version: &str,
        reference: &str,
        output_digest: &str,
        members: &[LeaseMember],
    ) -> io::Result<(LeaseRequest, super::FileLock)> {
        let owner_lock = self.acquire_owner(lease_id)?;
        let request = self.request(
            Operation::Acquire,
            lease_id,
            1,
            owner_scope,
            snapshot_root,
            package,
            version,
            reference,
            output_digest,
            "",
            members,
        )?;
        Ok((request, owner_lock))
    }

    // Card #963 owns wiring replacement into the profile/update path; this
    // authenticated producer is exercised here and consumed by that slice.
    #[allow(dead_code)]
    pub(crate) fn prepare_replacement(
        &self,
        previous: &LeaseReceipt,
        snapshot_root: &Path,
        output_digest: &str,
        members: &[LeaseMember],
    ) -> io::Result<Vec<u8>> {
        let request = self.request(
            Operation::Replace,
            &previous.lease_id,
            next_generation(previous.generation)?,
            &previous.owner_scope,
            snapshot_root,
            &previous.package,
            &previous.version,
            &previous.reference,
            output_digest,
            &previous.output_digest,
            members,
        )?;
        self.encode_request(&request)
    }

    #[allow(dead_code)]
    pub(crate) fn prepare_rollback(
        &self,
        receipt: &LeaseReceipt,
        target_generation: u64,
    ) -> io::Result<Vec<u8>> {
        if target_generation == 0 || target_generation >= receipt.generation {
            return Err(invalid("rollback target is not an older lease generation"));
        }
        let target = self.read_generation(&receipt.lease_id, target_generation)?;
        let request = self.request(
            Operation::Rollback,
            &receipt.lease_id,
            next_generation(receipt.generation)?,
            &receipt.owner_scope,
            &self.root.join(&target.snapshot_rel),
            &target.package,
            &target.version,
            &target.reference,
            &target.output_digest,
            &receipt.output_digest,
            &target.members,
        )?;
        self.encode_request(&request)
    }

    /// The client-to-service wire boundary.  The service verifies the frame,
    /// the owner lock, the locked digest, and the snapshot before publishing
    /// an immutable complete receipt.
    pub(crate) fn accept_snapshot(
        &self,
        frame: &[u8],
        locked_output_digest: &str,
        snapshot_root: &Path,
    ) -> io::Result<LeaseReceipt> {
        self.with_lease_state_lock(|| {
            self.accept_snapshot_unlocked(frame, locked_output_digest, snapshot_root)
        })
    }

    fn accept_snapshot_unlocked(
        &self,
        frame: &[u8],
        locked_output_digest: &str,
        snapshot_root: &Path,
    ) -> io::Result<LeaseReceipt> {
        let request = self.decode_request(frame)?;
        if !matches!(
            request.operation,
            Operation::Acquire | Operation::Replace | Operation::Rollback
        ) {
            return Err(invalid("lease request is not a snapshot publication"));
        }
        self.validate_owner(&request)?;
        validate_digest(locked_output_digest, "locked output digest")?;
        if request.output_digest != locked_output_digest {
            return Err(invalid("lease request disagrees with the locked output digest"));
        }
        let requested_snapshot = self.snapshot_path(&request.snapshot_rel)?;
        if requested_snapshot != snapshot_root {
            return Err(invalid("lease request snapshot does not match the handoff"));
        }
        let metadata = fs::symlink_metadata(snapshot_root)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(invalid("executable lease snapshot is not a real directory"));
        }
        let actual = crate::Envelope::try_output_hash_of(&snapshot_root.to_string_lossy())
            .map_err(io::Error::other)?;
        if actual != locked_output_digest {
            return Err(invalid("executable lease snapshot digest changed before publication"));
        }
        validate_members(&request.members)?;
        let previous = self.current_receipt(&request.lease_id)?;
        match request.operation {
            Operation::Acquire => {
                if request.generation != 1 || previous.is_some() {
                    return Err(invalid("executable lease acquisition is not the first generation"));
                }
            }
            Operation::Replace => {
                let previous = previous
                    .ok_or_else(|| invalid("executable lease replacement has no base generation"))?;
                if request.generation != next_generation(previous.generation)?
                    || request.previous_output_digest != previous.output_digest
                {
                    return Err(invalid("executable lease replacement base is stale"));
                }
            }
            Operation::Rollback => {
                let previous = previous
                    .ok_or_else(|| invalid("executable lease rollback has no base generation"))?;
                if request.generation != next_generation(previous.generation)?
                    || request.previous_output_digest != previous.output_digest
                {
                    return Err(invalid("executable lease rollback base is stale"));
                }
                if !self.rollback_target_matches(&request, previous.generation)? {
                    return Err(invalid("executable lease rollback target is not a prior generation"));
                }
            }
            _ => unreachable!(),
        }
        let receipt = LeaseReceipt {
            lease_id: request.lease_id.clone(),
            generation: request.generation,
            owner_pid: request.owner_pid,
            owner_scope: request.owner_scope.clone(),
            package: request.package.clone(),
            version: request.version.clone(),
            reference: request.reference.clone(),
            output_digest: request.output_digest.clone(),
            snapshot_rel: request.snapshot_rel.clone(),
            members: request.members.clone(),
            mac: String::new(),
        };
        self.publish_receipt(receipt)
    }

    pub(crate) fn validate_snapshot(
        &self,
        lease_id: &str,
        generation: u64,
        owner_scope: &str,
        snapshot_root: &Path,
        expected_digest: &str,
    ) -> io::Result<()> {
        let receipt = self.read_generation(lease_id, generation)?;
        if receipt.owner_scope != owner_scope
            || receipt.snapshot_rel != self.relative_snapshot(snapshot_root)?
            || receipt.output_digest != expected_digest
        {
            return Err(invalid("executable lease receipt does not match the handoff"));
        }
        self.validate_owner_fields(lease_id, receipt.owner_pid, &receipt.owner_scope)?;
        let actual = crate::Envelope::try_output_hash_of(&snapshot_root.to_string_lossy())
            .map_err(io::Error::other)?;
        if actual != receipt.output_digest {
            return Err(invalid("executable lease receipt digest no longer matches"));
        }
        Ok(())
    }

    pub(crate) fn release(
        &self,
        lease_id: &str,
        generation: u64,
        owner_scope: &str,
    ) -> io::Result<()> {
        self.with_lease_state_lock(|| {
            self.release_unlocked(lease_id, generation, owner_scope)
        })
    }

    fn release_unlocked(
        &self,
        lease_id: &str,
        generation: u64,
        owner_scope: &str,
    ) -> io::Result<()> {
        let receipt = self.read_generation(lease_id, generation)?;
        if receipt.owner_scope != owner_scope {
            return Err(invalid("executable lease release owner mismatch"));
        }
        self.validate_owner_fields(lease_id, receipt.owner_pid, owner_scope)?;
        // A replaced lease keeps every complete older generation as rollback
        // authority. Releasing an old consumer must not erase that history;
        // only the current generation is eligible for removal.
        if self
            .current_receipt(lease_id)?
            .is_some_and(|current| current.generation != generation)
        {
            return Ok(());
        }
        let record = self.existing_record_dir(lease_id)?;
        let generation_dir = record.join("generations").join(generation.to_string());
        remove_owned_tree(&generation_dir)?;
        Ok(())
    }

    pub(crate) fn reap_empty_record(&self, lease_id: &str) -> io::Result<bool> {
        self.with_lease_state_lock(|| {
            self.reap_empty_record_unlocked(lease_id)
        })
    }

    fn reap_empty_record_unlocked(&self, lease_id: &str) -> io::Result<bool> {
        let record = match self.existing_record_dir(lease_id) {
            Ok(record) => record,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };
        let generations = record.join("generations");
        let metadata = fs::symlink_metadata(&generations)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(invalid("executable lease generations are not a real directory"));
        }
        if fs::read_dir(&generations)?.next().is_some() {
            return Ok(false);
        }
        remove_owned_tree(&record)?;
        Ok(true)
    }

    /// Reclaim only leases whose owner and container locks are both idle.
    /// Unknown or lock-damaged state stays untouched. The container sweep is
    /// here, beside receipt recovery, so no caller can apply a weaker liveness
    /// rule to the same executable snapshot.
    pub(crate) fn recover_stale_leases(&self) -> io::Result<usize> {
        self.with_lease_state_lock(|| self.recover_stale_leases_unlocked())
    }

    fn recover_stale_leases_unlocked(&self) -> io::Result<usize> {
        let records = self.root.join(SERVICE_DIR).join(RECORDS_DIR);
        let mut protected_snapshots = BTreeSet::new();
        let mut preserve_containers = false;
        if let Ok(metadata) = fs::symlink_metadata(&records) {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(invalid("executable lease records are not a real directory"));
            }
            // Collect every live snapshot before deleting any idle record. A
            // rollback or interrupted upgrade may temporarily have two
            // receipts naming the same snapshot; an in-order sweep must not
            // let the first idle receipt delete bytes still held by the other.
            for entry in fs::read_dir(&records)? {
                let entry = entry?;
                let name = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| invalid("executable lease id is not UTF-8"))?;
                if !is_hex_id(&name) {
                    continue;
                }
                let record_dir = entry.path();
                let metadata = fs::symlink_metadata(&record_dir)?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    continue;
                }
                let owner_state = lock_state(&record_dir.join(".locks").join("owner.lock"))?;
                if owner_state == LockState::Held {
                    // A held owner lock means the record may still be in the
                    // middle of publication. Without a complete receipt there
                    // is no safe way to name the snapshot, so leave every
                    // container untouched for this recovery pass.
                    preserve_containers = true;
                }
                let generations = record_dir.join("generations");
                let entries = match fs::read_dir(&generations) {
                    Ok(entries) => entries,
                    Err(error) if error.kind() == ErrorKind::NotFound => continue,
                    Err(_) => {
                        preserve_containers = true;
                        continue;
                    }
                };
                let mut unknown_state = false;
                for generation in entries {
                    let generation = generation?;
                    let generation_name = generation.file_name();
                    let Some(number) = generation_name
                        .to_str()
                        .and_then(|value| value.parse::<u64>().ok())
                    else {
                        if !generation_name.to_str().is_some_and(|value| {
                            value.starts_with('.') && value.ends_with(".partial")
                        }) {
                            unknown_state = true;
                        }
                        continue;
                    };
                    let Ok(receipt) = self.read_generation(&name, number) else {
                        let complete = complete_generation_artifacts(&generation.path());
                        if !matches!(complete, Ok(false))
                            || self.unverified_snapshot_path(&generation.path()).is_some()
                        {
                            unknown_state = true;
                        }
                        continue;
                    };
                    let snapshot = self.snapshot_path(&receipt.snapshot_rel)?;
                    if owner_state == LockState::Held || !self.receipt_is_idle(&receipt, &snapshot)? {
                        protected_snapshots.insert(snapshot);
                    }
                }
                if unknown_state {
                    preserve_containers = true;
                }
            }
            for entry in fs::read_dir(&records)? {
                let entry = entry?;
                let name = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| invalid("executable lease id is not UTF-8"))?;
                if !is_hex_id(&name) {
                    continue;
                }
                let record_dir = entry.path();
                let metadata = fs::symlink_metadata(&record_dir)?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    continue;
                }
                let Some(owner_lock) = super::try_acquire_lock(&record_dir, OWNER_LOCK_SCOPE)?
                else {
                    // The recovery claim lost a race with the owner. A probe
                    // is not enough: never delete state after it reports idle.
                    preserve_containers = true;
                    continue;
                };
                let generations = record_dir.join("generations");
                let entries = match fs::read_dir(&generations) {
                    Ok(entries) => entries,
                    Err(error) if error.kind() == ErrorKind::NotFound => {
                        drop(owner_lock);
                        continue;
                    }
                    Err(_) => {
                        preserve_containers = true;
                        drop(owner_lock);
                        continue;
                    }
                };
                let mut record_reclaimable = true;
                for generation in entries {
                    let generation = generation?;
                    let generation_name = generation.file_name();
                    let generation_path = generation.path();
                    let Some(number) = generation_name
                        .to_str()
                        .and_then(|value| value.parse::<u64>().ok())
                    else {
                        if generation_name.to_str().is_some_and(|value| {
                            value.starts_with('.') && value.ends_with(".partial")
                        }) {
                            if remove_owned_tree(&generation_path).is_err() {
                                record_reclaimable = false;
                                preserve_containers = true;
                            }
                        } else {
                            record_reclaimable = false;
                            preserve_containers = true;
                        }
                        continue;
                    };
                    let Ok(receipt) = self.read_generation(&name, number) else {
                        // Numeric generations are published by rename only
                        // after both artifacts are durable. A malformed
                        // complete-looking generation is unknown state: keep
                        // it and all containers. A plainly partial directory
                        // with no receipt is safe to discard after the owner
                        // claim. A parseable receipt may name the last good
                        // snapshot even when antivirus has hidden one artifact.
                        let complete = complete_generation_artifacts(&generation_path);
                        let claimed_snapshot = self.unverified_snapshot_path(&generation_path);
                        if matches!(complete, Ok(false)) && claimed_snapshot.is_none() {
                            if remove_owned_tree(&generation_path).is_err() {
                                record_reclaimable = false;
                            }
                        } else {
                            record_reclaimable = false;
                            preserve_containers = true;
                            if let Some(snapshot) = claimed_snapshot {
                                protected_snapshots.insert(snapshot);
                            }
                        }
                        continue;
                    };
                    let snapshot = self.snapshot_path(&receipt.snapshot_rel)?;
                    if protected_snapshots.contains(&snapshot) {
                        record_reclaimable = false;
                        continue;
                    }
                    let container = self.lease_container(&snapshot)?;
                    let Some(live_lock) = super::try_acquire_lock(&container, "live")? else {
                        protected_snapshots.insert(snapshot);
                        record_reclaimable = false;
                        continue;
                    };
                    // Remove the receipt first. If antivirus or permissions
                    // interrupt the removal, keep the executable snapshot
                    // referenced by the last good receipt.
                    if remove_owned_tree(&generation_path).is_err() {
                        protected_snapshots.insert(snapshot);
                        record_reclaimable = false;
                    }
                    drop(live_lock);
                }
                drop(owner_lock);
                if record_reclaimable && fs::read_dir(&generations)?.next().is_none() {
                    let _ = remove_owned_tree(&record_dir);
                }
            }
        } else if let Err(error) = fs::symlink_metadata(&records) {
            if error.kind() != ErrorKind::NotFound {
                return Err(error);
            }
        }

        let leases = self.root.join("leases");
        let metadata = match fs::symlink_metadata(&leases) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error),
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(invalid("executable lease containers are not a real directory"));
        }
        let mut swept = 0;
        for entry in fs::read_dir(&leases)? {
            let entry = entry?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| invalid("executable lease container name is not UTF-8"))?;
            let path = entry.path();
            if name.starts_with(".reclaiming-") {
                // Quarantines are already outside the live namespace. A
                // prior cleanup may have been interrupted by antivirus or a
                // crash; retry deletion without treating the private name as
                // a caller-visible lease identity.
                let _ = remove_owned_tree(&path);
                continue;
            }
            if name.len() > MAX_CONTAINER_NAME || !valid_container_name(&name) {
                return Err(invalid("executable lease container identity is invalid"));
            }
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(invalid("executable lease container is not a real directory"));
            }
            let snapshot = path.join("snapshot");
            if preserve_containers || protected_snapshots.contains(&snapshot) {
                continue;
            }
            let live_lock = match fs::symlink_metadata(path.join(".locks")) {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                    return Err(invalid("executable lease lock directory is not real"));
                }
                Err(error) if error.kind() != ErrorKind::NotFound => return Err(error),
                // A missing lock directory is not proof of idleness: an
                // antivirus can remove the marker while the process still
                // holds the canonical container inode. The claim helper
                // recreates only the marker and still tests that inode.
                Ok(_) | Err(_) => match super::try_acquire_lock(&path, "live")? {
                    Some(lock) => Some(lock),
                    None => continue,
                },
            };
            if reclaim_container(&path, &leases, live_lock) {
                swept += 1;
            }
        }
        Ok(swept)
    }

    fn receipt_is_idle(&self, receipt: &LeaseReceipt, snapshot: &Path) -> io::Result<bool> {
        if !matches!(
            lock_state(&self.owner_lock_path(&receipt.lease_id)?)?,
            LockState::Idle
        ) {
            return Ok(false);
        }
        let container = self.lease_container(snapshot)?;
        Ok(matches!(
            lock_state(&container.join(".locks").join("live.lock"))?,
            LockState::Idle
        ))
    }

    fn unverified_snapshot_path(&self, generation: &Path) -> Option<PathBuf> {
        let receipt_text = read_bounded(&generation.join(RECEIPT_FILE)).ok()?;
        let receipt = parse_receipt(&receipt_text).ok()?;
        self.snapshot_path(&receipt.snapshot_rel).ok()
    }

    fn lease_container(&self, snapshot: &Path) -> io::Result<PathBuf> {
        let leases = self.root.join("leases");
        let relative = snapshot
            .strip_prefix(&leases)
            .map_err(|_| invalid("executable lease snapshot is outside the lease root"))?;
        let mut components = relative.components();
        let Some(Component::Normal(name)) = components.next() else {
            return Err(invalid("executable lease snapshot has no container"));
        };
        let Some(Component::Normal(snapshot_name)) = components.next() else {
            return Err(invalid("executable lease snapshot is not a container snapshot"));
        };
        if snapshot_name != OsStr::new("snapshot") || components.next().is_some() {
            return Err(invalid("executable lease snapshot is not a container snapshot"));
        }
        Ok(leases.join(Path::new(name)))
    }

    fn request(
        &self,
        operation: Operation,
        lease_id: &str,
        generation: u64,
        owner_scope_text: &str,
        snapshot_root: &Path,
        package: &str,
        version: &str,
        reference: &str,
        output_digest: &str,
        previous_output_digest: &str,
        members: &[LeaseMember],
    ) -> io::Result<LeaseRequest> {
        validate_id(lease_id, "lease id")?;
        validate_scope(owner_scope_text)?;
        if owner_scope(lease_id)? != owner_scope_text {
            return Err(invalid("executable lease owner scope does not match the lease"));
        }
        validate_digest(output_digest, "output digest")?;
        if !previous_output_digest.is_empty() {
            validate_digest(previous_output_digest, "previous output digest")?;
        }
        validate_members(members)?;
        let request = LeaseRequest {
            operation,
            key_id: self.key.key_id.clone(),
            request_id: random_id(16)?,
            lease_id: lease_id.to_string(),
            generation,
            owner_pid: std::process::id(),
            owner_scope: owner_scope_text.to_string(),
            package: bounded_text(package, "package")?,
            version: bounded_text(version, "version")?,
            reference: bounded_text(reference, "reference")?,
            output_digest: output_digest.to_string(),
            previous_output_digest: previous_output_digest.to_string(),
            snapshot_rel: self.relative_snapshot(snapshot_root)?,
            members: members.to_vec(),
            nonce: random_id(32)?,
            mac: String::new(),
        };
        Ok(request)
    }

    pub(crate) fn encode_request(&self, request: &LeaseRequest) -> io::Result<Vec<u8>> {
        if request.key_id != self.key.key_id {
            return Err(invalid("executable lease request key does not match the service"));
        }
        if owner_scope(&request.lease_id)? != request.owner_scope {
            return Err(invalid("executable lease owner scope does not match the lease"));
        }
        let canonical = request_canonical(request, false)?;
        let mac = self.key.sign(&canonical).sig_hex;
        let mut signed = request.clone();
        signed.mac = mac;
        request_canonical(&signed, true)
    }

    fn decode_request(&self, frame: &[u8]) -> io::Result<LeaseRequest> {
        if frame.len() > MAX_FRAME_BYTES {
            return Err(invalid("executable lease request exceeds the frame bound"));
        }
        let text = std::str::from_utf8(frame)
            .map_err(|_| invalid("executable lease request is not UTF-8"))?;
        let mut fields = BTreeMap::new();
        let mut lines = text.split('\n');
        if lines.next() != Some(PROTOCOL_HEADER) {
            return Err(invalid("executable lease request header mismatch"));
        }
        for line in lines {
            if line.is_empty() {
                continue;
            }
            let (name, value) = line
                .split_once('=')
                .ok_or_else(|| invalid("executable lease request field is malformed"))?;
            if fields.insert(name.to_string(), value.to_string()).is_some() {
                return Err(invalid("executable lease request repeats a field"));
            }
        }
        let allowed = [
            "op",
            "key",
            "request",
            "lease",
            "generation",
            "pid",
            "owner",
            "package",
            "version",
            "reference",
            "output",
            "previous",
            "snapshot",
            "members",
            "nonce",
            "mac",
        ];
        if fields.keys().any(|field| !allowed.contains(&field.as_str())) {
            return Err(invalid("executable lease request contains an unknown field"));
        }
        let get = |name: &str| {
            fields
                .get(name)
                .ok_or_else(|| invalid("executable lease request is missing a field"))
        };
        let request = LeaseRequest {
            operation: Operation::parse(get("op")?)?,
            key_id: get("key")?.to_string(),
            request_id: get("request")?.to_string(),
            lease_id: get("lease")?.to_string(),
            generation: get("generation")?
                .parse()
                .map_err(|_| invalid("executable lease generation is not numeric"))?,
            owner_pid: get("pid")?
                .parse()
                .map_err(|_| invalid("executable lease owner id is not numeric"))?,
            owner_scope: decode_text(get("owner")?, "owner scope")?,
            package: decode_text(get("package")?, "package")?,
            version: decode_text(get("version")?, "version")?,
            reference: decode_text(get("reference")?, "reference")?,
            output_digest: decode_text(get("output")?, "output digest")?,
            previous_output_digest: decode_optional_text(
                get("previous")?,
                "previous output digest",
            )?,
            snapshot_rel: decode_text(get("snapshot")?, "snapshot path")?,
            members: decode_members(get("members")?)?,
            nonce: get("nonce")?.to_string(),
            mac: get("mac")?.to_string(),
        };
        validate_request(&request)?;
        if request.key_id != self.key.key_id {
            return Err(invalid("executable lease request key does not match the service"));
        }
        let canonical = request_canonical(&request, false)?;
        let expected = self.key.sign(&canonical).sig_hex;
        if !constant_time_equal(expected.as_bytes(), request.mac.as_bytes()) {
            return Err(invalid("executable lease request authentication failed"));
        }
        Ok(request)
    }

    fn publish_receipt(&self, mut receipt: LeaseReceipt) -> io::Result<LeaseReceipt> {
        let canonical = receipt_canonical(&receipt, false)?;
        receipt.mac = self.key.sign(canonical.as_bytes()).sig_hex;
        let record = self.record_dir(&receipt.lease_id)?;
        let generations = record.join("generations");
        ensure_real_directory(&generations, "executable lease generations")?;
        let generation_dir = generations.join(receipt.generation.to_string());
        let partial = generations.join(format!(
            ".{}-{}.partial",
            receipt.generation,
            PUBLICATION_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let result: io::Result<()> = (|| {
            fs::create_dir(&partial)?;
            write_new_synced(
                &partial.join(RECEIPT_FILE),
                receipt_canonical(&receipt, true)?.as_bytes(),
            )?;
            let witness = receipt_witness(&receipt)?;
            write_new_synced(
                &partial.join(COMPLETE_FILE),
                format!("{witness}\n").as_bytes(),
            )?;
            sync_directory(&partial)?;
            // A generation is visible only after both receipt and completion
            // witness are durable. Rename cannot overwrite an older complete
            // generation, so a concurrent publisher leaves the last good one.
            fs::rename(&partial, &generation_dir)?;
            sync_directory(&generations)?;
            sync_directory(&record)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = remove_owned_tree(&partial);
        }
        result?;
        Ok(receipt)
    }

    fn current_receipt(&self, lease_id: &str) -> io::Result<Option<LeaseReceipt>> {
        let record = self.existing_record_dir(lease_id)?;
        let generations = record.join("generations");
        let metadata = match fs::symlink_metadata(&generations) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(invalid("executable lease generations are not a real directory"));
        }
        let mut latest = None;
        for entry in fs::read_dir(&generations)? {
            let entry = entry?;
            let Some(generation) = entry
                .file_name()
                .to_str()
                .and_then(|value| value.parse::<u64>().ok())
            else {
                continue;
            };
            let generation_path = generations.join(generation.to_string());
            if !complete_generation_artifacts(&generation_path)? {
                continue;
            }
            let receipt = self.read_generation(lease_id, generation)?;
            if latest
                .as_ref()
                .is_none_or(|(current, _)| generation > *current)
            {
                latest = Some((generation, receipt));
            }
        }
        Ok(latest.map(|(_, receipt)| receipt))
    }

    fn rollback_target_matches(
        &self,
        request: &LeaseRequest,
        current_generation: u64,
    ) -> io::Result<bool> {
        let generations = self
            .record_dir(&request.lease_id)?
            .join("generations");
        let metadata = match fs::symlink_metadata(&generations) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(invalid("executable lease generations are not a real directory"));
        }
        for entry in fs::read_dir(generations)? {
            let entry = entry?;
            let Some(generation) = entry
                .file_name()
                .to_str()
                .and_then(|value| value.parse::<u64>().ok())
            else {
                continue;
            };
            if generation >= current_generation {
                continue;
            }
            let Ok(target) = self.read_generation(&request.lease_id, generation) else {
                continue;
            };
            if target.owner_scope == request.owner_scope
                && target.package == request.package
                && target.version == request.version
                && target.reference == request.reference
                && target.output_digest == request.output_digest
                && target.snapshot_rel == request.snapshot_rel
                && target.members == request.members
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn read_generation(&self, lease_id: &str, generation: u64) -> io::Result<LeaseReceipt> {
        validate_id(lease_id, "lease id")?;
        if generation == 0 {
            return Err(invalid("executable lease generation is zero"));
        }
        let path = self
            .existing_record_dir(lease_id)?
            .join("generations");
        let generations_metadata = fs::symlink_metadata(&path)?;
        if generations_metadata.file_type().is_symlink() || !generations_metadata.is_dir() {
            return Err(invalid("executable lease generations are not a real directory"));
        }
        let path = path.join(generation.to_string());
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(invalid("executable lease generation is not a real directory"));
        }
        let receipt_text = read_bounded(&path.join(RECEIPT_FILE))?;
        let receipt = parse_receipt(&receipt_text)?;
        if receipt.lease_id != lease_id || receipt.generation != generation {
            return Err(invalid("executable lease receipt identity mismatch"));
        }
        if owner_scope(lease_id)? != receipt.owner_scope {
            return Err(invalid("executable lease receipt owner scope mismatch"));
        }
        let complete = read_bounded(&path.join(COMPLETE_FILE))?;
        if complete.trim() != receipt_witness(&receipt)? {
            return Err(invalid("executable lease completion witness mismatch"));
        }
        let canonical = receipt_canonical(&receipt, false)?;
        let expected = self.key.sign(canonical.as_bytes()).sig_hex;
        if !constant_time_equal(expected.as_bytes(), receipt.mac.as_bytes()) {
            return Err(invalid("executable lease receipt authentication failed"));
        }
        Ok(receipt)
    }

    fn record_dir(&self, lease_id: &str) -> io::Result<PathBuf> {
        validate_id(lease_id, "lease id")?;
        let records = self.root.join(SERVICE_DIR).join(RECORDS_DIR);
        ensure_real_directory(&records, "executable lease records")?;
        let record = records.join(lease_id);
        match fs::symlink_metadata(&record) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                Err(invalid("executable lease record is not a real directory"))
            }
            Ok(_) => Ok(record),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                match fs::create_dir(&record) {
                    Ok(()) => {
                        sync_directory(&records)?;
                        Ok(record)
                    }
                    Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                        self.existing_record_dir(lease_id)
                    }
                    Err(error) => Err(error),
                }
            }
            Err(error) => Err(error),
        }
    }

    fn existing_record_dir(&self, lease_id: &str) -> io::Result<PathBuf> {
        validate_id(lease_id, "lease id")?;
        let records = self.root.join(SERVICE_DIR).join(RECORDS_DIR);
        ensure_real_directory(&records, "executable lease records")?;
        let record = records.join(lease_id);
        match fs::symlink_metadata(&record) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                Err(invalid("executable lease record is not a real directory"))
            }
            Ok(_) => Ok(record),
            Err(error) => Err(error),
        }
    }

    fn snapshot_path(&self, relative: &str) -> io::Result<PathBuf> {
        validate_relative_path(relative)?;
        let path = self.root.join(relative);
        let leases = self.root.join("leases");
        if !path.starts_with(&leases) {
            return Err(invalid("executable lease snapshot is outside the lease root"));
        }
        Ok(path)
    }

    fn relative_snapshot(&self, path: &Path) -> io::Result<String> {
        let leases = self.root.join("leases");
        let relative = path
            .strip_prefix(&self.root)
            .map_err(|_| invalid("executable lease snapshot is outside the managed root"))?;
        if !path.starts_with(&leases) {
            return Err(invalid("executable lease snapshot is outside the lease root"));
        }
        let text = relative
            .to_str()
            .ok_or_else(|| invalid("executable lease snapshot path is not UTF-8"))?;
        validate_relative_path(text)?;
        Ok(text.to_string())
    }

    fn validate_owner(&self, request: &LeaseRequest) -> io::Result<()> {
        self.validate_owner_fields(
            &request.lease_id,
            request.owner_pid,
            &request.owner_scope,
        )
    }

    fn validate_owner_fields(&self, lease_id: &str, _pid: u32, scope: &str) -> io::Result<()> {
        validate_scope(scope)?;
        if owner_scope(lease_id)? != scope {
            return Err(invalid("executable lease owner scope does not match the lease"));
        }
        if !self.owner_is_live(lease_id, scope) {
            return Err(invalid("executable lease owner lock is not held"));
        }
        Ok(())
    }

    fn owner_is_live(&self, lease_id: &str, _scope: &str) -> bool {
        self.owner_lock_path(lease_id)
            .ok()
            .and_then(|path| lock_state(&path).ok())
            .is_some_and(|state| state == LockState::Held)
    }
}

impl LeaseRequest {
    fn encode_fields(&self, include_mac: bool) -> io::Result<String> {
        let mut out = String::new();
        out.push_str(PROTOCOL_HEADER);
        out.push('\n');
        let fields = [
            ("op", self.operation.as_str().to_string()),
            ("key", self.key_id.clone()),
            ("request", self.request_id.clone()),
            ("lease", self.lease_id.clone()),
            ("generation", self.generation.to_string()),
            ("pid", self.owner_pid.to_string()),
            ("owner", encode_text(&self.owner_scope)),
            ("package", encode_text(&self.package)),
            ("version", encode_text(&self.version)),
            ("reference", encode_text(&self.reference)),
            ("output", encode_text(&self.output_digest)),
            ("previous", encode_text(&self.previous_output_digest)),
            ("snapshot", encode_text(&self.snapshot_rel)),
            ("members", encode_members(&self.members)?),
            ("nonce", self.nonce.clone()),
        ];
        for (name, value) in fields {
            out.push_str(name);
            out.push('=');
            out.push_str(&value);
            out.push('\n');
        }
        if include_mac {
            out.push_str("mac=");
            out.push_str(&self.mac);
            out.push('\n');
        }
        if out.len() > MAX_FRAME_BYTES {
            return Err(invalid("executable lease request exceeds the frame bound"));
        }
        Ok(out)
    }
}

fn request_canonical(request: &LeaseRequest, include_mac: bool) -> io::Result<Vec<u8>> {
    Ok(request.encode_fields(include_mac)?.into_bytes())
}

fn receipt_canonical(receipt: &LeaseReceipt, include_mac: bool) -> io::Result<String> {
    let mut out = String::new();
    out.push_str(PROTOCOL_HEADER);
    out.push_str(" receipt\n");
    let fields = [
        ("lease", receipt.lease_id.clone()),
        ("generation", receipt.generation.to_string()),
        ("pid", receipt.owner_pid.to_string()),
        ("owner", encode_text(&receipt.owner_scope)),
        ("package", encode_text(&receipt.package)),
        ("version", encode_text(&receipt.version)),
        ("reference", encode_text(&receipt.reference)),
        ("output", encode_text(&receipt.output_digest)),
        ("snapshot", encode_text(&receipt.snapshot_rel)),
        ("members", encode_members(&receipt.members)?),
    ];
    for (name, value) in fields {
        out.push_str(name);
        out.push('=');
        out.push_str(&value);
        out.push('\n');
    }
    if include_mac {
        out.push_str("mac=");
        out.push_str(&receipt.mac);
        out.push('\n');
    }
    Ok(out)
}

fn parse_receipt(text: &str) -> io::Result<LeaseReceipt> {
    let mut lines = text.split('\n');
    if lines.next() != Some("JET-EXECUTABLE-LEASE/1 receipt") {
        return Err(invalid("executable lease receipt header mismatch"));
    }
    let mut fields = BTreeMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (name, value) = line
            .split_once('=')
            .ok_or_else(|| invalid("executable lease receipt field is malformed"))?;
        if fields.insert(name.to_string(), value.to_string()).is_some() {
            return Err(invalid("executable lease receipt repeats a field"));
        }
    }
    let allowed = [
        "lease",
        "generation",
        "pid",
        "owner",
        "package",
        "version",
        "reference",
        "output",
        "snapshot",
        "members",
        "mac",
    ];
    if fields.keys().any(|field| !allowed.contains(&field.as_str())) {
        return Err(invalid("executable lease receipt contains an unknown field"));
    }
    let get = |name: &str| {
        fields
            .get(name)
            .ok_or_else(|| invalid("executable lease receipt is missing a field"))
    };
    let receipt = LeaseReceipt {
        lease_id: get("lease")?.to_string(),
        generation: get("generation")?
            .parse()
            .map_err(|_| invalid("executable lease generation is not numeric"))?,
        owner_pid: get("pid")?
            .parse()
            .map_err(|_| invalid("executable lease owner id is not numeric"))?,
        owner_scope: decode_text(get("owner")?, "owner scope")?,
        package: decode_text(get("package")?, "package")?,
        version: decode_text(get("version")?, "version")?,
        reference: decode_text(get("reference")?, "reference")?,
        output_digest: decode_text(get("output")?, "output digest")?,
        snapshot_rel: decode_text(get("snapshot")?, "snapshot path")?,
        members: decode_members(get("members")?)?,
        mac: get("mac")?.to_string(),
    };
    validate_id(&receipt.lease_id, "lease id")?;
    validate_scope(&receipt.owner_scope)?;
    validate_digest(&receipt.output_digest, "output digest")?;
    validate_relative_path(&receipt.snapshot_rel)?;
    validate_members(&receipt.members)?;
    Ok(receipt)
}

fn validate_request(request: &LeaseRequest) -> io::Result<()> {
    validate_id(&request.key_id, "key id")?;
    validate_id(&request.request_id, "request id")?;
    validate_id(&request.lease_id, "lease id")?;
    if request.generation == 0 || request.owner_pid == 0 {
        return Err(invalid("executable lease request has an invalid identity"));
    }
    validate_scope(&request.owner_scope)?;
    validate_digest(&request.output_digest, "output digest")?;
    if !request.previous_output_digest.is_empty() {
        validate_digest(&request.previous_output_digest, "previous output digest")?;
    }
    validate_relative_path(&request.snapshot_rel)?;
    validate_members(&request.members)?;
    if request.nonce.len() != 64 || !is_lower_hex(&request.nonce) {
        return Err(invalid("executable lease nonce is not canonical"));
    }
    if request.mac.len() != 64 || !is_lower_hex(&request.mac) {
        return Err(invalid("executable lease authentication tag is not canonical"));
    }
    for value in [&request.package, &request.version, &request.reference] {
        bounded_text(value, "lease identity")?;
    }
    Ok(())
}

fn validate_members(members: &[LeaseMember]) -> io::Result<()> {
    if members.len() > MAX_MEMBERS {
        return Err(invalid("executable lease member count exceeds the bound"));
    }
    let mut names = BTreeMap::new();
    for member in members {
        if member.name.is_empty()
            || member.name == "."
            || member.name == ".."
            || member.name.contains('/')
            || member.name.contains('\\')
            || names.insert(member.name.clone(), ()).is_some()
        {
            return Err(invalid("executable lease member name is not canonical"));
        }
        validate_digest(&member.digest, "executable member digest")?;
    }
    Ok(())
}

fn validate_digest(value: &str, field: &str) -> io::Result<()> {
    let Some(hex) = value.strip_prefix("sha256-") else {
        return Err(invalid(field));
    };
    if hex.len() != 64 || !is_lower_hex(hex) {
        return Err(invalid(field));
    }
    Ok(())
}

fn validate_id(value: &str, field: &str) -> io::Result<()> {
    if value.is_empty() || value.len() > 128 || !is_lower_hex(value) {
        return Err(invalid(field));
    }
    Ok(())
}

fn is_hex_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && is_lower_hex(value)
}

fn validate_scope(value: &str) -> io::Result<()> {
    if !value.starts_with(OWNER_SCOPE_PREFIX)
        || value.len() > 160
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(invalid("executable lease owner scope is not canonical"));
    }
    Ok(())
}

fn owner_scope(lease_id: &str) -> io::Result<String> {
    validate_id(lease_id, "lease id")?;
    Ok(format!("{OWNER_SCOPE_PREFIX}{lease_id}"))
}

fn valid_container_name(name: &str) -> bool {
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

fn validate_relative_path(value: &str) -> io::Result<()> {
    let path = Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_))
        })
    {
        return Err(invalid("executable lease snapshot path is not relative"));
    }
    Ok(())
}

fn bounded_text(value: &str, field: &str) -> io::Result<String> {
    if value.is_empty() || value.len() > MAX_FIELD_BYTES || value.contains('\n') {
        return Err(invalid(field));
    }
    Ok(value.to_string())
}

fn encode_text(value: &str) -> String {
    value.bytes().map(|byte| format!("{byte:02x}")).collect()
}

fn decode_text(value: &str, field: &str) -> io::Result<String> {
    let bytes = decode_hex(value, field)?;
    let text = String::from_utf8(bytes).map_err(|_| invalid(field))?;
    bounded_text(&text, field)
}

fn decode_optional_text(value: &str, field: &str) -> io::Result<String> {
    let bytes = decode_hex(value, field)?;
    String::from_utf8(bytes).map_err(|_| invalid(field))
}

fn encode_members(members: &[LeaseMember]) -> io::Result<String> {
    validate_members(members)?;
    let mut sorted = members.to_vec();
    sorted.sort_by(|left, right| left.name.cmp(&right.name));
    let text = sorted
        .iter()
        .map(|member| format!("{}\t{}\n", member.name, member.digest))
        .collect::<String>();
    Ok(encode_text(&text))
}

fn decode_members(value: &str) -> io::Result<Vec<LeaseMember>> {
    let text = String::from_utf8(decode_hex(value, "lease members")?)
        .map_err(|_| invalid("lease members"))?;
    let mut members = Vec::new();
    for line in text.split('\n') {
        if line.is_empty() {
            continue;
        }
        let (name, digest) = line
            .split_once('\t')
            .ok_or_else(|| invalid("lease member record is malformed"))?;
        members.push(LeaseMember {
            name: name.to_string(),
            digest: digest.to_string(),
        });
    }
    validate_members(&members)?;
    Ok(members)
}

fn decode_hex(value: &str, field: &str) -> io::Result<Vec<u8>> {
    if value.len() % 2 != 0 || value.len() > MAX_FIELD_BYTES * 2 {
        return Err(invalid(field));
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    let mut chars = value.bytes();
    while let (Some(high), Some(low)) = (chars.next(), chars.next()) {
        let high = hex_digit(high).ok_or_else(|| invalid(field))?;
        let low = hex_digit(low).ok_or_else(|| invalid(field))?;
        bytes.push((high << 4) | low);
    }
    Ok(bytes)
}

fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

fn is_lower_hex(value: &str) -> bool {
    value.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn random_id(bytes: usize) -> io::Result<String> {
    let mut value = vec![0u8; bytes];
    #[cfg(unix)]
    {
        value.copy_from_slice(&os_random_bytes::<32>()?[..bytes]);
    }
    #[cfg(windows)]
    {
        value.copy_from_slice(&os_random_bytes::<32>()?[..bytes]);
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = bytes;
        return Err(io::Error::new(
            ErrorKind::Unsupported,
            "executable lease ids require a platform CSPRNG",
        ));
    }
    Ok(value.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn next_generation(previous: u64) -> io::Result<u64> {
    previous
        .checked_add(1)
        .ok_or_else(|| invalid("executable lease generation overflow"))
}

fn receipt_witness(receipt: &LeaseReceipt) -> io::Result<String> {
    let canonical = receipt_canonical(receipt, true)?;
    Ok(format!(
        "sha256-{}",
        SHA256::sha256_hex(canonical.as_bytes())
    ))
}

fn load_or_create_key(service: &Path) -> io::Result<Vec<u8>> {
    let path = service.join(AUTH_KEY_FILE);
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() != 32 {
                return Err(invalid("executable lease authentication key is not a 32-byte file"));
            }
            return fs::read(path);
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let partial = service.join(format!(".{AUTH_KEY_FILE}.{}-{sequence}.partial", std::process::id()));
    let secret = os_random_bytes::<32>()?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(&secret)?;
    file.sync_all()?;
    // `rename` replaces an existing destination on Unix.  That would let two
    // first openers race and silently rotate the shared authentication key,
    // invalidating every receipt signed by the winner.  A hard link gives us
    // create-new publication on both supported native platforms.
    match fs::hard_link(&partial, &path) {
        Ok(()) => {
            let _ = fs::remove_file(&partial);
            sync_directory(service)?;
        }
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(&partial);
        }
        Err(error) => {
            let _ = fs::remove_file(&partial);
            return Err(error);
        }
    }
    fs::read(path)
}

fn ensure_real_directory(path: &Path, label: &str) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(invalid(label))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            fs::create_dir_all(path)?;
            let metadata = fs::symlink_metadata(path)?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                Err(invalid(label))
            } else {
                Ok(())
            }
        }
        Err(error) => Err(error),
    }
}

fn write_new_synced(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn complete_generation_artifacts(path: &Path) -> io::Result<bool> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(invalid("executable lease generation is not a real directory"));
    }
    Ok(regular_artifact_exists(&path.join(RECEIPT_FILE))?
        && regular_artifact_exists(&path.join(COMPLETE_FILE))?)
}

fn regular_artifact_exists(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(invalid("executable lease receipt artifact is not a regular file"))
        }
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn read_bounded(path: &Path) -> io::Result<String> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(invalid("executable lease receipt artifact is not a regular file"));
    }
    #[cfg(unix)]
    let file = {
        use std::os::unix::fs::OpenOptionsExt as _;
        let mut options = fs::OpenOptions::new();
        options
            .read(true)
            .custom_flags(crate::Envelope::nofollow_open_flag().map_err(io::Error::other)?);
        options.open(path)?
    };
    #[cfg(windows)]
    let file = {
        use std::os::windows::fs::OpenOptionsExt as _;
        let mut options = fs::OpenOptions::new();
        options
            .read(true)
            .custom_flags(0x0020_0000);
        options.open(path)?
    };
    #[cfg(not(any(unix, windows)))]
    let file = fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(invalid("executable lease receipt artifact is not a regular file"));
    }
    if file.metadata()?.len() > MAX_FIELD_BYTES as u64 * 8 {
        return Err(invalid("executable lease receipt exceeds the bound"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_FIELD_BYTES as u64 * 8 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_FIELD_BYTES * 8 {
        return Err(invalid("executable lease receipt exceeds the bound"));
    }
    String::from_utf8(bytes).map_err(|_| invalid("executable lease receipt is not UTF-8"))
}

fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        return fs::File::open(path)?.sync_all();
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        let file = fs::OpenOptions::new()
            .read(true)
            .share_mode(0x0000_0001 | 0x0000_0002 | 0x0000_0004)
            .custom_flags(0x0200_0000 | 0x0020_0000)
            .open(path)?;
        return file.sync_all();
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Ok(())
    }
}

fn reclaim_container(
    path: &Path,
    leases: &Path,
    live_lock: Option<super::FileLock>,
) -> bool {
    let sequence = RECLAIM_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let quarantine = leases.join(format!(
        ".reclaiming-{}-{sequence}",
        std::process::id()
    ));
    // Windows denies directory replacement while the lock marker is open.
    // The outer Hangar lock prevents a production producer from entering this
    // gap; Unix keeps the kernel claim through the namespace move.
    #[cfg(windows)]
    let live_lock = {
        drop(live_lock);
        None::<super::FileLock>
    };
    #[cfg(not(windows))]
    let live_lock = live_lock;
    if fs::rename(path, &quarantine).is_err() {
        return false;
    }
    // Publish the quarantine name before deleting anything. An interrupted
    // delete leaves the complete old tree recoverable and outside the live
    // lease namespace.
    if sync_directory(leases).is_err() {
        drop(live_lock);
        return true;
    }
    drop(live_lock);
    let _ = remove_owned_tree(&quarantine);
    let _ = sync_directory(leases);
    true
}

fn remove_owned_tree(path: &Path) -> io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || metadata.is_file() {
        fs::remove_file(path)
    } else if metadata.is_dir() {
        let mut permissions = metadata.permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            permissions.set_mode(0o700);
        }
        #[cfg(not(unix))]
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions)?;
        for entry in fs::read_dir(path)? {
            remove_owned_tree(&entry?.path())?;
        }
        fs::remove_dir(path)
    } else {
        Err(invalid("executable lease state contains a special file"))
    }
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (left, right) in left.iter().zip(right) {
        difference |= left ^ right;
    }
    difference == 0
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[cfg(unix)]
    use std::process::Command;

    fn scratch(tag: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "jet-exec-lease-{tag}-{}",
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn snapshot(root: &Path, name: &str, bytes: &str) -> (PathBuf, String) {
        let path = root.join("leases").join(name).join("snapshot");
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("program"), bytes).unwrap();
        let digest = crate::Envelope::try_output_hash_of(&path.to_string_lossy()).unwrap();
        (path, digest)
    }

    fn member(bytes: &str) -> LeaseMember {
        LeaseMember {
            name: "program".into(),
            digest: format!("sha256-{}", SHA256::sha256_hex(bytes.as_bytes())),
        }
    }

    #[test]
    fn authenticated_frame_rejects_tampering_and_records_identity() {
        let root = scratch("auth");
        let protocol = ExecutableLeaseProtocol::open(&root).unwrap();
        let lease_id = random_id(16).unwrap();
        let scope = owner_scope(&lease_id).unwrap();
        let snapshot = root.join("leases").join("auth");
        fs::create_dir_all(&snapshot).unwrap();
        fs::write(snapshot.join("program"), "one").unwrap();
        let digest = crate::Envelope::try_output_hash_of(&snapshot.to_string_lossy()).unwrap();
        let (request, owner) = protocol
            .prepare_snapshot(
                &lease_id,
                &scope,
                &snapshot,
                "demo",
                "1",
                "demo@core#1",
                &digest,
                &[member("one")],
            )
            .unwrap();
        let frame = protocol.encode_request(&request).unwrap();
        let receipt = protocol
            .accept_snapshot(&frame, &digest, &snapshot)
            .unwrap();
        protocol
            .validate_snapshot(
                &receipt.lease_id,
                receipt.generation,
                &receipt.owner_scope,
                &snapshot,
                &digest,
            )
            .unwrap();
        let mut tampered = frame;
        let index = tampered.iter().position(|byte| *byte == b'1').unwrap();
        tampered[index] = b'2';
        assert!(protocol
            .accept_snapshot(&tampered, &digest, &snapshot)
            .is_err());
        drop(owner);
        drop(protocol);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn owner_lock_is_lease_scoped_and_frame_identity_is_bound() {
        let root = scratch("owner-scope");
        let protocol = ExecutableLeaseProtocol::open(&root).unwrap();
        let lease_id = random_id(16).unwrap();
        let scope = owner_scope(&lease_id).unwrap();
        let (snapshot, digest) = snapshot(&root, "1-0-owner-scope", "one");
        let global = crate::RuntimePolicy::acquire_lock(&root, "hangar").unwrap();

        let (request, owner) = protocol
            .prepare_snapshot(
                &lease_id,
                &scope,
                &snapshot,
                "demo",
                "1",
                "demo@core#1",
                &digest,
                &[member("one")],
            )
            .unwrap();
        let mut wrong_scope = request.clone();
        wrong_scope.owner_scope = owner_scope(&random_id(16).unwrap()).unwrap();
        assert!(protocol.encode_request(&wrong_scope).is_err());

        let mut wrong_key = request;
        wrong_key.key_id = "0000000000000000".into();
        assert!(protocol.encode_request(&wrong_key).is_err());

        drop(owner);
        drop(global);
        drop(protocol);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn incomplete_generation_is_not_published_and_stale_owner_is_reclaimed() {
        let root = scratch("recovery");
        let protocol = ExecutableLeaseProtocol::open(&root).unwrap();
        let lease_id = random_id(16).unwrap();
        let owner_scope = owner_scope(&lease_id).unwrap();
        let (snapshot, digest) = snapshot(&root, "1-0-recovery", "one");
        let live = crate::RuntimePolicy::acquire_lease_lock(
            snapshot.parent().unwrap(),
            "live",
        )
        .unwrap();
        let (request, owner) = protocol
            .prepare_snapshot(
                &lease_id,
                &owner_scope,
                &snapshot,
                "demo",
                "1",
                "demo@core#1",
                &digest,
                &[member("one")],
            )
            .unwrap();
        let frame = protocol.encode_request(&request).unwrap();
        let receipt = protocol
            .accept_snapshot(&frame, &digest, &snapshot)
            .unwrap();
        let incomplete = root
            .join(SERVICE_DIR)
            .join(RECORDS_DIR)
            .join(&lease_id)
            .join("generations")
            .join("99");
        fs::create_dir_all(&incomplete).unwrap();
        fs::write(incomplete.join(RECEIPT_FILE), "partial").unwrap();
        assert_eq!(protocol.recover_stale_leases().unwrap(), 0);
        assert!(incomplete.exists());
        drop(owner);
        drop(live);
        assert_eq!(protocol.recover_stale_leases().unwrap(), 1);
        assert!(!snapshot.exists());
        assert!(!incomplete.exists());
        assert!(protocol
            .read_generation(&receipt.lease_id, receipt.generation)
            .is_err());
        drop(protocol);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recovery_does_not_remove_a_partial_generation_while_owner_is_held() {
        let root = scratch("active-partial");
        let protocol = ExecutableLeaseProtocol::open(&root).unwrap();
        let lease_id = random_id(16).unwrap();
        let owner = protocol.acquire_owner(&lease_id).unwrap();
        let partial = root
            .join(SERVICE_DIR)
            .join(RECORDS_DIR)
            .join(&lease_id)
            .join("generations")
            .join(".1-0.partial");
        fs::create_dir_all(&partial).unwrap();
        fs::write(partial.join(RECEIPT_FILE), "in flight").unwrap();

        assert_eq!(protocol.recover_stale_leases().unwrap(), 0);
        assert!(partial.exists());

        drop(owner);
        protocol.recover_stale_leases().unwrap();
        assert!(!partial.exists());
        drop(protocol);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recovery_preserves_a_complete_generation_when_its_witness_is_interfered_with() {
        let root = scratch("witness-interference");
        let protocol = ExecutableLeaseProtocol::open(&root).unwrap();
        let lease_id = random_id(16).unwrap();
        let owner_scope = owner_scope(&lease_id).unwrap();
        let (snapshot, digest) = snapshot(&root, "1-0-witness-interference", "one");
        let (request, owner) = protocol
            .prepare_snapshot(
                &lease_id,
                &owner_scope,
                &snapshot,
                "demo",
                "1",
                "demo@core#1",
                &digest,
                &[member("one")],
            )
            .unwrap();
        let receipt = protocol
            .accept_snapshot(
                &protocol.encode_request(&request).unwrap(),
                &digest,
                &snapshot,
            )
            .unwrap();
        drop(owner);

        let generation = root
            .join(SERVICE_DIR)
            .join(RECORDS_DIR)
            .join(&lease_id)
            .join("generations")
            .join(receipt.generation.to_string());
        fs::write(
            generation.join(COMPLETE_FILE),
            "antivirus-interference",
        )
        .unwrap();

        assert_eq!(protocol.recover_stale_leases().unwrap(), 0);
        assert!(generation.exists());
        assert!(snapshot.exists());
        drop(protocol);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn interrupted_pre_handoff_container_is_quarantined_and_removed() {
        let root = scratch("pre-handoff");
        let protocol = ExecutableLeaseProtocol::open(&root).unwrap();
        let (snapshot, _) = snapshot(&root, "4294967294-1-pre-handoff", "partial");

        assert_eq!(protocol.recover_stale_leases().unwrap(), 1);
        assert!(!snapshot.parent().unwrap().exists());
        assert!(fs::read_dir(root.join("leases")).unwrap().next().is_none());

        drop(protocol);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn missing_lifetime_marker_without_receipt_is_not_live_proof() {
        let root = scratch("marker-without-receipt");
        let protocol = ExecutableLeaseProtocol::open(&root).unwrap();
        let (snapshot, _) = snapshot(&root, "1-0-marker-without-receipt", "one");
        let live_root = snapshot.parent().unwrap();
        let live = crate::RuntimePolicy::acquire_lease_lock(live_root, "live").unwrap();
        fs::remove_file(live_root.join(".locks/live.lock")).unwrap();

        assert_eq!(protocol.recover_stale_leases().unwrap(), 0);
        assert!(snapshot.exists());

        drop(live);
        fs::write(live_root.join(".locks/live.lock"), b"").unwrap();
        assert_eq!(protocol.recover_stale_leases().unwrap(), 1);
        assert!(!snapshot.exists());
        drop(protocol);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn missing_lifetime_lock_directory_never_proves_a_live_snapshot_idle() {
        let root = scratch("lock-directory-interference");
        let protocol = ExecutableLeaseProtocol::open(&root).unwrap();
        let (snapshot, _) = snapshot(&root, "1-0-lock-directory-interference", "one");
        let live_root = snapshot.parent().unwrap();
        let live = crate::RuntimePolicy::acquire_lease_lock(live_root, "live").unwrap();
        fs::remove_dir_all(live_root.join(".locks")).unwrap();

        assert_eq!(protocol.recover_stale_leases().unwrap(), 0);
        assert!(snapshot.exists());

        drop(live);
        assert_eq!(protocol.recover_stale_leases().unwrap(), 1);
        assert!(!snapshot.exists());
        drop(protocol);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recovery_waits_for_container_lifetime_lock() {
        let root = scratch("container-lock");
        let protocol = ExecutableLeaseProtocol::open(&root).unwrap();
        let lease_id = random_id(16).unwrap();
        let owner_scope = owner_scope(&lease_id).unwrap();
        let (snapshot, digest) = snapshot(&root, "1-0-container-lock", "one");
        let live = crate::RuntimePolicy::acquire_lease_lock(
            snapshot.parent().unwrap(),
            "live",
        )
        .unwrap();
        let (request, owner) = protocol
            .prepare_snapshot(
                &lease_id,
                &owner_scope,
                &snapshot,
                "demo",
                "1",
                "demo@core#1",
                &digest,
                &[member("one")],
            )
            .unwrap();
        protocol
            .accept_snapshot(
                &protocol.encode_request(&request).unwrap(),
                &digest,
                &snapshot,
            )
            .unwrap();
        drop(owner);
        assert_eq!(protocol.recover_stale_leases().unwrap(), 0);
        assert!(snapshot.exists());
        drop(live);
        assert_eq!(protocol.recover_stale_leases().unwrap(), 1);
        assert!(!snapshot.exists());
        drop(protocol);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn missing_lifetime_marker_never_proves_a_live_snapshot_idle() {
        let root = scratch("marker-interference");
        let protocol = ExecutableLeaseProtocol::open(&root).unwrap();
        let lease_id = random_id(16).unwrap();
        let owner_scope = owner_scope(&lease_id).unwrap();
        let (snapshot, digest) = snapshot(&root, "1-0-marker-interference", "one");
        let live_root = snapshot.parent().unwrap();
        let live = crate::RuntimePolicy::acquire_lease_lock(live_root, "live").unwrap();
        let (request, owner) = protocol
            .prepare_snapshot(
                &lease_id,
                &owner_scope,
                &snapshot,
                "demo",
                "1",
                "demo@core#1",
                &digest,
                &[member("one")],
            )
            .unwrap();
        protocol
            .accept_snapshot(
                &protocol.encode_request(&request).unwrap(),
                &digest,
                &snapshot,
            )
            .unwrap();
        drop(owner);

        let marker = live_root.join(".locks/live.lock");
        fs::remove_file(&marker).unwrap();
        assert_eq!(protocol.recover_stale_leases().unwrap(), 0);
        assert!(snapshot.exists());

        drop(live);
        fs::write(&marker, b"").unwrap();
        assert_eq!(protocol.recover_stale_leases().unwrap(), 1);
        assert!(!snapshot.exists());
        drop(protocol);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn replacement_requires_the_current_digest_and_keeps_old_generation() {
        let root = scratch("replace");
        let protocol = ExecutableLeaseProtocol::open(&root).unwrap();
        let lease_id = random_id(16).unwrap();
        let owner_scope = owner_scope(&lease_id).unwrap();
        let (first, first_digest) = snapshot(&root, "1-0-first", "one");
        let (request, owner) = protocol
            .prepare_snapshot(
                &lease_id,
                &owner_scope,
                &first,
                "demo",
                "1",
                "demo@core#1",
                &first_digest,
                &[member("one")],
            )
            .unwrap();
        let first_receipt = protocol
            .accept_snapshot(
                &protocol.encode_request(&request).unwrap(),
                &first_digest,
                &first,
            )
            .unwrap();
        drop(owner);
        drop(protocol);
        let protocol = ExecutableLeaseProtocol::open(&root).unwrap();
        let owner = protocol.acquire_owner(&lease_id).unwrap();
        let (second, second_digest) = snapshot(&root, "1-1-second", "two");
        let frame = protocol
            .prepare_replacement(
                &first_receipt,
                &second,
                &second_digest,
                &[member("two")],
            )
            .unwrap();
        let second_receipt = protocol
            .accept_snapshot(&frame, &second_digest, &second)
            .unwrap();
        assert_eq!(second_receipt.generation, 2);
        assert_eq!(
            protocol.read_generation(&lease_id, first_receipt.generation).unwrap(),
            first_receipt
        );
        protocol
            .release(&lease_id, first_receipt.generation, &owner_scope)
            .unwrap();
        assert_eq!(
            protocol.read_generation(&lease_id, first_receipt.generation).unwrap(),
            first_receipt
        );
        let rollback = protocol
            .prepare_rollback(&second_receipt, first_receipt.generation)
            .unwrap();
        let rollback_receipt = protocol
            .accept_snapshot(&rollback, &first_digest, &first)
            .unwrap();
        assert_eq!(rollback_receipt.generation, 3);
        assert_eq!(rollback_receipt.output_digest, first_receipt.output_digest);
        assert_eq!(rollback_receipt.snapshot_rel, first_receipt.snapshot_rel);
        assert_eq!(
            protocol.current_receipt(&lease_id).unwrap().unwrap(),
            rollback_receipt
        );
        assert_eq!(
            protocol.read_generation(&lease_id, second_receipt.generation).unwrap(),
            second_receipt
        );
        drop(owner);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn replacement_worker() {
        let Some(root) = std::env::var_os("JET_EXEC_LEASE_WORKER_ROOT") else {
            return;
        };
        let root = PathBuf::from(root);
        let lease_id = std::env::var("JET_EXEC_LEASE_WORKER_LEASE").unwrap();
        let snapshot = PathBuf::from(std::env::var_os("JET_EXEC_LEASE_WORKER_SNAPSHOT").unwrap());
        let digest = std::env::var("JET_EXEC_LEASE_WORKER_DIGEST").unwrap();
        let ready = PathBuf::from(std::env::var_os("JET_EXEC_LEASE_WORKER_READY").unwrap());
        let go = PathBuf::from(std::env::var_os("JET_EXEC_LEASE_WORKER_GO").unwrap());
        let prepared =
            PathBuf::from(std::env::var_os("JET_EXEC_LEASE_WORKER_PREPARED").unwrap());
        let publish = PathBuf::from(std::env::var_os("JET_EXEC_LEASE_WORKER_PUBLISH").unwrap());
        let result = PathBuf::from(std::env::var_os("JET_EXEC_LEASE_WORKER_RESULT").unwrap());
        let protocol = ExecutableLeaseProtocol::open(&root).unwrap();
        fs::write(&ready, b"ready").unwrap();
        while !go.exists() {
            std::thread::sleep(Duration::from_millis(1));
        }
        let current = protocol.current_receipt(&lease_id).unwrap().unwrap();
        let member_bytes = fs::read_to_string(snapshot.join("program")).unwrap();
        let frame = protocol
            .prepare_replacement(&current, &snapshot, &digest, &[member(&member_bytes)])
            .unwrap();
        fs::write(&prepared, b"prepared").unwrap();
        while !publish.exists() {
            std::thread::sleep(Duration::from_millis(1));
        }
        match protocol.accept_snapshot(&frame, &digest, &snapshot) {
            Ok(receipt) => fs::write(&result, format!("ok\n{}\n", receipt.generation)).unwrap(),
            Err(error) => fs::write(&result, format!("err\n{error}\n")).unwrap(),
        }
    }

    #[cfg(unix)]
    #[test]
    fn concurrent_replacements_publish_one_serial_generation() {
        let root = scratch("concurrent-replace");
        let protocol = ExecutableLeaseProtocol::open(&root).unwrap();
        let lease_id = random_id(16).unwrap();
        let scope = owner_scope(&lease_id).unwrap();
        let (first, first_digest) = snapshot(&root, "1-0-concurrent-first", "one");
        let (request, owner) = protocol
            .prepare_snapshot(
                &lease_id,
                &scope,
                &first,
                "demo",
                "1",
                "demo@core#1",
                &first_digest,
                &[member("one")],
            )
            .unwrap();
        protocol
            .accept_snapshot(
                &protocol.encode_request(&request).unwrap(),
                &first_digest,
                &first,
            )
            .unwrap();
        let (left, left_digest) = snapshot(&root, "1-1-concurrent-left", "left");
        let (right, right_digest) = snapshot(&root, "1-2-concurrent-right", "right");
        let binary = std::env::current_exe().unwrap();
        let go = root.join("workers.go");
        let publish = root.join("workers.publish");
        let spawn = |tag: &str, snapshot: &Path, digest: &str| {
            Command::new(&binary)
                .arg("replacement_worker")
                .env("JET_EXEC_LEASE_WORKER_ROOT", &root)
                .env("JET_EXEC_LEASE_WORKER_LEASE", &lease_id)
                .env("JET_EXEC_LEASE_WORKER_SNAPSHOT", snapshot)
                .env("JET_EXEC_LEASE_WORKER_DIGEST", digest)
                .env("JET_EXEC_LEASE_WORKER_READY", root.join(format!("{tag}.ready")))
                .env(
                    "JET_EXEC_LEASE_WORKER_PREPARED",
                    root.join(format!("{tag}.prepared")),
                )
                .env("JET_EXEC_LEASE_WORKER_GO", &go)
                .env("JET_EXEC_LEASE_WORKER_PUBLISH", &publish)
                .env("JET_EXEC_LEASE_WORKER_RESULT", root.join(format!("{tag}.result")))
                .spawn()
                .unwrap()
        };
        let mut workers = vec![
            spawn("left", &left, &left_digest),
            spawn("right", &right, &right_digest),
        ];
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while (!root.join("left.ready").exists() || !root.join("right.ready").exists())
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(1));
        }
        if !(root.join("left.ready").is_file() && root.join("right.ready").is_file()) {
            for worker in &mut workers {
                let _ = worker.kill();
                let _ = worker.wait();
            }
            panic!("replacement workers did not reach the barrier");
        }
        fs::write(&go, b"go").unwrap();
        let prepared_deadline = std::time::Instant::now() + Duration::from_secs(5);
        while (!root.join("left.prepared").exists() || !root.join("right.prepared").exists())
            && std::time::Instant::now() < prepared_deadline
        {
            std::thread::sleep(Duration::from_millis(1));
        }
        if !(root.join("left.prepared").is_file() && root.join("right.prepared").is_file()) {
            for worker in &mut workers {
                let _ = worker.kill();
                let _ = worker.wait();
            }
            panic!("replacement workers did not prepare the same base generation");
        }
        fs::write(&publish, b"publish").unwrap();
        for worker in &mut workers {
            assert!(worker.wait().unwrap().success());
        }
        let results = ["left", "right"]
            .map(|tag| fs::read_to_string(root.join(format!("{tag}.result"))).unwrap());
        assert_eq!(
            results.iter().filter(|result| result.starts_with("ok\n")).count(),
            1,
            "replacement results: {results:?}"
        );
        assert_eq!(
            results
                .iter()
                .filter(|result| result.contains("executable lease replacement base is stale"))
                .count(),
            1,
            "replacement results: {results:?}"
        );
        let current = protocol.current_receipt(&lease_id).unwrap().unwrap();
        assert_eq!(current.generation, 2);
        assert_eq!(
            protocol.read_generation(&lease_id, 1).unwrap().output_digest,
            first_digest
        );
        let generations = root
            .join(SERVICE_DIR)
            .join(RECORDS_DIR)
            .join(&lease_id)
            .join("generations");
        assert!(fs::read_dir(generations)
            .unwrap()
            .flatten()
            .all(|entry| !entry.file_name().to_string_lossy().ends_with(".partial")));
        drop(owner);
        protocol.recover_stale_leases().unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}
