//! Host-owned binary-cache bindings and the signed NAR transfer boundary.
//!
//! Workspace policy may request cache roles. This module owns the host-side
//! binding that maps a role to ordered mirrors, trust material, and an
//! optional typed credential-provider label. Secrets never enter this file,
//! URLs, argv, locks, or logs. Endpoint adapters exchange the same signed
//! narinfo and canonical NAR bytes, and a capability failure is reported
//! before an object is made locally usable.

use super::{NarInfo, Roots, StoreEntry};
use crate::TrustRoot::TrustKey;
use crate::SHA256;
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

const BINDING_MAGIC: &str = "jet-cache-bind-v1";
const MAX_BINDING_BYTES: u64 = 1024 * 1024;
const MAX_INFO_BYTES: u64 = 1024 * 1024;
const NEGATIVE_CACHE_TTL_SECS: u64 = 60;

struct CacheArtifact {
    info: NarInfo,
    nar: Vec<u8>,
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
            Self::Nix(_) => EndpointCapabilities {
                read: true,
                write: true,
                promote: true,
                remote_execute: false,
                trust: true,
                credential: true,
                credential_required: true,
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
    pub credential_provider: Option<String>,
    pub bytes: u64,
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
        line(
            &mut out,
            "key",
            &self.trust_key.to_string_lossy(),
        )?;
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
                write_new_key(&binding.trust_key, role)?;
            }
            Err(error) => return Err(error),
        }
    }
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
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > MAX_BINDING_BYTES {
        return Err(invalid("cache binding is not a regular file within its limit"));
    }
    let text = String::from_utf8(read_regular_bounded(&path, MAX_BINDING_BYTES)?)
        .map_err(|_| invalid("cache binding is not UTF-8"))?;
    let binding = CacheBinding::decode(&text)?;
    if binding.role != role {
        return Err(invalid("cache binding role disagrees with its file name"));
    }
    Ok(binding)
}

pub fn list_cache_bindings(roots: &Roots) -> io::Result<Vec<CacheBinding>> {
    let dir = bindings_dir(roots);
    let Ok(entries) = fs::read_dir(&dir) else {
        return Ok(Vec::new());
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
            return Err(invalid("cache binding directory contains a non-regular entry"));
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
        bindings.push(binding);
    }
    Ok(bindings)
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
        return Err(invalid("cache binding is read-only; publishing needs a write grant"));
    }
    let entry = select_entry(roots, target)?;
    let output = Path::new(&entry.out);
    let metadata = fs::symlink_metadata(output)?;
    if metadata.file_type().is_symlink() || (!metadata.is_dir() && !metadata.is_file()) {
        return Err(invalid("only a local Hangar file or directory can be published as a NAR"));
    }
    let (nar, stats) = super::write_nar(output)?;
    if entry.envelope.output_hash.is_empty() {
        return Err(invalid("Hangar entry has no output identity for cache publication"));
    }
    let key = read_trust_key(&binding.trust_key)?;
    let info = nar_info_for(&entry, &stats);
    let mut failures = Vec::new();
    for mirror in &binding.mirrors {
        let endpoint = parse_endpoint(mirror)?;
        validate_endpoint_binding(&binding, &endpoint, true)?;
        match publish_endpoint(&endpoint, &info, &nar, &key, roots, &entry) {
            Ok(proof) => {
                return Ok(CacheTransferReport {
                    role: binding.role.clone(),
                    mirror: mirror.clone(),
                    entry: entry.id.clone(),
                    output_hash: entry.envelope.output_hash.clone(),
                    nar_hash: stats.digest,
                    nix_nar_hash: proof.nix_nar_hash,
                    signed_fingerprint: proof.signed_fingerprint,
                    credential_provider: binding.credential_provider.clone(),
                    bytes: stats.bytes,
                });
            }
            Err(error) => failures.push(format!("{mirror}: {error}")),
        }
    }
    Err(invalid(&format!("all cache mirrors rejected publication: {}", failures.join("; "))))
}

