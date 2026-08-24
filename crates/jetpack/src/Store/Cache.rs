//! Host-owned binary-cache bindings and the signed NAR transfer boundary.
//!
//! Workspace policy may request cache roles. This module owns the host-side
//! binding that maps a role to ordered mirrors, trust material, and an
//! optional typed credential-provider label. Secrets never enter this file,
//! URLs, argv, locks, or logs. Endpoint adapters exchange the same signed
//! narinfo and canonical NAR bytes, and an access failure is reported
//! before an object is made locally usable.

use super::{entry_action_key, NarInfo, ProducerRecord, Roots, StoreEntry};
use crate::TrustRoot::{
    allow_cache_builder, allow_cache_witness, cache_builder_identity, current_receipt_witness,
    is_cache_builder_allowed, is_cache_builder_revoked, is_cache_witness_allowed, os_random_bytes,
    pin_cache_key, verify_pinned_cache_key, CacheProvenance, CacheReceipt, Signature,
    SystemTrustedClock, TrustKey,
};
use crate::SHA256;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};
pub(crate) fn make_tree_writable_for_removal(path: &Path) -> std::io::Result<()> {
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
/// Validate then seal a locally produced output before it becomes reusable.
/// Files keep executable bits but lose all write bits; directories are sealed
/// bottom-up. Canonical archive validation rejects external hardlinks first.
pub fn seal_local_output(path: &Path) -> std::io::Result<()> {
    super::super::Envelope::try_output_hash_of(&path.to_string_lossy())
        .map_err(std::io::Error::other)?;
    seal_node(path)
}

pub(crate) fn seal_node(path: &Path) -> std::io::Result<()> {
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

pub(crate) fn fsync_tree(path: &Path) -> std::io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Ok(());
    }
    if meta.is_dir() {
        for entry in fs::read_dir(path)? {
            fsync_tree(&entry?.path())?;
        }
    }
    super::sync_store_node(path, meta.is_dir())
}

const BINDING_MAGIC: &str = "jet-cache-bind-v1";
const MAX_BINDING_BYTES: u64 = 1024 * 1024;
const MAX_INFO_BYTES: u64 = 1024 * 1024;
const NEGATIVE_CACHE_TTL_SECS: u64 = 60;
const CACHE_RECEIPT_TTL_SECS: u64 = 24 * 60 * 60;
const CACHE_RECEIPT_MAGIC: &str = "jet-cache-receipt-v1";

struct CacheArtifact {
    info: NarInfo,
    nar: Vec<u8>,
    receipt: CacheReceipt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CacheEndpoint {
    Local(PathBuf),
    Http(String),
    Ssh { target: String, root: String },
    S3(String),
    Nix(String),
    Hangar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EndpointCapabilities {
    read: bool,
    write: bool,
    promote: bool,
    remote_execute: bool,
    trust: bool,
    credential: bool,
    credential_required: bool,
}

impl CacheEndpoint {
    fn capabilities(&self) -> EndpointCapabilities {
        match self {
            Self::Local(_) => EndpointCapabilities {
                read: true,
                write: true,
                promote: true,
                remote_execute: false,
                trust: true,
                credential: false,
                credential_required: false,
            },
            // Hangar is the source store, not a cache publication transport.
            // It can satisfy a read locally, but publishing a NAR/narinfo back
            // into the same object is not a cache operation and must not report
            // success without writing the cache protocol objects.
            Self::Hangar => EndpointCapabilities {
                read: true,
                write: false,
                promote: false,
                remote_execute: false,
                trust: true,
                credential: false,
                credential_required: false,
            },
            Self::Ssh { .. } | Self::S3(_) => EndpointCapabilities {
                read: true,
                write: true,
                promote: true,
                remote_execute: false,
                trust: true,
                credential: true,
                credential_required: true,
            },
            Self::Http(_) => EndpointCapabilities {
                read: true,
                write: false,
                promote: false,
                remote_execute: false,
                trust: true,
                credential: true,
                credential_required: false,
            },
            // Nix transfers use the dedicated path-info/verify/copy adapter
            // below. They do not use cache-object PUT/GET or remote
            // execution, and Jetpack credentials are not implicitly attached
            // to the host's Nix configuration.
            Self::Nix(_) => EndpointCapabilities {
                read: true,
                write: true,
                promote: false,
                remote_execute: false,
                trust: true,
                credential: false,
                credential_required: false,
            },
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Local(_) => "file",
            Self::Http(_) => "http",
            Self::Ssh { .. } => "ssh",
            Self::S3(_) => "s3",
            Self::Nix(_) => "nix-store",
            Self::Hangar => "hangar",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheBinding {
    pub role: String,
    /// Ordered, host-owned mirror endpoints. The order is semantic.
    pub mirrors: Vec<String>,
    /// Host path to the trust key. The key bytes are never serialized in the
    /// binding and are never returned by inspection APIs.
    pub trust_key: PathBuf,
    pub credential_provider: Option<String>,
    /// Separate write authority. Read bindings cannot publish by accident.
    pub allow_write: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheTransferReport {
    pub role: String,
    pub mirror: String,
    pub entry: String,
    pub output_hash: String,
    /// Jetpack's digest of the canonical NAR bytes.
    pub nar_hash: String,
    /// The Nix spelling recorded at a Nix store boundary, when applicable.
    pub nix_nar_hash: Option<String>,
    /// Public signature identity, never secret credential material.
    pub signed_fingerprint: String,
    /// Provenance identities carried in the signed cache admission decision.
    pub builder: String,
    pub provenance: String,
    /// The signed admission receipt accepted for this transfer, when the
    /// endpoint uses Jetpack's native cache receipt protocol.
    pub witness: Option<String>,
    pub receipt_version: Option<u64>,
    pub receipt_expires_unix: Option<u64>,
    pub credential_provider: Option<String>,
    pub bytes: u64,
}

/// Read-only trust state exposed by package explanation. This is the host's
/// accepted admission pin, not a new cache decision path; transfer still
/// performs the full signature, provenance, freshness, and output checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CacheAdmission {
    pub role: String,
    pub decision: String,
    pub builder: String,
    pub provenance: String,
    pub receipt_version: Option<u64>,
    pub receipt_expires_unix: Option<u64>,
    pub reason: String,
}

impl CacheBinding {
    pub fn validate(&self) -> io::Result<()> {
        validate_component(&self.role, "cache role")?;
        if self.mirrors.is_empty() {
            return Err(invalid("cache binding needs at least one mirror"));
        }
        let mut seen = BTreeSet::new();
        for mirror in &self.mirrors {
            validate_endpoint(mirror)?;
            let endpoint = parse_endpoint(mirror)?;
            validate_endpoint_binding(self, &endpoint, self.allow_write)?;
            if !seen.insert(mirror.clone()) {
                return Err(invalid("cache binding contains a duplicate mirror"));
            }
        }
        if !self.trust_key.is_absolute() {
            return Err(invalid("cache trust key must be an absolute host path"));
        }
        validate_path_components(&self.trust_key)?;
        if let Some(provider) = &self.credential_provider {
            validate_text(provider, "credential provider")?;
            if provider.is_empty() {
                return Err(invalid("credential provider cannot be empty"));
            }
        }
        Ok(())
    }

    fn encode(&self) -> io::Result<String> {
        self.validate()?;
        let mut out = String::new();
        out.push_str(BINDING_MAGIC);
        out.push('\n');
        line(&mut out, "role", &self.role)?;
        line(&mut out, "key", &self.trust_key.to_string_lossy())?;
        line(
            &mut out,
            "write",
            if self.allow_write { "true" } else { "false" },
        )?;
        if let Some(provider) = &self.credential_provider {
            line(&mut out, "credential", provider)?;
        }
        for mirror in &self.mirrors {
            line(&mut out, "mirror", mirror)?;
        }
        Ok(out)
    }

    fn decode(text: &str) -> io::Result<Self> {
        if text.len() > MAX_BINDING_BYTES as usize {
            return Err(invalid("cache binding is too large"));
        }
        let mut role = None;
        let mut key = None;
        let mut mirrors = Vec::new();
        let mut credential = None;
        let mut allow_write = false;
        let mut seen = BTreeSet::new();
        let mut lines = text.lines();
        if lines.next() != Some(BINDING_MAGIC) {
            return Err(invalid("cache binding has an unknown format"));
        }
        for raw in lines {
            let (name, value) = raw
                .split_once('=')
                .ok_or_else(|| invalid("cache binding has a malformed line"))?;
            validate_text(name, "cache binding field")?;
            validate_text(value, "cache binding value")?;
            match name {
                "role" if role.is_none() => role = Some(value.to_string()),
                "key" if key.is_none() => key = Some(PathBuf::from(value)),
                "write" if !seen.contains("write") => {
                    allow_write = match value {
                        "true" => true,
                        "false" => false,
                        _ => return Err(invalid("cache binding write field is not boolean")),
                    };
                    seen.insert("write");
                }
                "credential" if credential.is_none() => credential = Some(value.to_string()),
                "mirror" => mirrors.push(value.to_string()),
                _ => return Err(invalid("cache binding has an unknown or duplicate field")),
            }
        }
        let binding = Self {
            role: role.ok_or_else(|| invalid("cache binding has no role"))?,
            mirrors,
            trust_key: key.ok_or_else(|| invalid("cache binding has no trust key"))?,
            credential_provider: credential,
            allow_write,
        };
        binding.validate()?;
        Ok(binding)
    }
}

/// Bind one host-owned cache role. Endpoint and trust policy are deliberately
/// not read from environment variables or repository files.
pub fn bind_cache(
    roots: &Roots,
    role: &str,
    mirrors: Vec<String>,
    trust_key: Option<&Path>,
    credential_provider: Option<String>,
    allow_write: bool,
) -> io::Result<CacheBinding> {
    let key_path = trust_key
        .map(Path::to_path_buf)
        .unwrap_or_else(|| roots.root.join("trust").join(format!("cache-{role}.key")));
    let key_path = absolutize_host_path(key_path)?;
    let binding = CacheBinding {
        role: role.to_string(),
        mirrors,
        trust_key: key_path,
        credential_provider,
        allow_write,
    };
    binding.validate()?;
    if let Some(requested) = trust_key {
        let metadata = fs::symlink_metadata(requested)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(invalid("explicit cache trust key is not a regular file"));
        }
        read_trust_key(&binding.trust_key)?;
    } else {
        match fs::symlink_metadata(&binding.trust_key) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err(invalid("cache trust key is not a regular file"));
            }
            Ok(_) => {
                read_trust_key(&binding.trust_key)?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                write_new_key(&binding.trust_key)?;
            }
            Err(error) => return Err(error),
        }
    }
    let key = read_trust_key(&binding.trust_key)?;
    pin_cache_key(&roots.root, role, &key).map_err(io::Error::other)?;
    let path = binding_path(roots, role)?;
    binding.validate()?;
    crate::RuntimePolicy::with_lock(&roots.root, "cache-config", || {
        ensure_parent(&path)?;
        atomic_replace(&path, binding.encode()?.as_bytes())
    })?;
    Ok(binding)
}

pub fn read_cache_binding(roots: &Roots, role: &str) -> io::Result<CacheBinding> {
    let path = binding_path(roots, role)?;
    let metadata = fs::symlink_metadata(&path)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_BINDING_BYTES
    {
        return Err(invalid(
            "cache binding is not a regular file within its limit",
        ));
    }
    let text = String::from_utf8(read_regular_bounded(&path, MAX_BINDING_BYTES)?)
        .map_err(|_| invalid("cache binding is not UTF-8"))?;
    let binding = CacheBinding::decode(&text)?;
    if binding.role != role {
        return Err(invalid("cache binding role disagrees with its file name"));
    }
    validate_cache_binding_trust(roots, &binding)?;
    Ok(binding)
}

pub fn list_cache_bindings(roots: &Roots) -> io::Result<Vec<CacheBinding>> {
    let dir = bindings_dir(roots);
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut paths = entries
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("conf"))
        .collect::<Vec<_>>();
    paths.sort();
    let mut bindings = Vec::new();
    for path in paths {
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() > MAX_BINDING_BYTES
        {
            return Err(invalid(
                "cache binding directory contains a non-regular entry",
            ));
        }
        let file_role = path
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| invalid("cache binding file name is not UTF-8"))?;
        let text = String::from_utf8(read_regular_bounded(&path, MAX_BINDING_BYTES)?)
            .map_err(|_| invalid("cache binding is not UTF-8"))?;
        let binding = CacheBinding::decode(&text)?;
        if binding.role != file_role {
            return Err(invalid("cache binding role disagrees with its file name"));
        }
        validate_cache_binding_trust(roots, &binding)?;
        bindings.push(binding);
    }
    Ok(bindings)
}

fn validate_cache_binding_trust(roots: &Roots, binding: &CacheBinding) -> io::Result<()> {
    let key = read_trust_key(&binding.trust_key)?;
    verify_pinned_cache_key(&roots.root, &binding.role, &key).map_err(io::Error::other)
}

pub fn remove_cache_binding(roots: &Roots, role: &str) -> io::Result<bool> {
    let path = binding_path(roots, role)?;
    crate::RuntimePolicy::with_lock(&roots.root, "cache-config", || {
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                Err(invalid("cache binding is not a regular file"))
            }
            Ok(_) => {
                fs::remove_file(path)?;
                Ok(true)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    })
}

