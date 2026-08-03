//! Host-owned binary-cache bindings and the signed NAR transfer boundary.
//!
//! Workspace policy may request cache roles. This module owns the host-side
//! binding that maps a role to ordered mirrors, trust material, and an
//! optional typed credential-provider label. Secrets never enter this file,
//! URLs, argv, locks, or logs. The current transport adapters are local paths
//! and `file://` mirrors; other endpoint families fail with an explicit
//! capability error until their verified adapter is present.

use super::{NarInfo, Roots, StoreEntry};
use crate::TrustRoot::TrustKey;
use crate::SHA256;
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const BINDING_MAGIC: &str = "jet-cache-bind-v1";
const MAX_BINDING_BYTES: u64 = 1024 * 1024;
const MAX_INFO_BYTES: u64 = 1024 * 1024;

struct CacheArtifact {
    info: NarInfo,
    info_path: PathBuf,
    nar_path: PathBuf,
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
    pub nar_hash: String,
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
    let text = fs::read_to_string(path)?;
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
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(invalid("cache binding directory contains a non-regular entry"));
        }
        let file_role = path
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| invalid("cache binding file name is not UTF-8"))?;
        let binding = CacheBinding::decode(&fs::read_to_string(&path)?)?;
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
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(invalid("only a local Hangar directory can be published as a NAR"));
    }
    let (nar, stats) = super::write_nar(output)?;
    if entry.envelope.output_hash.is_empty() {
        return Err(invalid("Hangar entry has no output identity for cache publication"));
    }
    let key = read_trust_key(&binding.trust_key)?;
    let info = nar_info_for(&entry, &stats);
    let mut failures = Vec::new();
    for mirror in &binding.mirrors {
        let mirror_path = endpoint_path(mirror)?;
        ensure_directory(&mirror_path)?;
        ensure_directory(&mirror_path.join("nar"))?;
        match super::publish_local(&mirror_path, info.clone(), &nar, &key) {
            Ok(_) => {
                return Ok(CacheTransferReport {
                    role: binding.role,
                    mirror: mirror.clone(),
                    entry: entry.id,
                    output_hash: entry.envelope.output_hash,
                    nar_hash: stats.digest,
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
        let mirror_path = endpoint_path(mirror)?;
        match find_artifact(&mirror_path, target, Some(&expected), &key) {
            Ok(Some(artifact)) => {
                let bytes = read_regular_bounded(&artifact.nar_path, super::MAX_NAR_BYTES as u64)?;
                let digest = super::nar_digest(&bytes);
                if digest != artifact.info.nar_hash {
                    failures.push(format!("{mirror}: NAR digest mismatch"));
                    continue;
                }
                if let Err(error) = super::validate_nar(&bytes) {
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
        let mirror_path = endpoint_path(mirror)?;
        match find_artifact(&mirror_path, target, Some(&expected), &key) {
            Ok(Some(artifact)) => match super::substitute_local(
                &artifact.nar_path,
                &artifact.info_path,
                destination,
                &key,
            ) {
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
                        role: binding.role,
                        mirror: mirror.clone(),
                        entry: expected.id.clone(),
                        output_hash: expected.envelope.output_hash.clone(),
                        nar_hash: stats.digest,
                        bytes: stats.bytes,
                    });
                }
                Err(error) => failures.push(format!("{mirror}: {error}")),
            },
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
        "{{\"operation\":{},\"role\":{},\"mirror\":{},\"entry\":{},\"output_hash\":{},\"nar_hash\":{},\"bytes\":{}}}",
        crate::JSON::quote(operation),
        crate::JSON::quote(&report.role),
        crate::JSON::quote(&report.mirror),
        crate::JSON::quote(&report.entry),
        crate::JSON::quote(&report.output_hash),
        crate::JSON::quote(&report.nar_hash),
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
        signatures: Vec::new(),
    }
}

fn find_artifact(
    mirror: &Path,
    _target: &str,
    expected: Option<&StoreEntry>,
    key: &TrustKey,
) -> io::Result<Option<CacheArtifact>> {
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
        let nar_metadata = fs::symlink_metadata(&nar_path)?;
        if nar_metadata.file_type().is_symlink() || !nar_metadata.is_file() {
            continue;
        }
        return Ok(Some(CacheArtifact {
            info,
            info_path,
            nar_path,
        }));
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
            .unwrap_or(info.store_path),
        output_hash: expected
            .map(|entry| entry.envelope.output_hash.clone())
            .unwrap_or_default(),
        nar_hash: info.nar_hash,
        bytes,
    }
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
    TrustKey::from_secret(fs::read(path)?).map_err(|error| invalid(&error.to_string()))
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

fn endpoint_path(endpoint: &str) -> io::Result<PathBuf> {
    if let Some(path) = endpoint.strip_prefix("file://") {
        if path.is_empty() || !Path::new(path).is_absolute() {
            return Err(invalid("file cache endpoint must contain an absolute path"));
        }
        if path.contains('?') || path.contains('#') {
            return Err(invalid("file cache endpoint cannot contain a query or fragment"));
        }
        return Ok(PathBuf::from(path));
    }
    if endpoint.starts_with("http://")
        || endpoint.starts_with("https://")
        || endpoint.starts_with("ssh://")
        || endpoint.starts_with("s3://")
        || endpoint.starts_with("hangar://")
        || endpoint.starts_with("daemon://")
    {
        return Err(invalid(
            "cache endpoint capability is not available in this build; bind a local path or file:// mirror",
        ));
    }
    let path = PathBuf::from(endpoint);
    if !path.is_absolute() {
        return Err(invalid("cache mirror must be an absolute path or file:// endpoint"));
    }
    Ok(path)
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
    validate_text(endpoint, "cache endpoint")?;
    let path = endpoint_path(endpoint)?;
    validate_path_components(&path)?;
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
    let mut file = fs::File::open(path)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes)?;
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

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}
