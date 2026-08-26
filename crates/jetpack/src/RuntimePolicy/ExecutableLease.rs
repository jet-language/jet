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
use std::collections::BTreeMap;
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
const MAX_FRAME_BYTES: usize = 64 * 1024;
const MAX_FIELD_BYTES: usize = 16 * 1024;
const MAX_MEMBERS: usize = 4096;
const OWNER_SCOPE_PREFIX: &str = "executable-lease-owner-";

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

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

    /// Hold this lock for the lifetime of the process handoff.  Kernel lock
    /// ownership, rather than a PID or timestamp, is the stale-lease fact.
    pub(crate) fn acquire_owner(&self, lease_id: &str) -> io::Result<super::FileLock> {
        let scope = owner_scope(lease_id)?;
        super::acquire_lease_lock(&self.root, &scope)
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

    // Card #963 owns wiring replacement and rollback into the production
    // handoff path; the request builders below are written and tested but
    // have no caller until that card lands.
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

    // Card #963 owns wiring rollback into the production handoff path.
    #[allow(dead_code)]
    pub(crate) fn prepare_rollback(
        &self,
        receipt: &LeaseReceipt,
        target_generation: u64,
    ) -> io::Result<Vec<u8>> {
        if target_generation == 0 || target_generation >= receipt.generation {
            return Err(invalid("rollback target is not an older lease generation"));
        }
        let request = self.request(
            Operation::Rollback,
            &receipt.lease_id,
            target_generation,
            &receipt.owner_scope,
            &self.root.join(&receipt.snapshot_rel),
            &receipt.package,
            &receipt.version,
            &receipt.reference,
            &receipt.output_digest,
            "",
            &receipt.members,
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
        let request = self.decode_request(frame)?;
        if !matches!(request.operation, Operation::Acquire | Operation::Replace) {
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
                if request.generation <= previous.generation
                    || request.previous_output_digest != previous.output_digest
                {
                    return Err(invalid("executable lease replacement base is stale"));
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
        self.validate_owner_fields(receipt.owner_pid, &receipt.owner_scope)?;
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
        let receipt = self.read_generation(lease_id, generation)?;
        if receipt.owner_scope != owner_scope {
            return Err(invalid("executable lease release owner mismatch"));
        }
        self.validate_owner_fields(receipt.owner_pid, owner_scope)?;
        let record = self.record_dir(lease_id)?;
        let generation_dir = record.join("generations").join(generation.to_string());
        remove_owned_tree(&generation_dir)?;
        if fs::read_dir(record.join("generations"))?.next().is_none() {
            remove_owned_tree(&record)?;
        }
        Ok(())
    }

    /// Reclaim only receipts authenticated by this root's key and whose
    /// owner lock is idle. Unknown lease directories stay untouched.
    pub(crate) fn recover_stale_leases(&self) -> io::Result<usize> {
        let records = self.root.join(SERVICE_DIR).join(RECORDS_DIR);
        let metadata = match fs::symlink_metadata(&records) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error),
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(invalid("executable lease records are not a real directory"));
        }
        let mut swept = 0;
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
            let generations = record_dir.join("generations");
            let Ok(entries) = fs::read_dir(&generations) else {
                continue;
            };
            for generation in entries {
                let generation = generation?;
                let Some(number) = generation
                    .file_name()
                    .to_str()
                    .and_then(|value| value.parse::<u64>().ok())
                else {
                    continue;
                };
                let Ok(receipt) = self.read_generation(&name, number) else {
                    continue;
                };
                if self.owner_is_live(receipt.owner_pid, &receipt.owner_scope) {
                    continue;
                }
                let snapshot = self.snapshot_path(&receipt.snapshot_rel)?;
                if remove_owned_tree(&snapshot).is_err() {
                    continue;
                }
                let generation_dir = generations.join(number.to_string());
                remove_owned_tree(&generation_dir)?;
                swept += 1;
            }
            if fs::read_dir(&generations)?.next().is_none() {
                remove_owned_tree(&record_dir)?;
            }
        }
        Ok(swept)
    }

    fn request(
        &self,
        operation: Operation,
        lease_id: &str,
        generation: u64,
        owner_scope: &str,
        snapshot_root: &Path,
        package: &str,
        version: &str,
        reference: &str,
        output_digest: &str,
        previous_output_digest: &str,
        members: &[LeaseMember],
    ) -> io::Result<LeaseRequest> {
        validate_id(lease_id, "lease id")?;
        validate_scope(owner_scope)?;
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
            owner_scope: owner_scope.to_string(),
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
            previous_output_digest: decode_text(get("previous")?, "previous output digest")?,
            snapshot_rel: decode_text(get("snapshot")?, "snapshot path")?,
            members: decode_members(get("members")?)?,
            nonce: get("nonce")?.to_string(),
            mac: get("mac")?.to_string(),
        };
        validate_request(&request)?;
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
        fs::create_dir(&generation_dir)?;
        write_new_synced(
            &generation_dir.join(RECEIPT_FILE),
            receipt_canonical(&receipt, true)?.as_bytes(),
        )?;
        let witness = receipt_witness(&receipt)?;
        write_new_synced(
            &generation_dir.join(COMPLETE_FILE),
            format!("{witness}\n").as_bytes(),
        )?;
        sync_directory(&generation_dir)?;
        sync_directory(&generations)?;
        sync_directory(&record)?;
        Ok(receipt)
    }

    fn current_receipt(&self, lease_id: &str) -> io::Result<Option<LeaseReceipt>> {
        let record = self.record_dir(lease_id)?;
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
        for entry in fs::read_dir(generations)? {
            let entry = entry?;
            let Some(generation) = entry
                .file_name()
                .to_str()
                .and_then(|value| value.parse::<u64>().ok())
            else {
                continue;
            };
            if latest
                .as_ref()
                .is_none_or(|(current, _)| generation > *current)
            {
                if let Ok(receipt) = self.read_generation(lease_id, generation) {
                    latest = Some((generation, receipt));
                }
            }
        }
        Ok(latest.map(|(_, receipt)| receipt))
    }

    fn read_generation(&self, lease_id: &str, generation: u64) -> io::Result<LeaseReceipt> {
        validate_id(lease_id, "lease id")?;
        if generation == 0 {
            return Err(invalid("executable lease generation is zero"));
        }
        let path = self
            .record_dir(lease_id)?
            .join("generations")
            .join(generation.to_string());
        let receipt_text = read_bounded(&path.join(RECEIPT_FILE))?;
        let receipt = parse_receipt(&receipt_text)?;
        if receipt.lease_id != lease_id || receipt.generation != generation {
            return Err(invalid("executable lease receipt identity mismatch"));
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
                fs::create_dir(&record)?;
                Ok(record)
            }
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
        self.validate_owner_fields(request.owner_pid, &request.owner_scope)
    }

    fn validate_owner_fields(&self, pid: u32, scope: &str) -> io::Result<()> {
        validate_scope(scope)?;
        if !self.owner_is_live(pid, scope) {
            return Err(invalid("executable lease owner lock is not held"));
        }
        Ok(())
    }

    fn owner_is_live(&self, _pid: u32, scope: &str) -> bool {
        let path = super::lock_path_for_scope(&self.root, scope);
        matches!(lock_state(&path), Ok(LockState::Held))
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

// Card #963: only the replacement and rollback builders bump a generation.
#[allow(dead_code)]
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
    match fs::rename(&partial, &path) {
        Ok(()) => sync_directory(service)?,
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

fn read_bounded(path: &Path) -> io::Result<String> {
    let file = fs::File::open(path)?;
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

fn remove_owned_tree(path: &Path) -> io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || metadata.is_file() {
        fs::remove_file(path)
    } else if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            remove_owned_tree(&entry?.path())?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
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

    fn scratch(tag: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "jet-exec-lease-{tag}-{}",
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn snapshot(root: &Path, name: &str, bytes: &str) -> (PathBuf, String) {
        let path = root.join("leases").join(name);
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
        let owner_scope = owner_scope(&lease_id).unwrap();
        let snapshot = root.join("leases").join("auth");
        fs::create_dir_all(&snapshot).unwrap();
        fs::write(snapshot.join("program"), "one").unwrap();
        let digest = crate::Envelope::try_output_hash_of(&snapshot.to_string_lossy()).unwrap();
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
    fn incomplete_generation_is_not_published_and_stale_owner_is_reclaimed() {
        let root = scratch("recovery");
        let protocol = ExecutableLeaseProtocol::open(&root).unwrap();
        let lease_id = random_id(16).unwrap();
        let owner_scope = owner_scope(&lease_id).unwrap();
        let (snapshot, digest) = snapshot(&root, "recovery", "one");
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
        drop(owner);
        assert_eq!(protocol.recover_stale_leases().unwrap(), 1);
        assert!(!snapshot.exists());
        assert!(incomplete.exists());
        assert!(protocol.read_generation(&receipt.lease_id, receipt.generation).is_err());
        drop(protocol);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn replacement_requires_the_current_digest_and_keeps_old_generation() {
        let root = scratch("replace");
        let protocol = ExecutableLeaseProtocol::open(&root).unwrap();
        let lease_id = random_id(16).unwrap();
        let owner_scope = owner_scope(&lease_id).unwrap();
        let (first, first_digest) = snapshot(&root, "first", "one");
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
        let (second, second_digest) = snapshot(&root, "second", "two");
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
        let rollback = protocol
            .prepare_rollback(&second_receipt, first_receipt.generation)
            .unwrap();
        assert_eq!(rollback, rollback);
        drop(owner);
        fs::remove_dir_all(root).unwrap();
    }
}