pub fn publish_cache_entry(
    roots: &Roots,
    target: &str,
    role: &str,
) -> io::Result<CacheTransferReport> {
    let binding = read_cache_binding(roots, role)?;
    if !binding.allow_write {
        return Err(invalid(
            "cache binding is read-only; publishing needs a write grant",
        ));
    }
    let entry = select_entry(roots, target)?;
    ensure_reproducible_for_shared_cache(roots, &entry)?;
    let output = Path::new(&entry.out);
    let metadata = fs::symlink_metadata(output)?;
    if metadata.file_type().is_symlink() || (!metadata.is_dir() && !metadata.is_file()) {
        return Err(invalid(
            "only a local Hangar file or directory can be published as a NAR",
        ));
    }
    if entry.envelope.output_hash.is_empty() {
        return Err(invalid(
            "Hangar entry has no output identity for cache publication",
        ));
    }
    let actual_output_hash =
        super::try_entry_output_hash(roots, &entry).map_err(io::Error::other)?;
    if actual_output_hash != entry.envelope.output_hash {
        return Err(invalid(
            "local output does not match the Hangar entry output identity",
        ));
    }
    let (nar, stats) = super::write_nar(output)?;
    let key = read_trust_key(&binding.trust_key)?;
    verify_cache_writer_authority(roots, &entry, role, &key)?;
    let info = nar_info_for(&entry, &stats)?;
    verify_decoded_output_hash(roots, &entry, &nar)?;
    let mut failures = Vec::new();
    for mirror in &binding.mirrors {
        let endpoint = match parse_endpoint(mirror) {
            Ok(endpoint) => endpoint,
            Err(error) => {
                failures.push(format!("{mirror}: {error}"));
                continue;
            }
        };
        if let Err(error) = validate_endpoint_binding(&binding, &endpoint, true) {
            failures.push(format!("{mirror}: {error}"));
            continue;
        }
        match publish_endpoint(&endpoint, &info, &nar, &key, roots, &entry, role) {
            Ok(proof) => {
                let builder = cache_builder_for_entry(&entry)?;
                allow_cache_builder(&roots.root, role, &builder).map_err(io::Error::other)?;
                if let Some(witness) = &proof.witness {
                    allow_cache_witness(&roots.root, role, witness).map_err(io::Error::other)?;
                }
                return Ok(CacheTransferReport {
                    role: binding.role.clone(),
                    mirror: mirror.clone(),
                    entry: entry.id.clone(),
                    output_hash: entry.envelope.output_hash.clone(),
                    nar_hash: stats.digest,
                    nix_nar_hash: proof.nix_nar_hash,
                    signed_fingerprint: proof.signed_fingerprint,
                    builder: cache_builder_for_report(&entry),
                    provenance: cache_provenance_for_report(&entry),
                    witness: proof.witness,
                    receipt_version: proof.receipt_version,
                    receipt_expires_unix: proof.receipt_expires_unix,
                    credential_provider: binding.credential_provider.clone(),
                    bytes: stats.bytes,
                });
            }
            Err(error) => failures.push(format!("{mirror}: {error}")),
        }
    }
    Err(invalid(&format!(
        "all cache mirrors rejected publication: {}",
        failures.join("; ")
    )))
}

pub fn verify_cache_transfer(
    roots: &Roots,
    target: &str,
    role: &str,
) -> io::Result<CacheTransferReport> {
    let binding = read_cache_binding(roots, role)?;
    let expected = select_entry(roots, target)?;
    ensure_reproducible_for_shared_cache(roots, &expected)?;
    let key = read_trust_key(&binding.trust_key)?;
    verify_cache_writer_authority(roots, &expected, role, &key)?;
    let builder = cache_builder_for_entry(&expected)?;
    if !is_cache_builder_allowed(&roots.root, role, &builder).map_err(io::Error::other)? {
        return Err(invalid(
            "cache builder is not allowlisted for this shared cache role",
        ));
    }
    let mut failures = Vec::new();
    for mirror in &binding.mirrors {
        let endpoint = match parse_endpoint(mirror) {
            Ok(endpoint) => endpoint,
            Err(error) => {
                failures.push(format!("{mirror}: {error}"));
                continue;
            }
        };
        if let Err(error) = validate_endpoint_binding(&binding, &endpoint, false) {
            failures.push(format!("{mirror}: {error}"));
            continue;
        }
        match verify_hangar_endpoint(&endpoint, &expected) {
            Ok(Some(bytes)) => {
                return Ok(CacheTransferReport {
                    role: binding.role.clone(),
                    mirror: mirror.clone(),
                    entry: expected.id.clone(),
                    output_hash: expected.envelope.output_hash.clone(),
                    nar_hash: super::nar_digest(&bytes),
                    nix_nar_hash: None,
                    signed_fingerprint: fingerprint_for_key(&key),
                    builder: cache_builder_for_report(&expected),
                    provenance: cache_provenance_for_report(&expected),
                    witness: None,
                    receipt_version: None,
                    receipt_expires_unix: None,
                    credential_provider: binding.credential_provider.clone(),
                    bytes: bytes.len() as u64,
                });
            }
            Ok(None) => {}
            Err(error) => failures.push(format!("{mirror}: {error}")),
        }
        match verify_nix_endpoint(&endpoint, &expected, &key) {
            Ok(Some(transfer)) => {
                return Ok(CacheTransferReport {
                    role: binding.role.clone(),
                    mirror: mirror.clone(),
                    entry: expected.id.clone(),
                    output_hash: expected.envelope.output_hash.clone(),
                    nar_hash: transfer.nar_hash,
                    nix_nar_hash: Some(transfer.nix_nar_hash),
                    signed_fingerprint: transfer.signed_fingerprint,
                    builder: cache_builder_for_report(&expected),
                    provenance: cache_provenance_for_report(&expected),
                    witness: None,
                    receipt_version: None,
                    receipt_expires_unix: None,
                    credential_provider: binding.credential_provider.clone(),
                    bytes: transfer.nar.len() as u64,
                });
            }
            Ok(None) => {}
            Err(error) => failures.push(format!("{mirror}: {error}")),
        }
        match find_artifact(&endpoint, target, Some(&expected), &key) {
            Ok(Some(artifact)) => {
                let bytes = artifact.nar;
                if let Err(error) = verify_artifact_bytes(&artifact.info, &bytes) {
                    failures.push(format!("{mirror}: NAR validation failed: {error}"));
                    continue;
                }
                if let Err(error) = verify_decoded_output_hash(roots, &expected, &bytes) {
                    failures.push(format!("{mirror}: output identity failed: {error}"));
                    continue;
                }
                // Admit the signed receipt only after the complete payload has
                // passed transport, NAR, and output-identity checks. A bad or
                // compromised mirror must not advance the host pin and then
                // freeze a later valid receipt behind a rollback decision.
                if let Err(error) =
                    verify_cache_receipt(roots, role, &expected, &artifact.receipt, &key)
                {
                    failures.push(format!("{mirror}: trust receipt rejected: {error}"));
                    continue;
                }
                let receipt = artifact.receipt;
                let info = artifact.info;
                return Ok(report_for(
                    &binding,
                    mirror,
                    info,
                    bytes.len() as u64,
                    Some(&expected),
                    Some(&receipt),
                ));
            }
            Ok(None) => failures.push(format!("{mirror}: cache entry not found")),
            Err(error) => failures.push(format!("{mirror}: {error}")),
        }
    }
    Err(invalid(&format!(
        "no verifying cache hit: {}",
        failures.join("; ")
    )))
}

pub fn substitute_cache_entry(
    roots: &Roots,
    target: &str,
    role: &str,
    destination: &Path,
) -> io::Result<CacheTransferReport> {
    let binding = read_cache_binding(roots, role)?;
    if fs::symlink_metadata(destination).is_ok() {
        return Err(invalid("cache substitution destination already exists"));
    }
    validate_path_components(destination)?;
    let expected = select_entry(roots, target)?;
    ensure_reproducible_for_shared_cache(roots, &expected)?;
    let key = read_trust_key(&binding.trust_key)?;
    verify_cache_writer_authority(roots, &expected, role, &key)?;
    let builder = cache_builder_for_entry(&expected)?;
    if !is_cache_builder_allowed(&roots.root, role, &builder).map_err(io::Error::other)? {
        return Err(invalid(
            "cache builder is not allowlisted for this shared cache role",
        ));
    }
    let mut failures = Vec::new();
    for mirror in &binding.mirrors {
        let endpoint = match parse_endpoint(mirror) {
            Ok(endpoint) => endpoint,
            Err(error) => {
                failures.push(format!("{mirror}: {error}"));
                continue;
            }
        };
        if let Err(error) = validate_endpoint_binding(&binding, &endpoint, false) {
            failures.push(format!("{mirror}: {error}"));
            continue;
        }
        match substitute_hangar_endpoint(&endpoint, &expected, destination) {
            Ok(Some(bytes)) => {
                return Ok(CacheTransferReport {
                    role: binding.role.clone(),
                    mirror: mirror.clone(),
                    entry: expected.id.clone(),
                    output_hash: expected.envelope.output_hash.clone(),
                    nar_hash: super::nar_digest(&bytes),
                    nix_nar_hash: None,
                    signed_fingerprint: fingerprint_for_key(&key),
                    builder: cache_builder_for_report(&expected),
                    provenance: cache_provenance_for_report(&expected),
                    witness: None,
                    receipt_version: None,
                    receipt_expires_unix: None,
                    credential_provider: binding.credential_provider.clone(),
                    bytes: bytes.len() as u64,
                });
            }
            Ok(None) => {}
            Err(error) => failures.push(format!("{mirror}: {error}")),
        }
        match substitute_nix_endpoint(&endpoint, &expected, destination, &key) {
            Ok(Some(transfer)) => {
                return Ok(CacheTransferReport {
                    role: binding.role.clone(),
                    mirror: mirror.clone(),
                    entry: expected.id.clone(),
                    output_hash: expected.envelope.output_hash.clone(),
                    nar_hash: transfer.nar_hash,
                    nix_nar_hash: Some(transfer.nix_nar_hash),
                    signed_fingerprint: transfer.signed_fingerprint,
                    builder: cache_builder_for_report(&expected),
                    provenance: cache_provenance_for_report(&expected),
                    witness: None,
                    receipt_version: None,
                    receipt_expires_unix: None,
                    credential_provider: binding.credential_provider.clone(),
                    bytes: transfer.nar.len() as u64,
                });
            }
            Ok(None) => {}
            Err(error) => failures.push(format!("{mirror}: {error}")),
        }
        match find_artifact(&endpoint, target, Some(&expected), &key) {
            Ok(Some(artifact)) => {
                let result: io::Result<_> = (|| {
                    artifact.info.verify(&key)?;
                    verify_artifact_bytes(&artifact.info, &artifact.nar)?;
                    let stats = super::read_nar(&artifact.nar, destination)?;
                    super::seal_node(destination)?;
                    let actual =
                        crate::Envelope::try_output_hash_of(&destination.to_string_lossy())
                            .map_err(io::Error::other)?;
                    if actual != expected.envelope.output_hash {
                        return Err(invalid(&format!(
                            "restored output hash {actual} disagrees with {}",
                            expected.envelope.output_hash
                        )));
                    }
                    // Keep receipt admission transactional with the restored
                    // payload. Invalid bytes must not consume a newer pin.
                    verify_cache_receipt(roots, role, &expected, &artifact.receipt, &key)?;
                    Ok(stats)
                })();
                match result {
                    Ok(stats) => {
                        return Ok(CacheTransferReport {
                            role: binding.role.clone(),
                            mirror: mirror.clone(),
                            entry: expected.id.clone(),
                            output_hash: expected.envelope.output_hash.clone(),
                            nar_hash: stats.digest,
                            nix_nar_hash: None,
                            signed_fingerprint: fingerprint_for_info(&artifact.info),
                            builder: cache_builder_for_report(&expected),
                            provenance: cache_provenance_for_report(&expected),
                            witness: Some(artifact.receipt.witness.clone()),
                            receipt_version: Some(artifact.receipt.version),
                            receipt_expires_unix: Some(artifact.receipt.expires_unix),
                            credential_provider: binding.credential_provider.clone(),
                            bytes: stats.bytes,
                        });
                    }
                    Err(error) => {
                        let _ = remove_tree(destination);
                        failures.push(format!("{mirror}: {error}"));
                    }
                }
            }
            Ok(None) => failures.push(format!("{mirror}: cache entry not found")),
            Err(error) => failures.push(format!("{mirror}: {error}")),
        }
    }
    Err(invalid(&format!(
        "no verifying cache hit: {}",
        failures.join("; ")
    )))
}

fn cache_binding_fields(binding: &CacheBinding) -> String {
    let mirrors = binding
        .mirrors
        .iter()
        .map(|mirror| crate::JSON::quote(mirror))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "\"role\":{},\"mirrors\":[{}],\"credential_provider\":{},\"write\":{}",
        crate::JSON::quote(&binding.role),
        mirrors,
        binding
            .credential_provider
            .as_deref()
            .map(crate::JSON::quote)
            .unwrap_or_else(|| "null".to_string()),
        if binding.allow_write { "true" } else { "false" }
    )
}

pub fn cache_binding_json(binding: &CacheBinding) -> String {
    jet_foundation::Report::render_status_json(
        "ok",
        true,
        "cache-bind",
        &format!(",{}", cache_binding_fields(binding)),
    )
}