pub fn verify_cache_transfer(
    roots: &Roots,
    target: &str,
    role: &str,
) -> io::Result<CacheTransferReport> {
    let binding = read_cache_binding(roots, role)?;
    let expected = select_entry(roots, target)?;
    let key = read_trust_key(&binding.trust_key)?;
    let mut failures = Vec::new();
    for mirror in &binding.mirrors {
        let endpoint = parse_endpoint(mirror)?;
        validate_endpoint_binding(&binding, &endpoint, false)?;
        if let Some(bytes) = verify_hangar_endpoint(&endpoint, &expected)? {
            return Ok(CacheTransferReport {
                role: binding.role.clone(),
                mirror: mirror.clone(),
                entry: expected.id.clone(),
                output_hash: expected.envelope.output_hash.clone(),
                nar_hash: super::nar_digest(&bytes),
                nix_nar_hash: None,
                signed_fingerprint: fingerprint_for_key(&key),
                credential_provider: binding.credential_provider.clone(),
                bytes: bytes.len() as u64,
            });
        }
        match verify_nix_endpoint(&endpoint, &expected) {
            Ok(Some(transfer)) => {
                return Ok(CacheTransferReport {
                    role: binding.role.clone(),
                    mirror: mirror.clone(),
                    entry: expected.id.clone(),
                    output_hash: expected.envelope.output_hash.clone(),
                    nar_hash: transfer.nar_hash,
                    nix_nar_hash: Some(transfer.nix_nar_hash),
                    signed_fingerprint: transfer.signed_fingerprint,
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
                return Ok(report_for(
                    &binding,
                    mirror,
                    artifact.info,
                    bytes.len() as u64,
                    Some(&expected),
                ));
            }
            Ok(None) => failures.push(format!("{mirror}: cache entry not found")),
            Err(error) => failures.push(format!("{mirror}: {error}")),
        }
    }
    Err(invalid(&format!("no verifying cache hit: {}", failures.join("; "))))
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
    let key = read_trust_key(&binding.trust_key)?;
    let mut failures = Vec::new();
    for mirror in &binding.mirrors {
        let endpoint = parse_endpoint(mirror)?;
        validate_endpoint_binding(&binding, &endpoint, false)?;
        if let Some(bytes) = substitute_hangar_endpoint(&endpoint, &expected, destination)? {
            return Ok(CacheTransferReport {
                role: binding.role.clone(),
                mirror: mirror.clone(),
                entry: expected.id.clone(),
                output_hash: expected.envelope.output_hash.clone(),
                nar_hash: super::nar_digest(&bytes),
                nix_nar_hash: None,
                signed_fingerprint: fingerprint_for_key(&key),
                credential_provider: binding.credential_provider.clone(),
                bytes: bytes.len() as u64,
            });
        }
        if let Some(transfer) = substitute_nix_endpoint(&endpoint, &expected, destination)? {
            return Ok(CacheTransferReport {
                role: binding.role.clone(),
                mirror: mirror.clone(),
                entry: expected.id.clone(),
                output_hash: expected.envelope.output_hash.clone(),
                nar_hash: transfer.nar_hash,
                nix_nar_hash: Some(transfer.nix_nar_hash),
                signed_fingerprint: transfer.signed_fingerprint,
                credential_provider: binding.credential_provider.clone(),
                bytes: transfer.nar.len() as u64,
            });
        }
        match find_artifact(&endpoint, target, Some(&expected), &key) {
            Ok(Some(artifact)) => {
                let result = (|| {
                    artifact.info.verify(&key)?;
                    verify_artifact_bytes(&artifact.info, &artifact.nar)?;
                    super::read_nar(&artifact.nar, destination)
                })();
                match result {
                    Ok(stats) => {
                        let actual = crate::Envelope::try_output_hash_of(&destination.to_string_lossy())
                            .map_err(io::Error::other)?;
                        if actual != expected.envelope.output_hash {
                            let _ = remove_tree(destination);
                            failures.push(format!(
                                "{mirror}: restored output hash {actual} disagrees with {}",
                                expected.envelope.output_hash
                            ));
                            continue;
                        }
                        return Ok(CacheTransferReport {
                            role: binding.role.clone(),
                            mirror: mirror.clone(),
                            entry: expected.id.clone(),
                            output_hash: expected.envelope.output_hash.clone(),
                            nar_hash: stats.digest,
                            nix_nar_hash: None,
                            signed_fingerprint: fingerprint_for_info(&artifact.info),
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
    Err(invalid(&format!("no verifying cache hit: {}", failures.join("; "))))
}

pub fn cache_binding_json(binding: &CacheBinding) -> String {
    let mirrors = binding
        .mirrors
        .iter()
        .map(|mirror| crate::JSON::quote(mirror))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"role\":{},\"mirrors\":[{}],\"credential_provider\":{},\"write\":{}}}",
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

pub fn cache_report_json(operation: &str, report: &CacheTransferReport) -> String {
    format!(
        "{{\"operation\":{},\"role\":{},\"mirror\":{},\"entry\":{},\"output_hash\":{},\"nar_hash\":{},\"nix_nar_hash\":{},\"signed_fingerprint\":{},\"credential_provider\":{},\"bytes\":{}}}",
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
        report
            .credential_provider
            .as_deref()
            .map(crate::JSON::quote)
            .unwrap_or_else(|| "null".to_string()),
        report.bytes
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

fn nar_info_for(entry: &StoreEntry, stats: &super::NarStats) -> NarInfo {
    let name = format!("{}-{}", entry.envelope.output_hash, entry.id);
    NarInfo {
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
        deriver: None,
        ca: None,
        signatures: Vec::new(),
    }
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
            let text = String::from_utf8(info_bytes)
                .map_err(|_| invalid("narinfo is not UTF-8"))?;
            let info = NarInfo::parse(&text)?;
            if info.store_path.rsplit('/').next() != Some(store_name.as_str())
                || !nar_info_matches_entry(&info, expected)
            {
                return Ok(None);
            }
            info.verify(key)?;
            let Some(nar) = endpoint_get(endpoint, &info.url, super::MAX_NAR_BYTES as u64)? else {
                return Ok(None);
            };
            Ok(Some(CacheArtifact { info, nar }))
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
        let expected_name = expected.map(|entry| format!("{}-{}", entry.envelope.output_hash, entry.id));
        let matches = expected_name.as_deref() == Some(store_name)
            && expected.is_some_and(|entry| nar_info_matches_entry(&info, entry));
        if !matches {
            continue;
        }
        if info.verify(key).is_err() {
            continue;
        }
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
        return Ok(Some(CacheArtifact { info, nar }));
    }
    if let Some(expected) = expected {
        let identity = format!("{}-{}", expected.envelope.output_hash, expected.id);
        write_negative_hint(mirror, &identity)?;
    }
    Ok(None)
}

fn report_for(
    binding: &CacheBinding,
    mirror: &str,
    info: NarInfo,
    bytes: u64,
    expected: Option<&StoreEntry>,
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
        credential_provider: binding.credential_provider.clone(),
        bytes,
    }
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
        || info.nar_hash != super::nar_digest(nar)
    {
        return Err(invalid("NAR bytes do not match signed FileSize, NarSize, or NarHash"));
    }
    super::validate_nar(nar)?;
    Ok(())
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
    info.store_path == expected_path
        && info.url == expected_url
        && info.nar_hash.starts_with("sha256:")
        && actual_references == expected_references
}

fn read_trust_key(path: &Path) -> io::Result<TrustKey> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > 4096 {
        return Err(invalid("cache trust key is not a regular file"));
    }
    TrustKey::from_secret(read_regular_bounded(path, 4096)?)
        .map_err(|error| invalid(&error.to_string()))
}

fn write_new_key(path: &Path, role: &str) -> io::Result<()> {
    ensure_parent(path)?;
    let mut secret = vec![0u8; 32];
    if let Ok(mut random) = fs::File::open("/dev/urandom") {
        random.read_exact(&mut secret)?;
    } else {
        let seed = format!(
            "jet-cache-key-v1\n{role}\n{}\n{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|value| value.as_nanos())
                .unwrap_or_default()
        );
        let digest = SHA256::sha256_hex(seed.as_bytes());
        for (slot, pair) in secret.iter_mut().zip(digest.as_bytes().chunks(2)) {
            *slot = u8::from_str_radix(std::str::from_utf8(pair).unwrap_or("00"), 16).unwrap_or(0);
        }
    }
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
        return Ok(CacheEndpoint::Http(endpoint.trim_end_matches('/').to_string()));
    }
    if endpoint.starts_with("ssh://") || endpoint.starts_with("ssh-ng://") {
        return parse_ssh_endpoint(endpoint);
    }
    if endpoint.starts_with("s3://") {
        validate_s3_uri(endpoint)?;
        return Ok(CacheEndpoint::S3(endpoint.trim_end_matches('/').to_string()));
    }
    if endpoint.starts_with("daemon://") || endpoint.starts_with("nix://") {
        if endpoint.contains('?') || endpoint.contains('#') {
            return Err(invalid("Nix store endpoint cannot contain a query or fragment"));
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
        return Err(invalid(&format!("{label} cannot contain a query or fragment")));
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
        || !authority
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'@' | b':' | b'-'))
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
        || key.bytes().any(|byte| matches!(byte, b'\'' | b'"' | b'`' | b'$' | b';' | b'&' | b'|' | b'<' | b'>'))
    {
        return Err(invalid("remote cache path is unsafe"));
    }
    Ok(())
}

fn validate_relative_key(key: &str) -> io::Result<()> {
    if key.is_empty()
        || key.starts_with('/')
        || key.bytes().any(|byte| {
            byte == 0
                || byte.is_ascii_control()
                || matches!(byte, b'?' | b'#' | b'%')
        })
        || Path::new(key).components().any(|component| {
            matches!(component, Component::ParentDir | Component::Prefix(_))
        })
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

fn endpoint_get(
    endpoint: &CacheEndpoint,
    key: &str,
    limit: u64,
) -> io::Result<Option<Vec<u8>>> {
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
) -> io::Result<TransferProof> {
    let caps = endpoint.capabilities();
    if !caps.write {
        return Err(invalid(&format!("{} endpoint is read-only", endpoint.label())));
    }
    match endpoint {
        CacheEndpoint::Local(root) => {
            publish_local_resumable(root, info, nar, key)?;
            Ok(TransferProof {
                nix_nar_hash: None,
                signed_fingerprint: fingerprint_for_key(key),
            })
        }
        CacheEndpoint::Hangar => Err(invalid(
            "Hangar is the local source store and cannot publish cache objects",
        )),
        CacheEndpoint::Nix(uri) => {
            let stats = super::validate_nar(nar)?;
            nix_copy(uri, "--to", &entry.out)?;
            prove_nix_transfer(uri, &entry.out, &stats.digest, stats.bytes)
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
            publish_remote_atomic(
                endpoint,
                [(info.url.as_str(), nar), (&info_key, signed_text.as_bytes())],
            )?;
            Ok(TransferProof {
                nix_nar_hash: None,
                signed_fingerprint: fingerprint_for_info(&signed),
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
            "{} cache endpoint does not provide a write capability",
            endpoint.label()
        )));
    }
    if !capabilities.promote {
        return Err(invalid(&format!(
            "{} cache endpoint has no atomic promotion capability",
            endpoint.label()
        )));
    }
    for (key, bytes) in objects {
        let partial = format!("{key}.partial");
        write_remote_resumable(endpoint, &partial, bytes)?;
        let uploaded = endpoint_get(endpoint, &partial, bytes.len() as u64 + 1)?
            .ok_or_else(|| invalid("cache endpoint lost its staged object"))?;
        if uploaded != bytes {
            return Err(invalid("cache endpoint staged bytes differ from the signed object"));
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
        CacheEndpoint::Ssh { target, .. } => ssh_append(target, &endpoint_key(endpoint, key)?, &bytes[offset..]),
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
        return Err(invalid("cache endpoint already has conflicting final bytes"));
    }
    match endpoint {
        CacheEndpoint::Ssh { target, .. } => {
            ssh_mkdir_for_key(endpoint, final_key)?;
            ssh_promote(target, &endpoint_key(endpoint, partial)?, &endpoint_key(endpoint, final_key)?)
        }
        CacheEndpoint::S3(_) => s3_promote(endpoint_key(endpoint, partial)?, endpoint_key(endpoint, final_key)?),
        _ => Err(invalid(&format!(
            "{} cache endpoint cannot promote staged objects",
            endpoint.label()
        ))),
    }
}

fn verify_nix_endpoint(
    endpoint: &CacheEndpoint,
    entry: &StoreEntry,
) -> io::Result<Option<NixTransfer>> {
    let CacheEndpoint::Nix(uri) = endpoint else {
        return Ok(None);
    };
    nix_copy(uri, "--from", &entry.out)?;
    let actual = crate::Envelope::try_output_hash_of(&entry.out).map_err(io::Error::other)?;
    if actual != entry.envelope.output_hash {
        return Err(invalid("Nix store output hash disagrees with the Hangar identity"));
    }
    let (nar, stats) = super::write_nar(Path::new(&entry.out))?;
    let proof = prove_nix_transfer(uri, &entry.out, &stats.digest, stats.bytes)?;
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
        return Err(invalid("local Hangar output hash disagrees with its identity"));
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
        copy_tree(Path::new(&entry.out), destination)?;
        let actual = crate::Envelope::try_output_hash_of(&destination.to_string_lossy())
            .map_err(io::Error::other)?;
        if actual != entry.envelope.output_hash {
            return Err(invalid("local Hangar output hash disagrees with its identity"));
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
) -> io::Result<Option<NixTransfer>> {
    if !matches!(endpoint, CacheEndpoint::Nix(_)) {
        return Ok(None);
    }
    let result = (|| {
        let transfer = verify_nix_endpoint(endpoint, entry)?
            .ok_or_else(|| invalid("Nix substitution was called for a non-Nix endpoint"))?;
        super::read_nar(&transfer.nar, destination)?;
        let actual = crate::Envelope::try_output_hash_of(&destination.to_string_lossy())
            .map_err(io::Error::other)?;
        if actual != entry.envelope.output_hash {
            return Err(invalid("Nix store output hash disagrees with the Hangar identity"));
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
) -> io::Result<TransferProof> {
    let info = nix_path_info(uri, path)?;
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
        nix_nar_hash: Some(info.nar_hash),
        signed_fingerprint: info.signed_fingerprint,
    })
}

fn nix_path_info(uri: &str, path: &str) -> io::Result<NixPathInfo> {
    let child = Command::new("nix")
        .args(["path-info", "--json", "--store", uri, path])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| invalid(&format!("Nix store metadata adapter could not start: {error}")))?;
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
            let (path, value) = object
                .iter()
                .next()
                .expect("one-entry Nix metadata object");
            (path.as_str(), value.as_object().map_err(io::Error::other)?)
        }
        crate::JSON::JSONValue::Array(_) => {
            return Err(invalid("Nix store metadata returned more than one path"));
        }
        _ => return Err(invalid("Nix store metadata is not an object")),
    };
    if reported_path != path {
        return Err(invalid("Nix store metadata path disagrees with the requested path"));
    }
    let nar_hash = object
        .get("narHash")
        .ok_or_else(|| invalid("Nix store metadata has no NarHash"))?
        .as_str()
        .map_err(io::Error::other)?
        .to_string();
    let nar_size = object.get("narSize").map(|value| {
        let size = match value {
            crate::JSON::JSONValue::Num(size)
                if size.is_finite() && *size >= 0.0 && size.fract() == 0.0 => *size as u64,
            _ => return Err(invalid("Nix store metadata NarSize is not an integer")),
        };
        Ok(size)
    }).transpose()?;
    let signatures = match object.get("signatures") {
        Some(value) => value
            .as_array()
            .map_err(io::Error::other)?
            .iter()
            .map(|value| {
                let signature = value.as_str().map_err(io::Error::other)?;
                if signature.is_empty()
                    || signature.len() > 4096
                    || signature.bytes().any(|byte| byte == 0 || byte.is_ascii_control())
                {
                    return Err(invalid("Nix store metadata has an invalid signature fingerprint"));
                }
                Ok(signature.to_string())
            })
            .collect::<io::Result<Vec<_>>>()?,
        None => Vec::new(),
    };
    let signed_fingerprint = if signatures.is_empty() {
        if matches!(object.get("ultimate"), Some(crate::JSON::JSONValue::Bool(true))) {
            "nix:ultimate".to_string()
        } else {
            return Err(invalid("Nix store metadata has no signed fingerprint"));
        }
    } else {
        signatures.join(",")
    };
    Ok(NixPathInfo {
        nar_hash,
        nar_size,
        signed_fingerprint,
    })
}

fn nix_nar_hash_to_jet(value: &str) -> io::Result<String> {
    if let Some(hex) = value.strip_prefix("sha256:") {
        if hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Ok(format!("sha256:{}", hex.to_ascii_lowercase()));
        }
        return Err(invalid("Nix NarHash uses an unsupported non-hex sha256 encoding"));
    }
    let encoded = value
        .strip_prefix("sha256-")
        .ok_or_else(|| invalid("Nix metadata NarHash is not sha256"))?;
    let bytes = jet_foundation::base_encoding_strict::decode_base64(encoded, false, false)
        .map_err(|error| invalid(&format!("Nix metadata NarHash is invalid: {error}")))?;
    if bytes.len() != 32 {
        return Err(invalid("Nix metadata NarHash is not a 256-bit digest"));
    }
    Ok(format!("sha256:{}", bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>()))
}

fn nix_copy(uri: &str, direction: &str, path: &str) -> io::Result<()> {
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
    let metadata = fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() {
        return Err(invalid("Nix store output is a symlink"));
    }
    if metadata.is_dir() {
        if destination.exists() {
            return Err(invalid("cache substitution destination already exists"));
        }
        fs::create_dir_all(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy_tree(&entry.path(), &destination.join(entry.file_name()))?;
        }
        return Ok(());
    }
    if !metadata.is_file() {
        return Err(invalid("Nix store output is not a regular tree"));
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(source, destination)?;
    Ok(())
}

fn http_get(url: String, limit: u64) -> io::Result<Option<Vec<u8>>> {
    let child = Command::new("curl")
        .args(["--silent", "--show-error", "--netrc-optional", "--compressed", "--max-time", "120", "--write-out", "\n%{http_code}", &url])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| invalid(&format!("HTTP cache adapter could not start: {error}")))?;
    let (status_result, stdout, stderr) = run_bounded_output(
        child,
        limit.saturating_add(64),
        "HTTP cache response",
    )?;
    let Some(separator) = stdout.iter().rposition(|byte| *byte == b'\n') else {
        return Err(invalid("HTTP cache adapter returned no status"));
    };
    let status = std::str::from_utf8(&stdout[separator + 1..])
        .ok()
        .and_then(|value| value.trim().parse::<u16>().ok())
        .ok_or_else(|| invalid("HTTP cache adapter returned an invalid status"))?;
    if status == 404 || status == 410 {
        return Ok(None);
    }
    if !status_result.success() || !(200..300).contains(&status) {
        return Err(invalid(&format!("HTTP cache GET failed with status {status}: {}", bounded_stderr(&stderr))));
    }
    let body = &stdout[..separator];
    if body.len() as u64 > limit {
        return Err(invalid("HTTP cache response exceeded its bound"));
    }
    Ok(Some(body.to_vec()))
}

fn http_put(url: String, bytes: &[u8]) -> io::Result<()> {
    let mut child = Command::new("curl")
        .args(["--silent", "--show-error", "--netrc-optional", "--compressed", "--max-time", "120", "--request", "PUT", "--data-binary", "@-", "--output", null_device(), "--write-out", "\n%{http_code}", &url])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| invalid(&format!("HTTP cache adapter could not start: {error}")))?;
    child.stdin.take().ok_or_else(|| invalid("HTTP cache adapter has no input"))?.write_all(bytes)?;
    let (status_result, stdout, stderr) = run_bounded_output(child, 64, "HTTP cache response")?;
    let status = stdout.rsplit(|byte| *byte == b'\n').next()
        .and_then(|value| std::str::from_utf8(value).ok())
        .and_then(|value| value.trim().parse::<u16>().ok())
        .unwrap_or(0);
    if status_result.success() && (200..300).contains(&status) {
        Ok(())
    } else {
        Err(invalid(&format!("HTTP cache PUT failed with status {status}: {}", bounded_stderr(&stderr))))
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
        Err(invalid(&format!("SSH cache mkdir failed: {}", bounded_stderr(&stderr))))
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
    child.stdin.take().ok_or_else(|| invalid("SSH cache adapter has no input"))?.write_all(bytes)?;
    let (status, _, stderr) = run_bounded_output(child, 1, "SSH cache response")?;
    if status.success() {
        Ok(())
    } else {
        Err(invalid(&format!("SSH cache PUT failed: {}", bounded_stderr(&stderr))))
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
    child.stdin.take().ok_or_else(|| invalid("SSH cache adapter has no input"))?.write_all(bytes)?;
    let (status, _, stderr) = run_bounded_output(child, 1, "SSH cache response")?;
    if status.success() {
        Ok(())
    } else {
        Err(invalid(&format!("SSH cache append failed: {}", bounded_stderr(&stderr))))
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
        Err(invalid(&format!("SSH cache promotion failed: {}", bounded_stderr(&stderr))))
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
    child.stdin.take().ok_or_else(|| invalid("S3 cache adapter has no input"))?.write_all(bytes)?;
    let (status, _, stderr) = run_bounded_output(child, 4096, "S3 cache response")?;
    if status.success() {
        Ok(())
    } else {
        Err(invalid(&format!("S3 cache PUT failed: {}", bounded_stderr(&stderr))))
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
        Err(invalid(&format!("S3 cache promotion failed: {}", bounded_stderr(&stderr))))
    }
}

fn bounded_stderr(bytes: &[u8]) -> String {
    String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]).trim().to_string()
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
    if cfg!(windows) { "NUL" } else { "/dev/null" }
}

fn publish_local_resumable(
    root: &Path,
    info: &NarInfo,
    nar: &[u8],
    key: &TrustKey,
) -> io::Result<()> {
    crate::RuntimePolicy::with_lock(root, "cache-publish", || {
        let signed = info.clone().signed(key)?;
        if signed.nar_size != nar.len() as u64 || signed.nar_hash != super::nar_digest(nar) {
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
        write_resumable(&nar_path, nar)?;
        write_resumable(&info_path, signed.to_text()?.as_bytes())?;
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
        if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > bytes.len() as u64 {
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
            Err(invalid("cache object was concurrently published with different bytes"))
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
                Err(invalid("cache object was concurrently published with different bytes"))
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
        return Err(invalid("cache endpoint does not provide read capability"));
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
            "{} cache endpoint does not provide a read capability",
            endpoint.label()
        )));
    }
    if write && !caps.write {
        return Err(invalid(&format!(
            "{} cache endpoint does not provide a write capability",
            endpoint.label()
        )));
    }
    if !caps.trust {
        return Err(invalid(&format!(
            "{} cache endpoint does not provide a trust capability",
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
    if value.bytes().any(|byte| byte == 0 || byte == b'\n' || byte == b'\r') {
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
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > limit {
        return Err(invalid("cache input is not a bounded regular file"));
    }
    let file = fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(invalid("cache input exceeded its bound while being read"));
    }
    Ok(bytes)
}

fn remove_tree(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        fs::remove_file(path)
    } else {
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
            nix_nar_hash_to_jet("sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=")
                .unwrap(),
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
}