pub(crate) fn cache_bindings_json(bindings: &[CacheBinding]) -> String {
    let values = bindings
        .iter()
        .map(|binding| format!("{{{}}}", cache_binding_fields(binding)))
        .collect::<Vec<_>>()
        .join(",");
    jet_foundation::Report::render_status_json(
        "ok",
        true,
        "cache-list",
        &format!(",\"bindings\":[{}]", values),
    )
}

pub fn cache_report_json(operation: &str, report: &CacheTransferReport) -> String {
    jet_foundation::Report::render_status_json(
        "ok",
        true,
        operation,
        &format!(
            ",\"operation\":{},\"role\":{},\"mirror\":{},\"entry\":{},\"output_hash\":{},\"nar_hash\":{},\"nix_nar_hash\":{},\"signed_fingerprint\":{},\"builder\":{},\"provenance\":{},\"witness\":{},\"receipt_version\":{},\"receipt_expires_unix\":{},\"credential_provider\":{},\"bytes\":{}",
            crate::JSON::quote(operation),
            crate::JSON::quote(&report.role),
            crate::JSON::quote(&report.mirror),
            crate::JSON::quote(&report.entry),
            crate::JSON::quote(&report.output_hash),
            crate::JSON::quote(&report.nar_hash),
            report
                .nix_nar_hash
                .as_deref()
                .map(crate::JSON::quote)
                .unwrap_or_else(|| "null".to_string()),
            crate::JSON::quote(&report.signed_fingerprint),
            crate::JSON::quote(&report.builder),
            crate::JSON::quote(&report.provenance),
            report
                .witness
                .as_deref()
                .map(crate::JSON::quote)
                .unwrap_or_else(|| "null".to_string()),
            report
                .receipt_version
                .map(|version| version.to_string())
                .unwrap_or_else(|| "null".to_string()),
            report
                .receipt_expires_unix
                .map(|expires| expires.to_string())
                .unwrap_or_else(|| "null".to_string()),
            report
                .credential_provider
                .as_deref()
                .map(crate::JSON::quote)
                .unwrap_or_else(|| "null".to_string()),
            report.bytes
        ),
    )
}

fn select_entry(roots: &Roots, target: &str) -> io::Result<StoreEntry> {
    super::list_checked(roots)?
        .into_iter()
        .find(|entry| {
            entry.id == target
                || entry.reference == target
                || entry.envelope.output_hash == target
                || format!("{}@{}", entry.name, entry.version) == target
        })
        .ok_or_else(|| invalid("no Hangar entry matches the cache target"))
}

fn ensure_reproducible_for_shared_cache(roots: &Roots, entry: &StoreEntry) -> io::Result<()> {
    let action_key = entry_action_key(entry);
    if super::reproducibility_blocked(roots, &action_key)? {
        return Err(invalid(
            "action has unreproducible evidence and cannot satisfy trusted cache policy",
        ));
    }
    Ok(())
}

fn nar_info_for(entry: &StoreEntry, stats: &super::NarStats) -> io::Result<NarInfo> {
    let name = format!("{}-{}", entry.envelope.output_hash, entry.id);
    Ok(NarInfo {
        store_path: format!("/jet/hangar/{name}"),
        url: format!("nar/{}.nar", entry.envelope.output_hash),
        compression: "none".to_string(),
        file_size: stats.bytes,
        nar_size: stats.bytes,
        nar_hash: stats.digest.clone(),
        references: entry
            .references
            .iter()
            .map(|reference| format!("/jet/hangar/{reference}"))
            .collect(),
        deriver: Some(cache_deriver(entry)?),
        ca: None,
        signatures: Vec::new(),
    })
}

/// Carry the existing action identity and producer record through the signed
/// narinfo. The NAR itself carries bytes; this field carries the facts that
/// make those bytes eligible for this cache entry.
fn cache_deriver(entry: &StoreEntry) -> io::Result<String> {
    if entry.envelope.output_hash.trim().is_empty()
        || entry.envelope.platform.trim().is_empty()
        || entry.envelope.provenance.trim().is_empty()
        || entry.cache_identity.source_fingerprint.trim().is_empty()
        || entry.cache_identity.recipe_fingerprint.trim().is_empty()
        || entry.cache_identity.policy_fingerprint.trim().is_empty()
        || entry.cache_identity.platform != entry.envelope.platform
    {
        return Err(invalid(
            "cache entry has incomplete or mismatched provenance",
        ));
    }
    ProducerRecord::decode(&entry.producer_record).map_err(|error| {
        invalid(&format!(
            "cache entry has invalid producer provenance: {error}"
        ))
    })?;

    let mut canonical = b"jet.cache-deriver.v2\0".to_vec();
    for value in [
        entry_action_key(entry),
        entry.envelope.output_hash.clone(),
        entry.envelope.platform.clone(),
        entry.envelope.signature.clone(),
        entry.envelope.provenance.clone(),
        entry.cache_identity.source_fingerprint.clone(),
        entry.cache_identity.recipe_fingerprint.clone(),
        entry.cache_identity.policy_fingerprint.clone(),
        entry.cache_identity.platform.clone(),
        entry.platform_artifact_kind.clone(),
        entry.producer_record.clone(),
    ] {
        canonical.extend_from_slice(&(value.len() as u64).to_be_bytes());
        canonical.extend_from_slice(value.as_bytes());
    }
    canonical.extend_from_slice(&(entry.named_outputs.len() as u64).to_be_bytes());
    for (name, digest) in &entry.named_outputs {
        for value in [name, digest] {
            canonical.extend_from_slice(&(value.len() as u64).to_be_bytes());
            canonical.extend_from_slice(value.as_bytes());
        }
    }
    Ok(format!(
        "/jet/derivations/{}",
        SHA256::sha256_hex(&canonical)
    ))
}

fn find_artifact(
    endpoint: &CacheEndpoint,
    _target: &str,
    expected: Option<&StoreEntry>,
    key: &TrustKey,
) -> io::Result<Option<CacheArtifact>> {
    match endpoint {
        CacheEndpoint::Local(mirror) => find_local_artifact(mirror, expected, key),
        CacheEndpoint::Hangar | CacheEndpoint::Nix(_) => Ok(None),
        CacheEndpoint::Http(_) | CacheEndpoint::Ssh { .. } | CacheEndpoint::S3(_) => {
            let Some(expected) = expected else {
                return Ok(None);
            };
            let store_name = format!("{}-{}", expected.envelope.output_hash, expected.id);
            let info_path = format!("{store_name}.narinfo");
            let Some(info_bytes) = endpoint_get(endpoint, &info_path, MAX_INFO_BYTES)? else {
                return Ok(None);
            };
            let text =
                String::from_utf8(info_bytes).map_err(|_| invalid("narinfo is not UTF-8"))?;
            let info = NarInfo::parse(&text)?;
            if info.store_path.rsplit('/').next() != Some(store_name.as_str())
                || !nar_info_matches_entry(&info, expected)
            {
                return Ok(None);
            }
            info.verify(key)?;
            let receipt = endpoint_get(endpoint, &cache_receipt_key(&store_name)?, MAX_INFO_BYTES)?
                .ok_or_else(|| invalid("cache entry has no signed trust receipt"))
                .and_then(|bytes| decode_cache_receipt(&bytes))?;
            let Some(nar) = endpoint_get(endpoint, &info.url, super::MAX_NAR_BYTES as u64)? else {
                return Ok(None);
            };
            Ok(Some(CacheArtifact { info, nar, receipt }))
        }
    }
}

fn find_local_artifact(
    mirror: &Path,
    expected: Option<&StoreEntry>,
    key: &TrustKey,
) -> io::Result<Option<CacheArtifact>> {
    if let Some(expected) = expected {
        let identity = format!("{}-{}", expected.envelope.output_hash, expected.id);
        if negative_hint_fresh(mirror, &identity) {
            return Ok(None);
        }
    }
    let metadata = match fs::symlink_metadata(mirror) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(invalid("cache mirror is not a real directory"));
    }
    let mut infos = fs::read_dir(mirror)?.collect::<Result<Vec<_>, _>>()?;
    infos.sort_by_key(|entry| entry.file_name());
    for directory_entry in infos {
        let info_path = directory_entry.path();
        if info_path.extension().and_then(|value| value.to_str()) != Some("narinfo") {
            continue;
        }
        let info_metadata = fs::symlink_metadata(&info_path)?;
        if info_metadata.file_type().is_symlink() || !info_metadata.is_file() {
            return Err(invalid("cache mirror contains a non-regular narinfo"));
        }
        let text = read_regular_bounded(&info_path, MAX_INFO_BYTES)?;
        let text = String::from_utf8(text).map_err(|_| invalid("narinfo is not UTF-8"))?;
        let info = match NarInfo::parse(&text) {
            Ok(info) => info,
            Err(_) => continue,
        };
        let store_name = info.store_path.rsplit('/').next().unwrap_or_default();
        let expected_name =
            expected.map(|entry| format!("{}-{}", entry.envelope.output_hash, entry.id));
        let matches = expected_name.as_deref() == Some(store_name)
            && expected.is_some_and(|entry| nar_info_matches_entry(&info, entry));
        if !matches {
            continue;
        }
        if info.verify(key).is_err() {
            continue;
        }
        let receipt_path = mirror.join(cache_receipt_key(store_name)?);
        let receipt = match read_regular_bounded(&receipt_path, MAX_INFO_BYTES)
            .and_then(|bytes| decode_cache_receipt(&bytes))
        {
            Ok(receipt) => receipt,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(invalid("cache entry has no signed trust receipt"));
            }
            Err(error) => return Err(error),
        };
        let nar_path = mirror.join(&info.url);
        validate_path_components(&nar_path)?;
        let nar = match read_regular_bounded(&nar_path, super::MAX_NAR_BYTES as u64) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        if nar.is_empty() {
            continue;
        }
        if verify_artifact_bytes(&info, &nar).is_err() {
            continue;
        }
        return Ok(Some(CacheArtifact { info, nar, receipt }));
    }
    if let Some(expected) = expected {
        let identity = format!("{}-{}", expected.envelope.output_hash, expected.id);
        // A negative result is only an advisory optimization. A read-only or
        // disappearing mirror must not turn a source/cache miss into a hard
        // failure, and it must never suppress the next ordered mirror.
        let _ = write_negative_hint(mirror, &identity);
    }
    Ok(None)
}

fn report_for(
    binding: &CacheBinding,
    mirror: &str,
    info: NarInfo,
    bytes: u64,
    expected: Option<&StoreEntry>,
    receipt: Option<&CacheReceipt>,
) -> CacheTransferReport {
    CacheTransferReport {
        role: binding.role.clone(),
        mirror: mirror.to_string(),
        entry: expected
            .map(|entry| entry.id.clone())
            .unwrap_or_else(|| info.store_path.clone()),
        output_hash: expected
            .map(|entry| entry.envelope.output_hash.clone())
            .unwrap_or_default(),
        nar_hash: info.nar_hash.clone(),
        nix_nar_hash: None,
        signed_fingerprint: fingerprint_for_info(&info),
        builder: expected.map(cache_builder_for_report).unwrap_or_default(),
        provenance: expected
            .map(cache_provenance_for_report)
            .unwrap_or_default(),
        witness: receipt.map(|receipt| receipt.witness.clone()),
        receipt_version: receipt.map(|receipt| receipt.version),
        receipt_expires_unix: receipt.map(|receipt| receipt.expires_unix),
        credential_provider: binding.credential_provider.clone(),
        bytes,
    }
}

/// Project persisted cache admission pins without contacting a mirror or
/// changing trust state. Explain is observational; the transfer boundary is
/// the only path that admits bytes.
pub(crate) fn cache_admissions_for_explain(
    roots: &Roots,
    entry: &StoreEntry,
) -> io::Result<Vec<CacheAdmission>> {
    let bindings = list_cache_bindings(roots)?;
    let (builder, provenance) = match cache_provenance_for_entry(entry) {
        Ok(provenance) => (provenance.builder, cache_deriver(entry).unwrap_or_default()),
        Err(error) => {
            return Ok(bindings
                .into_iter()
                .map(|binding| CacheAdmission {
                    role: binding.role,
                    decision: "denied".to_string(),
                    builder: String::new(),
                    provenance: String::new(),
                    receipt_version: None,
                    receipt_expires_unix: None,
                    reason: format!("cache provenance is not admissible: {error}"),
                })
                .collect())
        }
    };
    let store_name = format!("{}-{}", entry.envelope.output_hash, entry.id);
    let now = now_seconds();
    let builder_revoked =
        is_cache_builder_revoked(&roots.root, &builder).map_err(io::Error::other)?;
    bindings
        .into_iter()
        .map(|binding| {
            let role = binding.role;
            let pin = cache_receipt_pin_path(roots, &role, &store_name)?;
            let (decision, receipt_version, receipt_expires_unix, reason) = if builder_revoked {
                (
                    "denied".to_string(),
                    None,
                    None,
                    "cache builder is revoked; rebuild before reuse".to_string(),
                )
            } else {
                match read_cache_receipt_pin(&pin) {
                    Ok(pin) if pin.expires_unix <= now => (
                        "expired".to_string(),
                        Some(pin.version),
                        Some(pin.expires_unix),
                        format!(
                            "accepted cache receipt expired at {}; refresh or rebuild",
                            pin.expires_unix
                        ),
                    ),
                    Ok(pin) => (
                        "accepted".to_string(),
                        Some(pin.version),
                        Some(pin.expires_unix),
                        "signed cache admission is pinned for this role and output".to_string(),
                    ),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => (
                        "not-accepted".to_string(),
                        None,
                        None,
                        "no signed cache admission has been accepted for this role and output"
                            .to_string(),
                    ),
                    Err(error) => (
                        "invalid".to_string(),
                        None,
                        None,
                        format!("cache admission pin is unusable: {error}"),
                    ),
                }
            };
            Ok(CacheAdmission {
                role,
                decision,
                builder: builder.clone(),
                provenance: provenance.clone(),
                receipt_version,
                receipt_expires_unix,
                reason,
            })
        })
        .collect()
}

fn fingerprint_for_key(key: &TrustKey) -> String {
    format!("{}:{}", key.key_id, key.algorithm)
}

fn fingerprint_for_info(info: &NarInfo) -> String {
    info.signatures
        .iter()
        .map(|signature| format!("{}:{}", signature.key_id, signature.algorithm))
        .collect::<Vec<_>>()
        .join(",")
}

fn verify_artifact_bytes(info: &NarInfo, nar: &[u8]) -> io::Result<()> {
    if info.file_size != nar.len() as u64
        || info.nar_size != nar.len() as u64
        || !super::nar_hash_matches(&info.nar_hash, nar)
    {
        return Err(invalid(
            "NAR bytes do not match signed FileSize, NarSize, or NarHash",
        ));
    }
    super::validate_nar(nar)?;
    Ok(())
}

/// Verify the decoded output identity before a cache hit is reported. NAR
/// validity and NAR digest prove the transport object; the Hangar envelope is
/// a different digest over the materialized output tree. Decode only into a
/// private staging path, then remove it on every path.
fn verify_decoded_output_hash(roots: &Roots, expected: &StoreEntry, nar: &[u8]) -> io::Result<()> {
    validate_component(&expected.id, "cache entry id")?;
    let staging = roots.root.join("cache").join("verify").join(format!(
        "{}-{}",
        expected.id,
        unique_suffix()
    ));
    validate_path_components(&staging)?;
    ensure_parent(&staging)?;
    let result = (|| {
        super::read_nar(nar, &staging)?;
        super::seal_node(&staging)?;
        let actual = crate::Envelope::try_output_hash_of(&staging.to_string_lossy())
            .map_err(io::Error::other)?;
        if actual != expected.envelope.output_hash {
            return Err(invalid(&format!(
                "decoded output hash {actual} disagrees with {}",
                expected.envelope.output_hash
            )));
        }
        Ok(())
    })();
    if fs::symlink_metadata(&staging).is_ok() {
        make_tree_writable_for_removal(&staging)?;
        remove_tree(&staging)?;
    }
    result
}

fn nar_info_matches_entry(info: &NarInfo, entry: &StoreEntry) -> bool {
    let expected_name = format!("{}-{}", entry.envelope.output_hash, entry.id);
    let expected_path = format!("/jet/hangar/{expected_name}");
    let expected_url = format!("nar/{}.nar", entry.envelope.output_hash);
    let mut expected_references = entry
        .references
        .iter()
        .map(|reference| format!("/jet/hangar/{reference}"))
        .collect::<Vec<_>>();
    expected_references.sort();
    let mut actual_references = info.references.clone();
    actual_references.sort();
    let expected_deriver = cache_deriver(entry).ok();
    info.store_path == expected_path
        && info.url == expected_url
        && actual_references == expected_references
        && info.deriver == expected_deriver
        && info.ca.is_none()
}

fn read_trust_key(path: &Path) -> io::Result<TrustKey> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            invalid(&format!(
                "cache trust key `{}` is missing; rebind the cache role",
                path.display()
            ))
        } else {
            error
        }
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > 4096 {
        return Err(invalid("cache trust key is not a regular file"));
    }
    TrustKey::from_secret(read_regular_bounded(path, 4096)?)
        .map_err(|error| invalid(&error.to_string()))
}

fn verify_cache_writer_authority(
    roots: &Roots,
    entry: &StoreEntry,
    role: &str,
    key: &TrustKey,
) -> io::Result<()> {
    verify_pinned_cache_key(&roots.root, role, key).map_err(io::Error::other)?;
    let builder = cache_builder_for_entry(entry)?;
    if is_cache_builder_revoked(&roots.root, &builder).map_err(io::Error::other)? {
        return Err(invalid(
            "cache builder is revoked; rebuild before publishing or using it",
        ));
    }
    Ok(())
}

fn cache_builder_for_entry(entry: &StoreEntry) -> io::Result<String> {
    Ok(cache_provenance_for_entry(entry)?.builder)
}

fn cache_provenance_for_entry(entry: &StoreEntry) -> io::Result<CacheProvenance> {
    if entry.reference.trim().is_empty()
        || entry.envelope.provenance.trim().is_empty()
        || entry.cache_identity.source_fingerprint.trim().is_empty()
        || entry.cache_identity.recipe_fingerprint.trim().is_empty()
        || entry.cache_identity.policy_fingerprint.trim().is_empty()
        || entry.cache_identity.platform != entry.envelope.platform
    {
        return Err(invalid("cache entry has incomplete writer provenance"));
    }
    let producer = ProducerRecord::decode(&entry.producer_record)
        .map_err(|error| invalid(&format!("cache producer record is invalid: {error}")))?;
    if super::is_private_untrusted_build(&producer) {
        return Err(invalid(
            "private untrusted local build output cannot enter shared cache publication",
        ));
    }
    let expected_policy = format!(
        "policy={}\nplatform={}",
        entry.cache_identity.policy_fingerprint, entry.cache_identity.platform
    );
    if producer.immutable_source.trim().is_empty()
        || producer.source_digest.trim().is_empty()
        || producer.facts.get("action.recipe").map(String::as_str)
            != Some(entry.cache_identity.recipe_fingerprint.as_str())
        || producer.policy_facts != expected_policy
        || !producer
            .facts
            .get("cache.reproducibility")
            .is_some_and(|value| {
                value == "attested-v1" || value.starts_with("independent-agreeing-v1:")
            })
    {
        return Err(invalid("cache entry has unverified writer provenance"));
    }
    let builder = cache_builder_identity(
        &producer.provider,
        &producer.immutable_source,
        &producer.source_digest,
    );
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
    if provenance.validate().is_err()
        || provenance.reference != entry.reference
        || provenance.source != producer.immutable_source
        || provenance.builder != builder
        || provenance.action
            != super::cache_action_identity(
                &producer,
                &entry.reference,
                &entry.cache_identity,
                &entry.references,
            )
        || provenance.output != entry.envelope.output_hash
        || provenance.platform != entry.cache_identity.platform
        || provenance.sandbox != "sandbox:policy-bound"
        || provenance.policy != entry.cache_identity.policy_fingerprint
    {
        return Err(invalid("cache entry has incomplete writer provenance"));
    }
    Ok(provenance)
}

fn cache_builder_for_report(entry: &StoreEntry) -> String {
    cache_builder_for_entry(entry).unwrap_or_default()
}

fn cache_provenance_for_report(entry: &StoreEntry) -> String {
    cache_deriver(entry).unwrap_or_default()
}

fn cache_receipt_key(store_name: &str) -> io::Result<String> {
    validate_component(store_name, "cache receipt store name")?;
    Ok(format!("trust/{store_name}.receipt"))
}

fn cache_receipt_pin_path(roots: &Roots, role: &str, store_name: &str) -> io::Result<PathBuf> {
    validate_component(role, "cache role")?;
    validate_component(store_name, "cache receipt store name")?;
    let pin = roots
        .root
        .join("trust")
        .join("cache-receipts")
        .join(role)
        .join(format!("{store_name}.pin"));
    validate_path_components(&pin)?;
    Ok(pin)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CacheReceiptPin {
    version: u64,
    expires_unix: u64,
}

fn read_cache_receipt_pin(path: &Path) -> io::Result<CacheReceiptPin> {
    let text = String::from_utf8(read_regular_bounded(path, 4096)?)
        .map_err(|_| invalid("cache trust pin is not UTF-8"))?;
    let mut version = None;
    let mut expires_unix = None;
    let mut digest = None;
    for raw in text.lines() {
        let (name, value) = raw
            .split_once('=')
            .ok_or_else(|| invalid("cache trust pin has a malformed line"))?;
        let slot = match name {
            "version" => &mut version,
            "expires" => &mut expires_unix,
            "digest" => &mut digest,
            _ => return Err(invalid("cache trust pin has an unknown field")),
        };
        if slot.is_some() {
            return Err(invalid("cache trust pin has a duplicate field"));
        }
        *slot = Some(value.to_string());
    }
    let version = version
        .ok_or_else(|| invalid("cache trust pin has no version"))?
        .parse::<u64>()
        .map_err(|_| invalid("cache trust pin version is not an integer"))?;
    let expires_unix = expires_unix
        .ok_or_else(|| invalid("cache trust pin has no expiry"))?
        .parse::<u64>()
        .map_err(|_| invalid("cache trust pin expiry is not an integer"))?;
    let digest = digest.ok_or_else(|| invalid("cache trust pin has no digest"))?;
    if version == 0
        || expires_unix == 0
        || digest.len() != 64
        || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(invalid("cache trust pin has invalid fields"));
    }
    Ok(CacheReceiptPin {
        version,
        expires_unix,
    })
}

fn cache_receipt_for_publication(
    endpoint: &CacheEndpoint,
    role: &str,
    entry: &StoreEntry,
    key: &TrustKey,
) -> io::Result<CacheReceipt> {
    validate_component(role, "cache role")?;
    let provenance = cache_provenance_for_entry(entry)?;
    let store_name = format!("{}-{}", entry.envelope.output_hash, entry.id);
    let existing = match endpoint {
        CacheEndpoint::Local(_)
        | CacheEndpoint::Http(_)
        | CacheEndpoint::Ssh { .. }
        | CacheEndpoint::S3(_) => {
            endpoint_get(endpoint, &cache_receipt_key(&store_name)?, MAX_INFO_BYTES)?
                .map(|bytes| decode_cache_receipt(&bytes))
                .transpose()?
        }
        CacheEndpoint::Nix(_) | CacheEndpoint::Hangar => None,
    };
    if let Some(existing) = existing {
        let current_witness = current_receipt_witness().map_err(io::Error::other)?;
        if existing.role == role
            && existing.witness == current_witness
            && existing.provenance == provenance
            && existing.verify(key, &SystemTrustedClock).is_ok()
        {
            return Ok(existing);
        }
        let version = existing.version.saturating_add(1).max(1);
        return CacheReceipt::issue(
            role,
            provenance,
            version,
            now_seconds(),
            now_seconds().saturating_add(CACHE_RECEIPT_TTL_SECS),
            key,
        )
        .map_err(|error| invalid(&error.to_string()));
    }
    CacheReceipt::issue(
        role,
        provenance,
        1,
        now_seconds(),
        now_seconds().saturating_add(CACHE_RECEIPT_TTL_SECS),
        key,
    )
    .map_err(|error| invalid(&error.to_string()))
}

fn encode_cache_receipt(receipt: &CacheReceipt) -> io::Result<String> {
    if receipt.witness.trim().is_empty() || receipt.witness.chars().any(char::is_control) {
        return Err(invalid(
            "cache trust receipt has an empty witness or control characters",
        ));
    }
    receipt
        .provenance
        .validate()
        .map_err(|error| invalid(&error.to_string()))?;
    let mut out = String::from(CACHE_RECEIPT_MAGIC);
    out.push('\n');
    line(&mut out, "role", &receipt.role)?;
    line(&mut out, "witness", &receipt.witness)?;
    line(&mut out, "version", &receipt.version.to_string())?;
    line(&mut out, "issued", &receipt.issued_unix.to_string())?;
    line(&mut out, "expires", &receipt.expires_unix.to_string())?;
    line(&mut out, "reference", &receipt.provenance.reference)?;
    line(&mut out, "source", &receipt.provenance.source)?;
    line(&mut out, "builder", &receipt.provenance.builder)?;
    line(&mut out, "action", &receipt.provenance.action)?;
    line(&mut out, "output", &receipt.provenance.output)?;
    line(&mut out, "platform", &receipt.provenance.platform)?;
    line(&mut out, "sandbox", &receipt.provenance.sandbox)?;
    line(&mut out, "policy", &receipt.provenance.policy)?;
    line(&mut out, "signature_key", &receipt.signature.key_id)?;
    line(
        &mut out,
        "signature_algorithm",
        &receipt.signature.algorithm,
    )?;
    line(&mut out, "signature", &receipt.signature.sig_hex)?;
    Ok(out)
}

fn decode_cache_receipt(bytes: &[u8]) -> io::Result<CacheReceipt> {
    let text = String::from_utf8(bytes.to_vec())
        .map_err(|_| invalid("cache trust receipt is not UTF-8"))?;
    let mut lines = text.lines();
    if lines.next() != Some(CACHE_RECEIPT_MAGIC) {
        return Err(invalid("cache trust receipt has an unknown format"));
    }
    let mut fields = BTreeMap::new();
    for raw in lines {
        let (name, value) = raw
            .split_once('=')
            .ok_or_else(|| invalid("cache trust receipt has a malformed line"))?;
        validate_text(name, "cache trust receipt field")?;
        validate_text(value, "cache trust receipt value")?;
        if fields.insert(name.to_string(), value.to_string()).is_some() {
            return Err(invalid("cache trust receipt has a duplicate field"));
        }
    }
    let mut take = |name: &str| {
        fields
            .remove(name)
            .ok_or_else(|| invalid(&format!("cache trust receipt has no {name} field")))
    };
    let receipt = CacheReceipt {
        role: take("role")?,
        witness: take("witness")?,
        version: take("version")?
            .parse()
            .map_err(|_| invalid("cache trust receipt version is not an integer"))?,
        issued_unix: take("issued")?
            .parse()
            .map_err(|_| invalid("cache trust receipt issue time is not an integer"))?,
        expires_unix: take("expires")?
            .parse()
            .map_err(|_| invalid("cache trust receipt expiry is not an integer"))?,
        provenance: CacheProvenance {
            reference: take("reference")?,
            source: take("source")?,
            builder: take("builder")?,
            action: take("action")?,
            output: take("output")?,
            platform: take("platform")?,
            sandbox: take("sandbox")?,
            policy: take("policy")?,
        },
        signature: Signature {
            key_id: take("signature_key")?,
            algorithm: take("signature_algorithm")?,
            sig_hex: take("signature")?,
        },
    };
    if !fields.is_empty() {
        return Err(invalid("cache trust receipt has an unknown field"));
    }
    receipt
        .provenance
        .validate()
        .map_err(|error| invalid(&error.to_string()))?;
    Ok(receipt)
}

fn write_receipt(path: &Path, receipt: &CacheReceipt) -> io::Result<()> {
    let bytes = encode_cache_receipt(receipt)?.into_bytes();
    if let Ok(existing) = fs::symlink_metadata(path) {
        if existing.file_type().is_symlink() || !existing.is_file() {
            return Err(invalid("cache trust receipt is not a regular file"));
        }
        let old = decode_cache_receipt(&read_regular_bounded(path, MAX_INFO_BYTES)?)?;
        if old == *receipt {
            return Ok(());
        }
        if old.version >= receipt.version {
            return Err(invalid(
                "cache trust receipt already has conflicting metadata",
            ));
        }
        return atomic_replace(path, &bytes);
    }
    write_resumable(path, &bytes)
}

fn verify_cache_receipt(
    roots: &Roots,
    role: &str,
    expected: &StoreEntry,
    receipt: &CacheReceipt,
    key: &TrustKey,
) -> io::Result<()> {
    receipt.verify(key, &SystemTrustedClock).map_err(|error| {
        invalid(&format!(
            "{error}; discard the mirror and republish trusted metadata"
        ))
    })?;
    if receipt.role != role {
        return Err(invalid(&format!(
            "cache trust receipt role `{}` disagrees with `{role}`; discard the mirror",
            receipt.role
        )));
    }
    let expected_provenance = cache_provenance_for_entry(expected)?;
    if receipt.provenance != expected_provenance {
        return Err(invalid(
            "cache trust receipt provenance does not match the requested output; mix-and-match rejected",
        ));
    }
    let policy = format!("cache-witnesses/{role}.allow");
    if !is_cache_witness_allowed(&roots.root, role, &receipt.witness).map_err(io::Error::other)? {
        return Err(invalid(
            &crate::TrustRoot::TrustError::CacheReceiptInvalid {
                detail: format!(
                    "cache receipt witness '{}' rejected by trust policy '{}'",
                    receipt.witness, policy
                ),
            }
            .to_string(),
        ));
    }
    let store_name = format!("{}-{}", expected.envelope.output_hash, expected.id);
    let pin = cache_receipt_pin_path(roots, role, &store_name)?;
    crate::RuntimePolicy::with_lock(&roots.root, "cache-trust", || {
        ensure_parent(&pin)?;
        let digest = SHA256::sha256_hex(encode_cache_receipt(receipt)?.as_bytes());
        match read_cache_receipt_pin(&pin) {
            Ok(existing) => {
                let version = existing.version;
                if receipt.version < version {
                    return Err(invalid(&format!(
                        "cache trust metadata rollback rejected: current {version}, incoming {}; refresh or rebuild",
                        receipt.version
                    )));
                }
                let accepted = String::from_utf8(read_regular_bounded(&pin, 4096)?)
                    .map_err(|_| invalid("cache trust pin is not UTF-8"))?
                    .lines()
                    .find_map(|line| line.strip_prefix("digest=").map(str::to_string))
                    .ok_or_else(|| invalid("cache trust pin has no digest"))?;
                if receipt.version == version && accepted != digest {
                    return Err(invalid(
                        "cache trust metadata changed at the same version; mix-and-match rejected",
                    ));
                }
                if receipt.version == version {
                    return Ok(());
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        atomic_replace(
            &pin,
            format!(
                "version={}\nexpires={}\ndigest={digest}\n",
                receipt.version, receipt.expires_unix
            )
            .as_bytes(),
        )
    })
}

fn write_new_key(path: &Path) -> io::Result<()> {
    ensure_parent(path)?;
    let secret = os_random_bytes::<32>()?.to_vec();
    if fs::symlink_metadata(path).is_ok() {
        return Ok(());
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(&secret)?;
    file.sync_all()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = file.metadata()?.permissions();
        permissions.set_mode(0o600);
        fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

fn bindings_dir(roots: &Roots) -> PathBuf {
    roots.root.join("cache").join("bindings")
}

fn binding_path(roots: &Roots, role: &str) -> io::Result<PathBuf> {
    validate_component(role, "cache role")?;
    Ok(bindings_dir(roots).join(format!("{role}.conf")))
}

fn parse_endpoint(endpoint: &str) -> io::Result<CacheEndpoint> {
    validate_text(endpoint, "cache endpoint")?;
    if endpoint == "hangar://local" || endpoint == "hangar://" {
        return Ok(CacheEndpoint::Hangar);
    }
    if let Some(path) = endpoint.strip_prefix("file://") {
        let path = absolute_endpoint_path(path, "file cache endpoint")?;
        return Ok(CacheEndpoint::Local(path));
    }
    if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
        validate_http_base(endpoint)?;
        return Ok(CacheEndpoint::Http(
            endpoint.trim_end_matches('/').to_string(),
        ));
    }
    if endpoint.starts_with("ssh://") || endpoint.starts_with("ssh-ng://") {
        return parse_ssh_endpoint(endpoint);
    }
    if endpoint.starts_with("s3://") {
        validate_s3_uri(endpoint)?;
        return Ok(CacheEndpoint::S3(
            endpoint.trim_end_matches('/').to_string(),
        ));
    }
    if endpoint.starts_with("daemon://") || endpoint.starts_with("nix://") {
        if endpoint.contains('?') || endpoint.contains('#') {
            return Err(invalid(
                "Nix store endpoint cannot contain a query or fragment",
            ));
        }
        return Ok(CacheEndpoint::Nix(endpoint.to_string()));
    }
    let path = absolute_endpoint_path(endpoint, "cache mirror")?;
    Ok(CacheEndpoint::Local(path))
}

fn absolute_endpoint_path(raw: &str, label: &str) -> io::Result<PathBuf> {
    if raw.is_empty() || !Path::new(raw).is_absolute() {
        return Err(invalid(&format!("{label} must contain an absolute path")));
    }
    if raw.contains('?') || raw.contains('#') {
        return Err(invalid(&format!(
            "{label} cannot contain a query or fragment"
        )));
    }
    let path = PathBuf::from(raw);
    validate_path_components(&path)?;
    Ok(path)
}

fn parse_ssh_endpoint(endpoint: &str) -> io::Result<CacheEndpoint> {
    let (scheme, rest) = endpoint
        .split_once("://")
        .ok_or_else(|| invalid("SSH endpoint is missing its scheme"))?;
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    if authority.is_empty()
        || authority.starts_with('-')
        || !authority.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'@' | b':' | b'-')
        })
    {
        return Err(invalid("SSH endpoint has an unsafe host name"));
    }
    let root = if path.is_empty() {
        "/var/cache/jet".to_string()
    } else {
        format!("/{path}")
    };
    validate_remote_key(&root)?;
    if scheme != "ssh" && scheme != "ssh-ng" {
        return Err(invalid("unknown SSH endpoint scheme"));
    }
    Ok(CacheEndpoint::Ssh {
        target: authority.to_string(),
        root,
    })
}

fn validate_http_base(endpoint: &str) -> io::Result<()> {
    let (_, rest) = endpoint
        .split_once("://")
        .ok_or_else(|| invalid("HTTP endpoint is missing its scheme"))?;
    let authority = rest.split('/').next().unwrap_or_default();
    if authority.is_empty()
        || authority.starts_with('-')
        || authority.contains('@')
        || authority.contains('?')
        || authority.contains('#')
        || authority.chars().any(char::is_whitespace)
    {
        return Err(invalid("HTTP endpoint has an unsafe host name"));
    }
    if rest.split('/').any(|part| part == "..") {
        return Err(invalid("HTTP endpoint contains a parent path"));
    }
    Ok(())
}

fn validate_s3_uri(endpoint: &str) -> io::Result<()> {
    let rest = endpoint
        .strip_prefix("s3://")
        .ok_or_else(|| invalid("S3 endpoint is missing its scheme"))?;
    let bucket = rest.split('/').next().unwrap_or_default();
    if bucket.is_empty()
        || !bucket
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
        || rest.contains('?')
        || rest.contains('#')
        || rest.split('/').any(|part| part == "..")
    {
        return Err(invalid("S3 endpoint has an unsafe bucket or prefix"));
    }
    Ok(())
}

fn validate_remote_key(key: &str) -> io::Result<()> {
    if key.is_empty()
        || !key.starts_with('/')
        || key.contains('\0')
        || key.chars().any(char::is_whitespace)
        || key.split('/').any(|part| part == "..")
        || key.bytes().any(|byte| {
            matches!(
                byte,
                b'\'' | b'"' | b'`' | b'$' | b';' | b'&' | b'|' | b'<' | b'>'
            )
        })
    {
        return Err(invalid("remote cache path is unsafe"));
    }
    Ok(())
}

fn validate_relative_key(key: &str) -> io::Result<()> {
    if key.is_empty()
        || key.starts_with('/')
        || key
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_control() || matches!(byte, b'?' | b'#' | b'%'))
        || Path::new(key)
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
    {
        return Err(invalid("cache object key is unsafe"));
    }
    Ok(())
}

fn endpoint_key(endpoint: &CacheEndpoint, key: &str) -> io::Result<String> {
    validate_relative_key(key)?;
    match endpoint {
        CacheEndpoint::Http(base) => Ok(format!("{base}/{key}")),
        CacheEndpoint::S3(base) => Ok(format!("{base}/{key}")),
        CacheEndpoint::Ssh { root, .. } => {
            let path = format!("{}/{}", root.trim_end_matches('/'), key);
            validate_remote_key(&path)?;
            Ok(path)
        }
        CacheEndpoint::Local(root) => {
            let path = root.join(key);
            validate_path_components(&path)?;
            Ok(path.to_string_lossy().into_owned())
        }
        CacheEndpoint::Nix(_) | CacheEndpoint::Hangar => {
            Err(invalid("this endpoint does not expose cache object keys"))
        }
    }
}

fn endpoint_get(endpoint: &CacheEndpoint, key: &str, limit: u64) -> io::Result<Option<Vec<u8>>> {
    match endpoint {
        CacheEndpoint::Local(_) => {
            let path = PathBuf::from(endpoint_key(endpoint, key)?);
            match read_regular_bounded(&path, limit) {
                Ok(bytes) => Ok(Some(bytes)),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(error),
            }
        }
        CacheEndpoint::Http(_) => http_get(endpoint_key(endpoint, key)?, limit),
        CacheEndpoint::Ssh { target, .. } => ssh_get(target, &endpoint_key(endpoint, key)?, limit),
        CacheEndpoint::S3(_) => s3_get(endpoint_key(endpoint, key)?, limit),
        CacheEndpoint::Nix(_) | CacheEndpoint::Hangar => {
            Err(invalid("this endpoint has no cache object read operation"))
        }
    }
}

fn endpoint_put(endpoint: &CacheEndpoint, key: &str, bytes: &[u8]) -> io::Result<()> {
    let remote = endpoint_key(endpoint, key)?;
    match endpoint {
        CacheEndpoint::Local(root) => {
            ensure_directory(root)?;
            write_resumable(&PathBuf::from(remote), bytes)
        }
        CacheEndpoint::Http(_) => http_put(remote, bytes),
        CacheEndpoint::Ssh { target, .. } => {
            let parent = Path::new(&remote)
                .parent()
                .ok_or_else(|| invalid("SSH cache object has no parent"))?;
            ssh_mkdir(target, &parent.to_string_lossy())?;
            ssh_put(target, &remote, bytes)
        }
        CacheEndpoint::S3(_) => s3_put(remote, bytes),
        CacheEndpoint::Nix(_) | CacheEndpoint::Hangar => {
            Err(invalid("this endpoint has no cache object write operation"))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TransferProof {
    nix_nar_hash: Option<String>,
    signed_fingerprint: String,
    witness: Option<String>,
    receipt_version: Option<u64>,
    receipt_expires_unix: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NixTransfer {
    nar: Vec<u8>,
    nar_hash: String,
    nix_nar_hash: String,
    signed_fingerprint: String,
}

fn publish_endpoint(
    endpoint: &CacheEndpoint,
    info: &NarInfo,
    nar: &[u8],
    key: &TrustKey,
    _roots: &Roots,
    entry: &StoreEntry,
    role: &str,
) -> io::Result<TransferProof> {
    let caps = endpoint.capabilities();
    if !caps.write {
        return Err(invalid(&format!(
            "{} endpoint is read-only",
            endpoint.label()
        )));
    }
    match endpoint {
        CacheEndpoint::Local(root) => {
            let receipt = cache_receipt_for_publication(endpoint, role, entry, key)?;
            publish_local_resumable(root, info, nar, key, &receipt)?;
            Ok(TransferProof {
                nix_nar_hash: None,
                signed_fingerprint: fingerprint_for_key(key),
                witness: Some(receipt.witness.clone()),
                receipt_version: Some(receipt.version),
                receipt_expires_unix: Some(receipt.expires_unix),
            })
        }
        CacheEndpoint::Hangar => Err(invalid(
            "Hangar is the local source store and cannot publish cache objects",
        )),
        CacheEndpoint::Nix(uri) => {
            let stats = super::validate_nar(nar)?;
            require_nix_store_path(&entry.out)?;
            nix_copy(uri, "--to", &entry.out)?;
            prove_nix_transfer(uri, &entry.out, &stats.digest, stats.bytes, key)
        }
        CacheEndpoint::Http(_) | CacheEndpoint::Ssh { .. } | CacheEndpoint::S3(_) => {
            let signed = info.clone().signed(key)?;
            let signed_text = signed.to_text()?;
            let info_key = format!(
                "{}.narinfo",
                info.store_path
                    .rsplit('/')
                    .next()
                    .ok_or_else(|| invalid("narinfo store path has no name"))?
            );
            let receipt = cache_receipt_for_publication(endpoint, role, entry, key)?;
            let receipt_text = encode_cache_receipt(&receipt)?;
            let store_name = info_key
                .strip_suffix(".narinfo")
                .ok_or_else(|| invalid("narinfo key has no suffix"))?;
            let receipt_key = cache_receipt_key(store_name)?;
            publish_remote_atomic(
                endpoint,
                [
                    (info.url.as_str(), nar),
                    (&info_key, signed_text.as_bytes()),
                    (receipt_key.as_str(), receipt_text.as_bytes()),
                ],
            )?;
            Ok(TransferProof {
                nix_nar_hash: None,
                signed_fingerprint: fingerprint_for_info(&signed),
                witness: Some(receipt.witness.clone()),
                receipt_version: Some(receipt.version),
                receipt_expires_unix: Some(receipt.expires_unix),
            })
        }
    }
}

fn publish_remote_atomic<'a>(
    endpoint: &CacheEndpoint,
    objects: impl IntoIterator<Item = (&'a str, &'a [u8])>,
) -> io::Result<()> {
    let capabilities = endpoint.capabilities();
    if !capabilities.write {
        return Err(invalid(&format!(
            "{} cache endpoint does not provide write access",
            endpoint.label()
        )));
    }
    if !capabilities.promote {
        return Err(invalid(&format!(
            "{} cache endpoint has no atomic promotion support",
            endpoint.label()
        )));
    }
    for (key, bytes) in objects {
        let partial = format!("{key}.partial");
        write_remote_resumable(endpoint, &partial, bytes)?;
        let uploaded = endpoint_get(endpoint, &partial, bytes.len() as u64 + 1)?
            .ok_or_else(|| invalid("cache endpoint lost its staged object"))?;
        if uploaded != bytes {
            return Err(invalid(
                "cache endpoint staged bytes differ from the signed object",
            ));
        }
        promote_remote(endpoint, &partial, key, bytes)?;
    }
    Ok(())
}

fn write_remote_resumable(endpoint: &CacheEndpoint, key: &str, bytes: &[u8]) -> io::Result<()> {
    let offset = match endpoint_get(endpoint, key, bytes.len() as u64 + 1)? {
        Some(existing) if existing == bytes => return Ok(()),
        Some(existing) if bytes.starts_with(&existing) => existing.len(),
        Some(_) => 0,
        None => 0,
    };
    if offset == 0 {
        return endpoint_put(endpoint, key, bytes);
    }
    match endpoint {
        CacheEndpoint::Ssh { target, .. } => {
            ssh_append(target, &endpoint_key(endpoint, key)?, &bytes[offset..])
        }
        _ => endpoint_put(endpoint, key, bytes),
    }
}

fn promote_remote(
    endpoint: &CacheEndpoint,
    partial: &str,
    final_key: &str,
    bytes: &[u8],
) -> io::Result<()> {
    if let Some(existing) = endpoint_get(endpoint, final_key, bytes.len() as u64 + 1)? {
        if existing == bytes {
            return Ok(());
        }
        return Err(invalid(
            "cache endpoint already has conflicting final bytes",
        ));
    }
    match endpoint {
        CacheEndpoint::Ssh { target, .. } => {
            ssh_mkdir_for_key(endpoint, final_key)?;
            ssh_promote(
                target,
                &endpoint_key(endpoint, partial)?,
                &endpoint_key(endpoint, final_key)?,
            )
        }
        CacheEndpoint::S3(_) => s3_promote(
            endpoint_key(endpoint, partial)?,
            endpoint_key(endpoint, final_key)?,
        ),
        _ => Err(invalid(&format!(
            "{} cache endpoint cannot promote staged objects",
            endpoint.label()
        ))),
    }
}

fn verify_nix_endpoint(
    endpoint: &CacheEndpoint,
    entry: &StoreEntry,
    key: &TrustKey,
) -> io::Result<Option<NixTransfer>> {
    let CacheEndpoint::Nix(uri) = endpoint else {
        return Ok(None);
    };
    require_nix_store_path(&entry.out)?;
    nix_copy(uri, "--from", &entry.out)?;
    let actual = crate::Envelope::try_output_hash_of(&entry.out).map_err(io::Error::other)?;
    if actual != entry.envelope.output_hash {
        return Err(invalid(
            "Nix store output hash disagrees with the Hangar identity",
        ));
    }
    let (nar, stats) = super::write_nar(Path::new(&entry.out))?;
    let proof = prove_nix_transfer(uri, &entry.out, &stats.digest, stats.bytes, key)?;
    Ok(Some(NixTransfer {
        nar,
        nar_hash: stats.digest,
        nix_nar_hash: proof
            .nix_nar_hash
            .ok_or_else(|| invalid("Nix transfer did not return its NarHash"))?,
        signed_fingerprint: proof.signed_fingerprint,
    }))
}

fn verify_hangar_endpoint(
    endpoint: &CacheEndpoint,
    entry: &StoreEntry,
) -> io::Result<Option<Vec<u8>>> {
    if !matches!(endpoint, CacheEndpoint::Hangar) {
        return Ok(None);
    }
    let actual = crate::Envelope::try_output_hash_of(&entry.out).map_err(io::Error::other)?;
    if actual != entry.envelope.output_hash {
        return Err(invalid(
            "local Hangar output hash disagrees with its identity",
        ));
    }
    let (nar, _) = super::write_nar(Path::new(&entry.out))?;
    Ok(Some(nar))
}

fn substitute_hangar_endpoint(
    endpoint: &CacheEndpoint,
    entry: &StoreEntry,
    destination: &Path,
) -> io::Result<Option<Vec<u8>>> {
    if !matches!(endpoint, CacheEndpoint::Hangar) {
        return Ok(None);
    }
    let result = (|| {
        let actual = crate::Envelope::try_output_hash_of(&entry.out).map_err(io::Error::other)?;
        if actual != entry.envelope.output_hash {
            return Err(invalid(
                "local Hangar output hash disagrees with its identity",
            ));
        }
        copy_tree(Path::new(&entry.out), destination)?;
        super::seal_node(destination)?;
        let actual = crate::Envelope::try_output_hash_of(&destination.to_string_lossy())
            .map_err(io::Error::other)?;
        if actual != entry.envelope.output_hash {
            return Err(invalid(
                "local Hangar output hash disagrees with its identity",
            ));
        }
        let (nar, _) = super::write_nar(destination)?;
        Ok(nar)
    })();
    match result {
        Ok(nar) => Ok(Some(nar)),
        Err(error) => {
            let _ = remove_tree(destination);
            Err(error)
        }
    }
}

fn substitute_nix_endpoint(
    endpoint: &CacheEndpoint,
    entry: &StoreEntry,
    destination: &Path,
    key: &TrustKey,
) -> io::Result<Option<NixTransfer>> {
    if !matches!(endpoint, CacheEndpoint::Nix(_)) {
        return Ok(None);
    }
    let result = (|| {
        let transfer = verify_nix_endpoint(endpoint, entry, key)?
            .ok_or_else(|| invalid("Nix substitution was called for a non-Nix endpoint"))?;
        super::read_nar(&transfer.nar, destination)?;
        super::seal_node(destination)?;
        let actual = crate::Envelope::try_output_hash_of(&destination.to_string_lossy())
            .map_err(io::Error::other)?;
        if actual != entry.envelope.output_hash {
            return Err(invalid(
                "Nix store output hash disagrees with the Hangar identity",
            ));
        }
        let (_, stats) = super::write_nar(destination)?;
        if stats.digest != transfer.nar_hash {
            return Err(invalid("Nix substitution changed the verified NAR bytes"));
        }
        Ok(transfer)
    })();
    match result {
        Ok(transfer) => Ok(Some(transfer)),
        Err(error) => {
            let _ = remove_tree(destination);
            Err(error)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NixPathInfo {
    nar_hash: String,
    nar_size: Option<u64>,
    signed_fingerprint: String,
}

fn prove_nix_transfer(
    uri: &str,
    path: &str,
    expected_nar_hash: &str,
    expected_bytes: u64,
    key: &TrustKey,
) -> io::Result<TransferProof> {
    require_nix_store_path(path)?;
    let info = nix_path_info(uri, path)?;
    nix_verify_path(uri, path)?;
    let normalized = nix_nar_hash_to_jet(&info.nar_hash)?;
    if normalized != expected_nar_hash {
        return Err(invalid(&format!(
            "Nix NarHash {} disagrees with Jetpack digest {expected_nar_hash}",
            info.nar_hash
        )));
    }
    if info.nar_size.is_some_and(|size| size != expected_bytes) {
        return Err(invalid("Nix NarSize disagrees with the canonical NAR size"));
    }
    Ok(TransferProof {
        nix_nar_hash: Some(info.nar_hash.clone()),
        signed_fingerprint: nix_admission_fingerprint(uri, path, &info, key),
        witness: None,
        receipt_version: None,
        receipt_expires_unix: None,
    })
}

/// Jetpack and Nix use different trust domains. Nix verifies its own signed
/// path against the host's configured trusted keys; this HMAC binds that
/// verified Nix fact to the cache binding that admitted it. The HMAC tag is
/// an admission proof, not a replacement for Nix's signature check.
fn nix_admission_fingerprint(uri: &str, path: &str, info: &NixPathInfo, key: &TrustKey) -> String {
    let message = format!(
        "jetpack-nix-transfer-v1\n{uri}\n{path}\n{}\n{}",
        info.nar_hash, info.signed_fingerprint
    );
    let signature = key.sign(message.as_bytes());
    format!(
        "{};jetpack:{}:{}",
        info.signed_fingerprint, signature.key_id, signature.sig_hex
    )
}

fn nix_path_info(uri: &str, path: &str) -> io::Result<NixPathInfo> {
    let child = Command::new("nix")
        .args(["path-info", "--json", "--store", uri, path])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            invalid(&format!(
                "Nix store metadata adapter could not start: {error}"
            ))
        })?;
    let (status, stdout, stderr) = run_bounded_output(child, MAX_INFO_BYTES, "Nix store metadata")?;
    if !status.success() {
        return Err(invalid(&format!(
            "Nix store metadata rejected path {path}: {}",
            bounded_stderr(&stderr)
        )));
    }
    let text = String::from_utf8(stdout).map_err(|_| invalid("Nix store metadata is not UTF-8"))?;
    let parsed = crate::JSON::parse(&text).map_err(io::Error::other)?;
    let (reported_path, object) = match &parsed {
        crate::JSON::JSONValue::Array(rows) if rows.len() == 1 => rows[0]
            .as_object()
            .map(|object| {
                let path = object
                    .get("path")
                    .and_then(|value| value.as_str().ok())
                    .unwrap_or(path);
                (path, object)
            })
            .map_err(io::Error::other)?,
        crate::JSON::JSONValue::Object(object) if object.contains_key("path") => (
            object
                .get("path")
                .ok_or_else(|| invalid("Nix store metadata has no path"))?
                .as_str()
                .map_err(io::Error::other)?,
            object,
        ),
        crate::JSON::JSONValue::Object(object) if object.len() == 1 => {
            let (path, value) = object.iter().next().expect("one-entry Nix metadata object");
            (path.as_str(), value.as_object().map_err(io::Error::other)?)
        }
        crate::JSON::JSONValue::Array(_) => {
            return Err(invalid("Nix store metadata returned more than one path"));
        }
        _ => return Err(invalid("Nix store metadata is not an object")),
    };
    if reported_path != path {
        return Err(invalid(
            "Nix store metadata path disagrees with the requested path",
        ));
    }
    let nar_hash = object
        .get("narHash")
        .ok_or_else(|| invalid("Nix store metadata has no NarHash"))?
        .as_str()
        .map_err(io::Error::other)?
        .to_string();
    let nar_size = object
        .get("narSize")
        .map(|value| {
            let size = match value {
                crate::JSON::JSONValue::Number(size) if *size >= 0 => *size as u64,
                crate::JSON::JSONValue::Flt(size)
                    if size.is_finite() && *size >= 0.0 && size.fract() == 0.0 =>
                {
                    *size as u64
                }
                _ => return Err(invalid("Nix store metadata NarSize is not an integer")),
            };
            Ok(size)
        })
        .transpose()?;
    let signatures = match object.get("signatures") {
        Some(value) => value
            .as_array()
            .map_err(io::Error::other)?
            .iter()
            .map(|value| {
                let signature = value.as_str().map_err(io::Error::other)?;
                if signature.is_empty()
                    || signature.len() > 4096
                    || signature
                        .bytes()
                        .any(|byte| byte == 0 || byte.is_ascii_control())
                {
                    return Err(invalid(
                        "Nix store metadata has an invalid signature fingerprint",
                    ));
                }
                Ok(signature.to_string())
            })
            .collect::<io::Result<Vec<_>>>()?,
        None => Vec::new(),
    };
    if signatures.is_empty() {
        return Err(invalid("Nix store metadata has no signed fingerprint"));
    }
    let signed_fingerprint = signatures.join(",");
    Ok(NixPathInfo {
        nar_hash,
        nar_size,
        signed_fingerprint,
    })
}

fn nix_nar_hash_to_jet(value: &str) -> io::Result<String> {
    super::normalize_nar_hash(value)
}

fn require_nix_store_path(path: &str) -> io::Result<()> {
    let path = Path::new(path);
    if !path.starts_with("/nix/store")
        || path == Path::new("/nix/store")
        || path.components().any(|component| {
            matches!(
                component,
                Component::CurDir | Component::ParentDir | Component::Prefix(_)
            )
        })
    {
        return Err(invalid(
            "Nix cache transfer requires an existing canonical /nix/store path; Hangar outputs cannot be relocated into Nix",
        ));
    }
    Ok(())
}

fn nix_verify_path(uri: &str, path: &str) -> io::Result<()> {
    require_nix_store_path(path)?;
    let child = Command::new("nix")
        .args([
            "store",
            "verify",
            "--store",
            uri,
            "--sigs-needed",
            "1",
            path,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| invalid(&format!("Nix trust verifier could not start: {error}")))?;
    let (status, _, stderr) = run_bounded_output(child, 4096, "Nix trust verifier response")?;
    if status.success() {
        Ok(())
    } else {
        Err(invalid(&format!(
            "Nix trust verifier rejected path {path}: {}",
            bounded_stderr(&stderr)
        )))
    }
}

fn nix_copy(uri: &str, direction: &str, path: &str) -> io::Result<()> {
    require_nix_store_path(path)?;
    if path.is_empty() || path.starts_with('-') {
        return Err(invalid("Nix store transfer has no path"));
    }
    if !matches!(direction, "--to" | "--from") {
        return Err(invalid("Nix store transfer has an invalid direction"));
    }
    let child = Command::new("nix")
        .args(["copy", direction, uri, path])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| invalid(&format!("Nix store adapter could not start: {error}")))?;
    let (status, _, stderr) = run_bounded_output(child, 4096, "Nix store response")?;
    if status.success() {
        Ok(())
    } else {
        Err(invalid(&format!(
            "Nix store adapter rejected path {path}: {}",
            bounded_stderr(&stderr)
        )))
    }
}

fn copy_tree(source: &Path, destination: &Path) -> io::Result<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    super::copy_snapshot_node(source, destination, &mut BTreeMap::new())
}

fn http_get(url: String, limit: u64) -> io::Result<Option<Vec<u8>>> {
    let response = jet_net::get_stream(&url, std::time::Duration::from_secs(120))
        .map_err(|error| invalid(&format!("HTTP cache GET failed: {error}")))?;
    let status = response.status();
    if status == 404 || status == 410 {
        return Ok(None);
    }
    if !(200..300).contains(&status) {
        return Err(invalid(&format!(
            "HTTP cache GET failed with status {status}"
        )));
    }
    let mut body = Vec::new();
    response
        .take(limit.saturating_add(1))
        .read_to_end(&mut body)
        .map_err(|error| invalid(&format!("HTTP cache response could not be read: {error}")))?;
    if body.len() as u64 > limit {
        return Err(invalid("HTTP cache response exceeded its bound"));
    }
    Ok(Some(body))
}

fn http_put(url: String, bytes: &[u8]) -> io::Result<()> {
    let mut child = Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--netrc-optional",
            "--compressed",
            "--max-time",
            "120",
            "--request",
            "PUT",
            "--data-binary",
            "@-",
            "--output",
            null_device(),
            "--write-out",
            "\n%{http_code}",
            &url,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| invalid(&format!("HTTP cache adapter could not start: {error}")))?;
    child
        .stdin
        .take()
        .ok_or_else(|| invalid("HTTP cache adapter has no input"))?
        .write_all(bytes)?;
    let (status_result, stdout, stderr) = run_bounded_output(child, 64, "HTTP cache response")?;
    let status = stdout
        .rsplit(|byte| *byte == b'\n')
        .next()
        .and_then(|value| std::str::from_utf8(value).ok())
        .and_then(|value| value.trim().parse::<u16>().ok())
        .unwrap_or(0);
    if status_result.success() && (200..300).contains(&status) {
        Ok(())
    } else {
        Err(invalid(&format!(
            "HTTP cache PUT failed with status {status}: {}",
            bounded_stderr(&stderr)
        )))
    }
}

fn ssh_get(target: &str, path: &str, limit: u64) -> io::Result<Option<Vec<u8>>> {
    let child = Command::new("ssh")
        .args([target, "cat", "--", path])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| invalid(&format!("SSH cache adapter could not start: {error}")))?;
    let (status, stdout, stderr) = run_bounded_output(child, limit, "SSH cache response")?;
    if !status.success() {
        let error = bounded_stderr(&stderr);
        if error.contains("No such file") || error.contains("not found") {
            return Ok(None);
        }
        return Err(invalid(&format!("SSH cache GET failed: {error}")));
    }
    Ok(Some(stdout))
}

fn ssh_mkdir(target: &str, root: &str) -> io::Result<()> {
    let child = Command::new("ssh")
        .args([target, "mkdir", "-p", "--", root])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| invalid(&format!("SSH cache adapter could not start: {error}")))?;
    let (status, _, stderr) = run_bounded_output(child, 4096, "SSH cache response")?;
    if status.success() {
        Ok(())
    } else {
        Err(invalid(&format!(
            "SSH cache mkdir failed: {}",
            bounded_stderr(&stderr)
        )))
    }
}

fn ssh_mkdir_for_key(endpoint: &CacheEndpoint, key: &str) -> io::Result<()> {
    let endpoint_key = endpoint_key(endpoint, key)?;
    let parent = Path::new(&endpoint_key)
        .parent()
        .ok_or_else(|| invalid("SSH cache object has no parent"))?;
    let CacheEndpoint::Ssh { target, .. } = endpoint else {
        return Err(invalid("SSH directory creation needs an SSH endpoint"));
    };
    ssh_mkdir(target, &parent.to_string_lossy())
}

fn ssh_put(target: &str, path: &str, bytes: &[u8]) -> io::Result<()> {
    // `path` has already passed `validate_remote_key`, so the remote shell
    // redirection cannot introduce a second command. Suppress `tee`'s copy to
    // stdout; otherwise a successful upload of a large object trips the local
    // bounded response guard.
    let command = format!("tee -- {path} >/dev/null");
    let mut child = Command::new("ssh")
        .args([target, command.as_str()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| invalid(&format!("SSH cache adapter could not start: {error}")))?;
    child
        .stdin
        .take()
        .ok_or_else(|| invalid("SSH cache adapter has no input"))?
        .write_all(bytes)?;
    let (status, _, stderr) = run_bounded_output(child, 1, "SSH cache response")?;
    if status.success() {
        Ok(())
    } else {
        Err(invalid(&format!(
            "SSH cache PUT failed: {}",
            bounded_stderr(&stderr)
        )))
    }
}

fn ssh_append(target: &str, path: &str, bytes: &[u8]) -> io::Result<()> {
    validate_remote_key(path)?;
    let command = format!("tee -a -- {path} >/dev/null");
    let mut child = Command::new("ssh")
        .args([target, command.as_str()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| invalid(&format!("SSH cache adapter could not start: {error}")))?;
    child
        .stdin
        .take()
        .ok_or_else(|| invalid("SSH cache adapter has no input"))?
        .write_all(bytes)?;
    let (status, _, stderr) = run_bounded_output(child, 1, "SSH cache response")?;
    if status.success() {
        Ok(())
    } else {
        Err(invalid(&format!(
            "SSH cache append failed: {}",
            bounded_stderr(&stderr)
        )))
    }
}

fn ssh_promote(target: &str, partial: &str, final_path: &str) -> io::Result<()> {
    validate_remote_key(partial)?;
    validate_remote_key(final_path)?;
    let command = format!("mv -n -- {partial} {final_path}");
    let child = Command::new("ssh")
        .args([target, command.as_str()])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| invalid(&format!("SSH cache adapter could not start: {error}")))?;
    let (status, _, stderr) = run_bounded_output(child, 4096, "SSH cache response")?;
    if status.success() {
        Ok(())
    } else {
        Err(invalid(&format!(
            "SSH cache promotion failed: {}",
            bounded_stderr(&stderr)
        )))
    }
}

fn s3_get(uri: String, limit: u64) -> io::Result<Option<Vec<u8>>> {
    let child = Command::new("aws")
        .args(["s3", "cp", &uri, "-", "--only-show-errors"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| invalid(&format!("S3 cache adapter could not start: {error}")))?;
    let (status, stdout, stderr) = run_bounded_output(child, limit, "S3 cache response")?;
    if !status.success() {
        let error = bounded_stderr(&stderr);
        if error.contains("404") || error.contains("NoSuchKey") || error.contains("Not Found") {
            return Ok(None);
        }
        return Err(invalid(&format!("S3 cache GET failed: {error}")));
    }
    Ok(Some(stdout))
}

fn s3_put(uri: String, bytes: &[u8]) -> io::Result<()> {
    let mut child = Command::new("aws")
        .args(["s3", "cp", "-", &uri, "--only-show-errors"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| invalid(&format!("S3 cache adapter could not start: {error}")))?;
    child
        .stdin
        .take()
        .ok_or_else(|| invalid("S3 cache adapter has no input"))?
        .write_all(bytes)?;
    let (status, _, stderr) = run_bounded_output(child, 4096, "S3 cache response")?;
    if status.success() {
        Ok(())
    } else {
        Err(invalid(&format!(
            "S3 cache PUT failed: {}",
            bounded_stderr(&stderr)
        )))
    }
}

fn s3_promote(partial: String, final_uri: String) -> io::Result<()> {
    let child = Command::new("aws")
        .args(["s3", "mv", &partial, &final_uri, "--only-show-errors"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| invalid(&format!("S3 cache adapter could not start: {error}")))?;
    let (status, _, stderr) = run_bounded_output(child, 4096, "S3 cache response")?;
    if status.success() {
        Ok(())
    } else {
        Err(invalid(&format!(
            "S3 cache promotion failed: {}",
            bounded_stderr(&stderr)
        )))
    }
}

fn bounded_stderr(bytes: &[u8]) -> String {
    String::from_utf8_lossy(&bytes[..bytes.len().min(4096)])
        .trim()
        .to_string()
}

/// Run a host transport with a hard stdout bound. The child is killed as soon
/// as the bound is crossed, while stderr is drained on a helper thread so a
/// noisy failed transport cannot deadlock the transfer. The caller receives
/// only bounded bytes and a bounded diagnostic prefix.
fn run_bounded_output(
    mut child: Child,
    limit: u64,
    label: &str,
) -> io::Result<(std::process::ExitStatus, Vec<u8>, Vec<u8>)> {
    let stdout = child.stdout.take().ok_or_else(|| {
        let _ = child.kill();
        let _ = child.wait();
        invalid(&format!("{label} has no output pipe"))
    })?;
    let stderr_thread = child.stderr.take().map(|mut stderr| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let mut buffer = [0u8; 4096];
            loop {
                match stderr.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => {
                        if bytes.len() < 4096 {
                            let keep = read.min(4096 - bytes.len());
                            bytes.extend_from_slice(&buffer[..keep]);
                        }
                    }
                    Err(_) => break,
                }
            }
            bytes
        })
    });
    let max = usize::try_from(limit.saturating_add(1))
        .map_err(|_| invalid(&format!("{label} bound is too large")))?;
    let mut bytes = Vec::new();
    stdout.take(max as u64).read_to_end(&mut bytes)?;
    if bytes.len() >= max {
        let _ = child.kill();
        let _ = child.wait();
        if let Some(thread) = stderr_thread {
            let _ = thread.join();
        }
        return Err(invalid(&format!("{label} exceeded its bound")));
    }
    let status = child.wait()?;
    let stderr = stderr_thread
        .map(|thread| thread.join().unwrap_or_default())
        .unwrap_or_default();
    Ok((status, bytes, stderr))
}

fn null_device() -> &'static str {
    if cfg!(windows) {
        "NUL"
    } else {
        "/dev/null"
    }
}

fn publish_local_resumable(
    root: &Path,
    info: &NarInfo,
    nar: &[u8],
    key: &TrustKey,
    receipt: &CacheReceipt,
) -> io::Result<()> {
    crate::RuntimePolicy::with_lock(root, "cache-publish", || {
        let signed = info.clone().signed(key)?;
        if signed.nar_size != nar.len() as u64 || !super::nar_hash_matches(&signed.nar_hash, nar) {
            return Err(invalid("NAR bytes do not match signed narinfo"));
        }
        ensure_directory(root)?;
        let nar_path = root.join(&signed.url);
        validate_path_components(&nar_path)?;
        let store_name = signed
            .store_path
            .rsplit('/')
            .next()
            .ok_or_else(|| invalid("narinfo store path has no name"))?;
        let info_path = root.join(format!("{store_name}.narinfo"));
        validate_path_components(&info_path)?;
        let receipt_path = root.join(cache_receipt_key(store_name)?);
        ensure_parent(&receipt_path)?;
        ensure_directory(
            receipt_path
                .parent()
                .ok_or_else(|| invalid("cache receipt has no parent"))?,
        )?;
        write_resumable(&nar_path, nar)?;
        write_resumable(&info_path, signed.to_text()?.as_bytes())?;
        write_receipt(&receipt_path, receipt)?;
        clear_negative_hint(root, store_name)?;
        Ok(())
    })
}

fn negative_hint_path(root: &Path, identity: &str) -> PathBuf {
    let digest = SHA256::sha256_hex(identity.as_bytes());
    root.join(".jet-negative").join(format!("{digest}.until"))
}

fn negative_hint_fresh(root: &Path, identity: &str) -> bool {
    let path = negative_hint_path(root, identity);
    let Ok(metadata) = fs::symlink_metadata(&path) else {
        return false;
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > 64 {
        return false;
    }
    let Ok(text) = read_regular_bounded(&path, 64).and_then(|bytes| {
        String::from_utf8(bytes).map_err(|_| invalid("negative cache hint is not UTF-8"))
    }) else {
        return false;
    };
    text.trim()
        .parse::<u64>()
        .ok()
        .is_some_and(|until| until > now_seconds())
}

fn write_negative_hint(root: &Path, identity: &str) -> io::Result<()> {
    crate::RuntimePolicy::with_lock(root, "cache-negative", || {
        let path = negative_hint_path(root, identity);
        ensure_parent(&path)?;
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(invalid("negative cache hint is not a regular file"));
            }
            fs::remove_file(&path)?;
        }
        let until = now_seconds().saturating_add(NEGATIVE_CACHE_TTL_SECS);
        write_resumable(&path, until.to_string().as_bytes())
    })
}

fn clear_negative_hint(root: &Path, identity: &str) -> io::Result<()> {
    let path = negative_hint_path(root, identity);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(invalid("negative cache hint is not a regular file"))
        }
        Ok(_) => {
            fs::remove_file(&path)?;
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Write an immutable object through a resumable private partial. A completed
/// object is never replaced; a matching object is accepted and a conflicting
/// object is rejected.
fn write_resumable(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(invalid("cache object is not a regular file"));
        }
        return if read_regular_bounded(path, bytes.len() as u64)? == bytes {
            Ok(())
        } else {
            Err(invalid("cache object already has conflicting bytes"))
        };
    }
    ensure_parent(path)?;
    let partial = path.with_extension("partial");
    let mut offset = 0usize;
    if let Ok(metadata) = fs::symlink_metadata(&partial) {
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() > bytes.len() as u64
        {
            let _ = fs::remove_file(&partial);
        } else {
            let existing = read_regular_bounded(&partial, bytes.len() as u64)?;
            if !bytes.starts_with(&existing) {
                let _ = fs::remove_file(&partial);
            } else {
                offset = existing.len();
            }
        }
    }
    if offset == 0 && fs::symlink_metadata(&partial).is_err() {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial)?;
    }
    let mut file = fs::OpenOptions::new().append(true).open(&partial)?;
    file.write_all(&bytes[offset..])?;
    file.sync_all()?;
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(invalid("cache object appeared as a non-regular file"));
        }
        let same = read_regular_bounded(path, bytes.len() as u64)
            .map(|existing| existing == bytes)
            .unwrap_or(false);
        return if same {
            let _ = fs::remove_file(&partial);
            Ok(())
        } else {
            let _ = fs::remove_file(&partial);
            Err(invalid(
                "cache object was concurrently published with different bytes",
            ))
        };
    }
    match fs::hard_link(&partial, path) {
        Ok(()) => {
            let _ = fs::remove_file(&partial);
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let same = read_regular_bounded(path, bytes.len() as u64)
                .map(|existing| existing == bytes)
                .unwrap_or(false);
            let _ = fs::remove_file(&partial);
            if same {
                Ok(())
            } else {
                Err(invalid(
                    "cache object was concurrently published with different bytes",
                ))
            }
        }
        Err(error) => {
            let _ = fs::remove_file(&partial);
            Err(error)
        }
    }
}

fn ensure_directory(path: &Path) -> io::Result<()> {
    validate_path_components(path)?;
    fs::create_dir_all(path)?;
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(invalid("cache mirror is not a real directory"));
    }
    Ok(())
}

fn validate_endpoint(endpoint: &str) -> io::Result<()> {
    let parsed = parse_endpoint(endpoint)?;
    let caps = parsed.capabilities();
    if !caps.read {
        return Err(invalid("cache endpoint does not provide read access"));
    }
    Ok(())
}

fn validate_endpoint_binding(
    binding: &CacheBinding,
    endpoint: &CacheEndpoint,
    write: bool,
) -> io::Result<()> {
    let caps = endpoint.capabilities();
    if !caps.read {
        return Err(invalid(&format!(
            "{} cache endpoint does not provide read access",
            endpoint.label()
        )));
    }
    if write && !caps.write {
        return Err(invalid(&format!(
            "{} cache endpoint does not provide write access",
            endpoint.label()
        )));
    }
    if !caps.trust {
        return Err(invalid(&format!(
            "{} cache endpoint does not provide trust support",
            endpoint.label()
        )));
    }
    if caps.remote_execute {
        return Err(invalid(&format!(
            "{} cache endpoint exposes forbidden remote execution",
            endpoint.label()
        )));
    }
    if binding.credential_provider.is_some() && !caps.credential {
        return Err(invalid(&format!(
            "{} cache endpoint cannot consume a typed credential provider",
            endpoint.label()
        )));
    }
    if binding.credential_provider.is_some() && caps.credential {
        return Err(invalid(&format!(
            "{} cache endpoint has no typed credential adapter; refusing ambient credentials",
            endpoint.label()
        )));
    }
    if caps.credential_required && binding.credential_provider.is_none() {
        return Err(invalid(&format!(
            "{} cache endpoint needs an explicit typed credential provider",
            endpoint.label()
        )));
    }
    Ok(())
}

fn validate_component(value: &str, label: &str) -> io::Result<()> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(invalid(&format!("{label} is not a safe name")));
    }
    Ok(())
}

fn validate_text(value: &str, label: &str) -> io::Result<()> {
    if value
        .bytes()
        .any(|byte| byte == 0 || byte == b'\n' || byte == b'\r')
    {
        return Err(invalid(&format!("{label} contains a line break")));
    }
    Ok(())
}

fn validate_path_components(path: &Path) -> io::Result<()> {
    if !path.is_absolute() {
        return Err(invalid("cache path must be absolute"));
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(Path::new(std::path::MAIN_SEPARATOR_STR)),
            Component::Normal(value) => {
                current.push(value);
                if let Ok(metadata) = fs::symlink_metadata(&current) {
                    if metadata.file_type().is_symlink() {
                        return Err(invalid("cache path cannot traverse a symlink"));
                    }
                }
            }
            Component::CurDir | Component::ParentDir => {
                return Err(invalid("cache path contains an unsafe component"));
            }
        }
    }
    Ok(())
}

fn absolutize_host_path(path: PathBuf) -> io::Result<PathBuf> {
    if path.is_absolute() {
        validate_path_components(&path)?;
        return Ok(path);
    }
    let current = std::env::current_dir()?;
    let absolute = current.join(path);
    validate_path_components(&absolute)?;
    Ok(absolute)
}

fn ensure_parent(path: &Path) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid("cache path has no parent"))?;
    validate_path_components(parent)?;
    fs::create_dir_all(parent)?;
    Ok(())
}

fn atomic_replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(invalid("cache binding path is not a regular file"));
        }
    }
    let partial = path.with_extension(format!(
        "partial-{}-{}",
        std::process::id(),
        unique_suffix()
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        #[cfg(windows)]
        {
            fs::remove_file(path)?;
        }
        fs::rename(&partial, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result
}

fn read_regular_bounded(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let limit = usize::try_from(limit).map_err(|_| invalid("cache input bound is too large"))?;
    super::Nar::read_bounded(path, limit)
}

fn remove_tree(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        fs::remove_file(path)
    } else {
        make_tree_writable_for_removal(path)?;
        fs::remove_dir_all(path)
    }
}

fn line(output: &mut String, name: &str, value: &str) -> io::Result<()> {
    validate_text(name, "cache binding field")?;
    validate_text(value, "cache binding value")?;
    output.push_str(name);
    output.push('=');
    output.push_str(value);
    output.push('\n');
    Ok(())
}

fn unique_suffix() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{}-{now}", std::process::id())
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nix_sri_hash_is_normalized_to_the_jetpack_digest() {
        assert_eq!(
            nix_nar_hash_to_jet("sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=").unwrap(),
            format!("sha256:{}", "00".repeat(32))
        );
    }

    #[test]
    fn cache_credentials_cannot_be_attached_to_a_non_credential_endpoint() {
        let binding = CacheBinding {
            role: "public".to_string(),
            mirrors: vec!["file:///tmp/jet-cache".to_string()],
            trust_key: PathBuf::from("/tmp/cache.key"),
            credential_provider: Some("host-keychain".to_string()),
            allow_write: false,
        };
        assert!(binding.validate().is_err());
    }

    #[test]
    fn cache_write_errors_use_access_language() {
        let binding = CacheBinding {
            role: "public".to_string(),
            mirrors: vec!["https://cache.example/jet".to_string()],
            trust_key: PathBuf::from("/tmp/cache.key"),
            credential_provider: None,
            allow_write: true,
        };
        let error = binding.validate().unwrap_err().to_string();
        assert!(error.contains("write access"), "{error}");
        assert!(!error.contains("capability"), "{error}");
    }

    #[test]
    fn remote_cache_endpoints_never_grant_remote_execution() {
        for endpoint in [
            parse_endpoint("https://cache.example/jet").unwrap(),
            parse_endpoint("ssh://builder.example/var/cache/jet").unwrap(),
            parse_endpoint("s3://cache.example/jet").unwrap(),
            parse_endpoint("daemon://").unwrap(),
        ] {
            assert!(!endpoint.capabilities().remote_execute);
        }
    }

    #[test]
    fn nix_cache_rejects_hangar_path_relocation() {
        assert!(require_nix_store_path("/nix/store/abcd-package").is_ok());
        assert!(require_nix_store_path("/tmp/jet-hangar/abcd-package").is_err());
        assert!(require_nix_store_path("/nix/store/../tmp/abcd-package").is_err());
    }

    #[test]
    fn nix_store_transfer_uses_typed_capabilities_and_rejects_ambient_credentials() {
        let nix = parse_endpoint("daemon://").unwrap();
        let capabilities = nix.capabilities();
        assert!(capabilities.read);
        assert!(capabilities.write);
        assert!(capabilities.trust);
        assert!(!capabilities.promote);
        assert!(!capabilities.remote_execute);
        let host_bound = CacheBinding {
            role: "remote".to_string(),
            mirrors: vec!["daemon://".to_string()],
            trust_key: PathBuf::from("/tmp/cache.key"),
            credential_provider: None,
            allow_write: false,
        };
        assert!(host_bound.validate().is_ok());
        let binding = CacheBinding {
            role: "remote".to_string(),
            mirrors: vec!["daemon://".to_string()],
            trust_key: PathBuf::from("/tmp/cache.key"),
            credential_provider: Some("host-keychain".to_string()),
            allow_write: false,
        };
        let error = binding.validate().unwrap_err();
        assert!(error.to_string().contains("typed credential"), "{error}");
    }

    #[test]
    fn malformed_cache_binding_is_not_treated_as_unbound() {
        let root = std::env::temp_dir().join(format!(
            "jetpack-cache-binding-{}-{}",
            std::process::id(),
            now_seconds()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("cache/bindings")).unwrap();
        fs::write(
            root.join("cache/bindings/public.conf"),
            "not a cache binding\n",
        )
        .unwrap();
        let roots = Roots {
            root: root.clone(),
            dev_mode: false,
        };
        let error = list_cache_bindings(&roots).unwrap_err();
        assert!(error.to_string().contains("cache binding"), "{error}");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cache_binding_missing_trust_key_fails_before_local_fallback() {
        let root = std::env::temp_dir().join(format!(
            "jetpack-cache-key-{}-{}",
            std::process::id(),
            now_seconds()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("mirror")).unwrap();
        let roots = Roots {
            root: root.clone(),
            dev_mode: false,
        };
        bind_cache(
            &roots,
            "public",
            vec![format!("file://{}", root.join("mirror").display())],
            None,
            None,
            false,
        )
        .unwrap();
        fs::remove_file(root.join("trust/cache-public.key")).unwrap();

        let error = list_cache_bindings(&roots).unwrap_err();
        assert!(error.to_string().contains("cache trust key"), "{error}");
        fs::remove_dir_all(root).unwrap();
    }
}
