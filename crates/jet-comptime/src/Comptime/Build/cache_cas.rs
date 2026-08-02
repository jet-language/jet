use super::actions_policy::BuildAction;
#[cfg(not(unix))]
use super::execution_runtime::prepare_output_destination;
use super::targets::BuildPath;
use super::validation::resolve_under;
use crate::SHA256;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const MAX_REMOTE_WIRE_BYTES: usize = 4 * 1024 * 1024;
const MAX_REMOTE_BLOB_BYTES: usize = 64 * 1024 * 1024;
const MAX_REMOTE_ITEMS: usize = 100_000;
const MAX_REMOTE_AUTH_KEY_BYTES: usize = 4096;

static REMOTE_EXECUTION_COMMIT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn remote_execution_commit_lock() -> Result<std::sync::MutexGuard<'static, ()>, RemoteCacheError> {
    REMOTE_EXECUTION_COMMIT_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| {
            RemoteCacheError::InvalidRecord(
                "remote execution commit lock was poisoned".to_string(),
            )
        })
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActionKey(pub(super) String);

impl ActionKey {
    pub fn new(value: impl Into<String>) -> Self {
        ActionKey(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContentDigest(String);

impl ContentDigest {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        ContentDigest(format!("sha256:{}", SHA256::sha256_hex(bytes)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn parse(value: &str) -> io::Result<Self> {
        let Some(hex) = value.strip_prefix("sha256:") else {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "digest must start with `sha256:`"));
        };
        if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "sha256 digest must contain exactly 64 hexadecimal digits"));
        }
        Ok(ContentDigest(format!("sha256:{}", hex.to_ascii_lowercase())))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionOutcome {
    Succeeded { exit_code: i32 },
    Failed { exit_code: i32 },
    RestoredFromCache,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheHitReason {
    LocalActionRecordMatched,
    DeclaredOutputsRestored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheMissReason {
    NoLocalActionRecord,
    ActionKeyChanged,
    DeclaredOutputMissing,
    CacheRecordInvalid,
    CacheRestoreFailed,
    RemoteDenied,
    UncachedAction,
    FrontEndIncomplete,
}

/// Stages that must finish before any action-cache lookup (E4-JP2 / #419).
/// Cache hits never skip parser, sema, policy, or diagnostics emission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FrontEndCompletion {
    pub parsed: bool,
    pub sema_checked: bool,
    pub policy_checked: bool,
    pub diagnostics_complete: bool,
}

impl FrontEndCompletion {
    pub fn all_complete() -> Self {
        FrontEndCompletion {
            parsed: true,
            sema_checked: true,
            policy_checked: true,
            diagnostics_complete: true,
        }
    }

    pub fn authorize_cache_lookup(self) -> Result<(), CacheBypassDenied> {
        if !self.parsed {
            return Err(CacheBypassDenied::Parser);
        }
        if !self.sema_checked {
            return Err(CacheBypassDenied::Sema);
        }
        if !self.policy_checked {
            return Err(CacheBypassDenied::Policy);
        }
        if !self.diagnostics_complete {
            return Err(CacheBypassDenied::Diagnostics);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheBypassDenied {
    Parser,
    Sema,
    Policy,
    Diagnostics,
}

impl CacheBypassDenied {
    pub fn as_str(self) -> &'static str {
        match self {
            CacheBypassDenied::Parser => "parser",
            CacheBypassDenied::Sema => "sema",
            CacheBypassDenied::Policy => "policy",
            CacheBypassDenied::Diagnostics => "diagnostics",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionCacheStatus {
    Hit(CacheHitReason),
    Miss(CacheMissReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionCacheProvenance {
    pub status: ActionCacheStatus,
    pub remote_policy: RemoteCachePolicy,
}

impl ActionCacheProvenance {
    pub fn hit(reason: CacheHitReason) -> Self {
        ActionCacheProvenance {
            status: ActionCacheStatus::Hit(reason),
            remote_policy: RemoteCachePolicy::disabled_until_grant_and_sandbox_proof(),
        }
    }

    pub fn miss(reason: CacheMissReason) -> Self {
        ActionCacheProvenance {
            status: ActionCacheStatus::Miss(reason),
            remote_policy: RemoteCachePolicy::disabled_until_grant_and_sandbox_proof(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteActionRequest {
    CacheRead,
    CacheWrite,
    Execute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteDeniedReason {
    MissingGrantAndSandboxProof,
    GrantNotAllowed,
    ProofDoesNotMatchAction,
    MissingAuthentication,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteCacheDenied {
    pub request: RemoteActionRequest,
    pub reason: RemoteDeniedReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSandboxProof {
    pub sandbox_id: String,
    pub action_key: String,
    pub provenance_digest: ContentDigest,
    pub worker_id: String,
    pub platform: String,
    pub abi: String,
    pub worker_receipt: String,
}

impl RemoteSandboxProof {
    pub fn new(
        sandbox_id: impl Into<String>,
        action_key: impl Into<String>,
        provenance_digest: ContentDigest,
    ) -> Self {
        RemoteSandboxProof {
            sandbox_id: sandbox_id.into(),
            action_key: action_key.into(),
            provenance_digest,
            worker_id: String::new(),
            platform: String::new(),
            abi: String::new(),
            worker_receipt: String::new(),
        }
    }

    fn with_worker_identity(
        mut self,
        worker_id: impl Into<String>,
        platform: impl Into<String>,
        abi: impl Into<String>,
        worker_receipt: impl Into<String>,
    ) -> Self {
        self.worker_id = worker_id.into();
        self.platform = platform.into();
        self.abi = abi.into();
        self.worker_receipt = worker_receipt.into();
        self
    }

    fn is_complete(&self) -> bool {
        !self.sandbox_id.is_empty()
            && self.sandbox_id.len() <= 512
            && !self.sandbox_id.chars().any(|ch| ch.is_control())
            && !self.action_key.is_empty()
            && self.action_key.len() <= 4096
            && !self.action_key.chars().any(|ch| ch.is_control())
            && ContentDigest::parse(self.provenance_digest.as_str()).is_ok()
            && (self.worker_id.is_empty()
                && self.platform.is_empty()
                && self.abi.is_empty()
                && self.worker_receipt.is_empty()
                || !self.worker_id.is_empty()
                    && !self.platform.is_empty()
                    && !self.abi.is_empty()
                    && !self.worker_receipt.is_empty()
                    && self.worker_id.len() <= 256
                    && self.platform.len() <= 256
                    && self.abi.len() <= 256
                    && self.worker_receipt.len() <= 256
                    && !self.worker_id.chars().any(|ch| ch.is_control())
                    && !self.platform.chars().any(|ch| ch.is_control())
                    && !self.abi.chars().any(|ch| ch.is_control())
                    && !self.worker_receipt.chars().any(|ch| ch.is_control()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteCachePolicy {
    DisabledUntilGrantAndSandboxProof,
    Granted {
        cache_read: bool,
        cache_write: bool,
        execute: bool,
        proof: RemoteSandboxProof,
    },
}

impl RemoteCachePolicy {
    pub fn disabled_until_grant_and_sandbox_proof() -> Self {
        RemoteCachePolicy::DisabledUntilGrantAndSandboxProof
    }

    pub fn granted(proof: RemoteSandboxProof) -> Self {
        RemoteCachePolicy::Granted {
            cache_read: true,
            cache_write: true,
            execute: true,
            proof,
        }
    }

    pub fn with_grants(
        cache_read: bool,
        cache_write: bool,
        execute: bool,
        proof: RemoteSandboxProof,
    ) -> Self {
        RemoteCachePolicy::Granted {
            cache_read,
            cache_write,
            execute,
            proof,
        }
    }

    pub fn proof(&self) -> Option<&RemoteSandboxProof> {
        match self {
            RemoteCachePolicy::DisabledUntilGrantAndSandboxProof => None,
            RemoteCachePolicy::Granted { proof, .. } => Some(proof),
        }
    }

    pub fn check(&self, request: RemoteActionRequest) -> Result<(), RemoteCacheDenied> {
        match self {
            RemoteCachePolicy::DisabledUntilGrantAndSandboxProof => {
                Err(RemoteCacheDenied {
                    request,
                    reason: RemoteDeniedReason::MissingGrantAndSandboxProof,
                })
            }
            RemoteCachePolicy::Granted {
                cache_read,
                cache_write,
                execute,
                proof,
            } => {
                let allowed = match request {
                    RemoteActionRequest::CacheRead => *cache_read,
                    RemoteActionRequest::CacheWrite => *cache_write,
                    RemoteActionRequest::Execute => *execute,
                };
                if !proof.is_complete() {
                    return Err(RemoteCacheDenied {
                        request,
                        reason: RemoteDeniedReason::MissingGrantAndSandboxProof,
                    });
                }
                if !allowed {
                    return Err(RemoteCacheDenied {
                        request,
                        reason: RemoteDeniedReason::GrantNotAllowed,
                    });
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug)]
pub enum RemoteCacheError {
    Denied(RemoteCacheDenied),
    Io(io::Error),
    InvalidRecord(String),
}

impl std::fmt::Display for RemoteCacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RemoteCacheError::Denied(denied) => write!(
                f,
                "remote {:?} denied: {:?}",
                denied.request, denied.reason
            ),
            RemoteCacheError::Io(error) => write!(f, "remote cache I/O failed: {error}"),
            RemoteCacheError::InvalidRecord(message) => {
                write!(f, "remote cache record is invalid: {message}")
            }
        }
    }
}

impl std::error::Error for RemoteCacheError {}

impl From<io::Error> for RemoteCacheError {
    fn from(error: io::Error) -> Self {
        RemoteCacheError::Io(error)
    }
}

/// Filesystem-backed remote transport used by the remote-cache and remote-
/// execution seams. It is a transport, not a local fallback: every operation
/// requires a grant plus a complete sandbox proof, and records are keyed by
/// the action key rather than by a caller-controlled filename.
#[derive(Clone, PartialEq, Eq)]
pub struct RemoteCredential(Vec<u8>);

impl RemoteCredential {
    pub fn new(bytes: impl AsRef<[u8]>) -> Result<Self, String> {
        let bytes = bytes.as_ref();
        if bytes.is_empty() {
            return Err("remote credential cannot be empty".to_string());
        }
        if bytes.len() > MAX_REMOTE_AUTH_KEY_BYTES {
            return Err(format!(
                "remote credential exceeds {MAX_REMOTE_AUTH_KEY_BYTES} bytes"
            ));
        }
        Ok(Self(bytes.to_vec()))
    }

    fn bytes(&self) -> &[u8] {
        &self.0
    }
}

impl std::fmt::Debug for RemoteCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteCredential")
            .field("configured", &true)
            .finish()
    }
}

/// A host-owned builder binding. The endpoint root and credential are supplied
/// by the host configuration layer; source text and ordinary build flags never
/// construct either value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteBuildBinding {
    pub builder: String,
    pub root: PathBuf,
    pub cache_read: bool,
    pub cache_write: bool,
    pub execute: bool,
    pub fallback_local: bool,
    pub timeout_ms: u64,
    pub trust_domain: String,
    pub worker_id: String,
    pub platform: String,
    pub abi: String,
    credential: RemoteCredential,
}

impl RemoteBuildBinding {
    pub fn new(
        builder: impl Into<String>,
        root: impl Into<PathBuf>,
        credential: impl AsRef<[u8]>,
    ) -> Result<Self, String> {
        let builder = builder.into();
        let root = root.into();
        validate_remote_name(&builder, "builder")?;
        if !root.is_absolute() {
            return Err("remote builder root must be an absolute host path".to_string());
        }
        let credential = RemoteCredential::new(credential)?;
        Ok(Self {
            builder,
            root,
            cache_read: false,
            cache_write: false,
            execute: false,
            fallback_local: false,
            timeout_ms: 30_000,
            trust_domain: String::new(),
            worker_id: "worker".to_string(),
            platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
            abi: "native".to_string(),
            credential,
        })
    }

    pub fn with_cache_read(mut self, enabled: bool) -> Self {
        self.cache_read = enabled;
        self
    }

    pub fn with_cache_write(mut self, enabled: bool) -> Self {
        self.cache_write = enabled;
        self
    }

    pub fn with_execute(mut self, enabled: bool) -> Self {
        self.execute = enabled;
        self
    }

    pub fn with_local_fallback(mut self, enabled: bool) -> Self {
        self.fallback_local = enabled;
        self
    }

    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms.max(1);
        self
    }

    pub fn with_trust_domain(mut self, trust_domain: impl Into<String>) -> Self {
        self.trust_domain = trust_domain.into();
        self
    }

    pub fn with_worker_id(mut self, worker_id: impl Into<String>) -> Self {
        self.worker_id = worker_id.into();
        self
    }

    pub fn with_platform(mut self, platform: impl Into<String>) -> Self {
        self.platform = platform.into();
        self
    }

    pub fn with_abi(mut self, abi: impl Into<String>) -> Self {
        self.abi = abi.into();
        self
    }

    pub fn is_enabled(&self) -> bool {
        self.cache_read || self.cache_write || self.execute
    }

    /// Register a host-owned builder binding. The credential is read from a
    /// separate provider file and is never written into the registry record.
    pub fn bind_host(
        builder: impl Into<String>,
        root: impl Into<PathBuf>,
        credential_provider: impl Into<PathBuf>,
        trust_domain: impl Into<String>,
        worker_id: impl Into<String>,
        platform: impl Into<String>,
        abi: impl Into<String>,
        cache_read: bool,
        cache_write: bool,
        execute: bool,
        fallback_local: bool,
        timeout_ms: u64,
    ) -> Result<Self, String> {
        let credential_provider = absolute_host_path(credential_provider.into())?;
        let credential = read_remote_credential_provider(&credential_provider)?;
        let binding = Self::new(builder, root, credential)?
            .with_trust_domain(trust_domain)
            .with_worker_id(worker_id)
            .with_platform(platform)
            .with_abi(abi)
            .with_cache_read(cache_read)
            .with_cache_write(cache_write)
            .with_execute(execute)
            .with_local_fallback(fallback_local)
            .with_timeout_ms(timeout_ms);
        binding.save_host(&credential_provider)?;
        Ok(binding)
    }

    /// Load one previously registered host binding by name.
    pub fn load_host(builder: &str) -> Result<Self, String> {
        validate_remote_name(builder, "builder")?;
        let (record_root, path) = remote_host_record_path(builder)?;
        let bytes = secure_read_file_bounded(&record_root, &record_root.join(&path), 64 * 1024)
            .map_err(|error| format!("cannot read remote builder `{builder}`: {error}"))?;
        let fields = parse_remote_host_record(&bytes, builder)?;
        let credential_provider = absolute_host_path(PathBuf::from(
            fields
                .get("credential_provider")
                .ok_or_else(|| "remote binding has no credential provider".to_string())?,
        ))?;
        let credential = read_remote_credential_provider(&credential_provider)?;
        let binding = Self::new(
            builder.to_string(),
            PathBuf::from(required_host_field(&fields, "root")?),
            credential,
        )?
        .with_trust_domain(required_host_field(&fields, "trust_domain")?.to_string())
        .with_worker_id(required_host_field(&fields, "worker_id")?.to_string())
        .with_platform(required_host_field(&fields, "platform")?.to_string())
        .with_abi(required_host_field(&fields, "abi")?.to_string())
        .with_cache_read(parse_host_bool(&fields, "cache_read")?)
        .with_cache_write(parse_host_bool(&fields, "cache_write")?)
        .with_execute(parse_host_bool(&fields, "execute")?)
        .with_local_fallback(parse_host_bool(&fields, "fallback_local")?)
        .with_timeout_ms(
            required_host_field(&fields, "timeout_ms")?
                .parse::<u64>()
                .map_err(|_| "remote binding timeout is not a number".to_string())?,
        );
        Ok(binding)
    }

    /// List valid host-owned builder names in deterministic order.
    pub fn list_host() -> Result<Vec<String>, String> {
        let directory = remote_host_config_dir()?;
        match ensure_existing_real_directory(&directory) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(format!("cannot inspect remote builders: {error}")),
        }
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(format!("cannot list remote builders: {error}")),
        };
        let mut names = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| format!("cannot list remote builders: {error}"))?;
            let metadata = fs::symlink_metadata(entry.path())
                .map_err(|error| format!("cannot inspect remote builder record: {error}"))?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                continue;
            }
            let entry_path = entry.path();
            let Some(name) = entry_path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_suffix(".conf"))
            else {
                continue;
            };
            if validate_remote_name(name, "builder").is_ok() {
                names.push(name.to_string());
            }
        }
        names.sort();
        Ok(names)
    }

    /// Remove one host-owned builder binding.
    pub fn remove_host(builder: &str) -> Result<(), String> {
        validate_remote_name(builder, "builder")?;
        let (record_root, path) = remote_host_record_path(builder)?;
        ensure_existing_real_directory(&record_root)
            .map_err(|error| format!("cannot inspect remote builder records: {error}"))?;
        let absolute_path = record_root.join(&path);
        match fs::symlink_metadata(&absolute_path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                Err(format!("remote builder record `{builder}` is not a regular file"))
            }
            Ok(_) => fs::remove_file(&absolute_path)
                .map_err(|error| format!("cannot remove remote builder `{builder}`: {error}")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Err(format!("remote builder `{builder}` is not bound"))
            }
            Err(error) => Err(format!("cannot remove remote builder `{builder}`: {error}")),
        }
    }

    fn save_host(&self, credential_provider: &Path) -> Result<(), String> {
        let (record_root, path) = remote_host_record_path(&self.builder)?;
        let provider = absolute_host_path(credential_provider.to_path_buf())?;
        let fields = [
            ("version", "1".to_string()),
            ("builder", self.builder.clone()),
            ("root", self.root.to_string_lossy().into_owned()),
            ("credential_provider", provider.to_string_lossy().into_owned()),
            ("trust_domain", self.trust_domain.clone()),
            ("worker_id", self.worker_id.clone()),
            ("platform", self.platform.clone()),
            ("abi", self.abi.clone()),
            ("cache_read", bool_host_value(self.cache_read).to_string()),
            ("cache_write", bool_host_value(self.cache_write).to_string()),
            ("execute", bool_host_value(self.execute).to_string()),
            ("fallback_local", bool_host_value(self.fallback_local).to_string()),
            ("timeout_ms", self.timeout_ms.to_string()),
        ];
        for (_, value) in fields.iter().skip(1) {
            if value.is_empty() || value.chars().any(|character| character.is_control()) {
                return Err("remote binding record contains an empty or control-valued field".to_string());
            }
        }
        let mut record = String::new();
        for (field, value) in fields {
            record.push_str(field);
            record.push('=');
            record.push_str(&value);
            record.push('\n');
        }
        ensure_real_directory(&record_root)
            .map_err(|error| format!("cannot create remote binding directory: {error}"))?;
        atomic_restore_file(&record_root, &record_root.join(&path), record.as_bytes())
            .map_err(|error| format!("cannot save remote builder `{}`: {error}", self.builder))
    }
}

const MAX_REMOTE_CREDENTIAL_BYTES: usize = 4096;

fn absolute_host_path(path: PathBuf) -> Result<PathBuf, String> {
    if path.is_absolute() {
        Ok(path)
    } else {
        std::env::current_dir()
            .map(|directory| directory.join(path))
            .map_err(|error| format!("cannot resolve host path: {error}"))
    }
}

fn remote_host_config_dir() -> Result<PathBuf, String> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or_else(|| "cannot locate the host config directory".to_string())?;
    let base = absolute_host_path(base)?;
    Ok(base.join("jet").join("remote-bindings"))
}

fn remote_host_record_path(builder: &str) -> Result<(PathBuf, PathBuf), String> {
    validate_remote_name(builder, "builder")?;
    Ok((
        remote_host_config_dir()?,
        PathBuf::from(format!("{builder}.conf")),
    ))
}

fn read_remote_credential_provider(path: &Path) -> Result<Vec<u8>, String> {
    let parent = path
        .parent()
        .ok_or_else(|| "credential provider path has no parent".to_string())?;
    ensure_existing_real_directory(parent)
        .map_err(|error| format!("cannot inspect credential provider directory: {error}"))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| "credential provider path has no file name".to_string())?;
    let bytes = secure_read_file_bounded(
        parent,
        &parent.join(file_name),
        MAX_REMOTE_CREDENTIAL_BYTES,
    )
        .map_err(|error| format!("cannot read credential provider `{}`: {error}", path.display()))?;
    if bytes.is_empty() {
        return Err("remote credential provider is empty".to_string());
    }
    RemoteCredential::new(bytes).map(|credential| credential.0).map_err(|error| {
        format!("invalid remote credential provider `{}`: {error}", path.display())
    })
}

fn parse_remote_host_record(
    bytes: &[u8],
    expected_builder: &str,
) -> Result<BTreeMap<String, String>, String> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| "remote binding record is not UTF-8".to_string())?;
    let allowed = [
        "version",
        "builder",
        "root",
        "credential_provider",
        "trust_domain",
        "worker_id",
        "platform",
        "abi",
        "cache_read",
        "cache_write",
        "execute",
        "fallback_local",
        "timeout_ms",
    ];
    let mut fields = BTreeMap::new();
    for line in text.lines() {
        let (field, value) = line
            .split_once('=')
            .ok_or_else(|| "remote binding record has a malformed line".to_string())?;
        if !allowed.contains(&field) || fields.insert(field.to_string(), value.to_string()).is_some() {
            return Err("remote binding record has an unknown or duplicate field".to_string());
        }
        if value.is_empty() || value.chars().any(|character| character.is_control()) {
            return Err(format!("remote binding field `{field}` is empty or contains control text"));
        }
    }
    if fields.get("version").map(String::as_str) != Some("1")
        || fields.get("builder").map(String::as_str) != Some(expected_builder)
    {
        return Err("remote binding record version or builder does not match".to_string());
    }
    for field in allowed.iter().skip(1) {
        if !fields.contains_key(*field) {
            return Err(format!("remote binding record has no `{field}` field"));
        }
    }
    Ok(fields)
}

fn required_host_field<'a>(fields: &'a BTreeMap<String, String>, field: &str) -> Result<&'a str, String> {
    fields
        .get(field)
        .map(String::as_str)
        .ok_or_else(|| format!("remote binding has no `{field}` field"))
}

fn parse_host_bool(fields: &BTreeMap<String, String>, field: &str) -> Result<bool, String> {
    match required_host_field(fields, field)? {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(format!("remote binding `{field}` must be 0 or 1")),
    }
}

fn bool_host_value(value: bool) -> u8 {
    u8::from(value)
}

#[derive(Clone, PartialEq, Eq)]
pub struct RemoteCacheTransport {
    root: PathBuf,
    cas: LocalCas,
    credential: Option<RemoteCredential>,
    worker_identity: Option<RemoteWorkerIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RemoteWorkerIdentity {
    builder: String,
    trust_domain: String,
    worker_id: String,
    platform: String,
    abi: String,
}

impl std::fmt::Debug for RemoteCacheTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteCacheTransport")
            .field("root", &self.root)
            .field("authenticated", &self.credential.is_some())
            .field("worker_identity", &self.worker_identity)
            .finish()
    }
}

impl RemoteCacheTransport {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        RemoteCacheTransport {
            cas: LocalCas::new(root.join("cas")),
            root,
            credential: None,
            worker_identity: None,
        }
    }

    /// Construct an authenticated cache transport. This form is intentionally
    /// cache-only: remote execution, worker blob exchange, and result
    /// publication require the binding constructor, which binds the worker
    /// identity, platform, ABI, and builder trust domain.
    pub fn authenticated(
        root: impl Into<PathBuf>,
        credential: impl AsRef<[u8]>,
    ) -> Result<Self, String> {
        let root = root.into();
        Ok(Self {
            cas: LocalCas::new(root.join("cas")),
            root,
            credential: Some(RemoteCredential::new(credential)?),
            worker_identity: None,
        })
    }

    pub fn for_binding(binding: &RemoteBuildBinding) -> Result<Self, String> {
        validate_remote_name(&binding.builder, "builder")?;
        validate_remote_name(&binding.trust_domain, "trust domain")?;
        validate_remote_name(&binding.worker_id, "worker id")?;
        validate_remote_name(&binding.platform, "platform")?;
        validate_remote_name(&binding.abi, "ABI")?;
        let mut transport = Self::authenticated(binding.root.clone(), binding.credential.bytes())?;
        transport.worker_identity = Some(RemoteWorkerIdentity {
            builder: binding.builder.clone(),
            trust_domain: binding.trust_domain.clone(),
            worker_id: binding.worker_id.clone(),
            platform: binding.platform.clone(),
            abi: binding.abi.clone(),
        });
        Ok(transport)
    }

    pub fn sandbox_proof(
        &self,
        sandbox_id: impl Into<String>,
        action_key: impl Into<String>,
        provenance_digest: ContentDigest,
    ) -> Result<RemoteSandboxProof, String> {
        let identity = self
            .worker_identity
            .as_ref()
            .ok_or_else(|| "remote transport has no worker identity".to_string())?;
        let sandbox_id = sandbox_id.into();
        let action_key = action_key.into();
        let worker_receipt = self.worker_receipt(
            identity,
            &sandbox_id,
            &action_key,
            &provenance_digest,
        )?;
        Ok(RemoteSandboxProof::new(sandbox_id, action_key, provenance_digest)
            .with_worker_identity(
                identity.worker_id.clone(),
                identity.platform.clone(),
                identity.abi.clone(),
                worker_receipt,
            ))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn require_auth(&self, request: RemoteActionRequest) -> Result<&RemoteCredential, RemoteCacheError> {
        self.credential.as_ref().ok_or(RemoteCacheError::Denied(RemoteCacheDenied {
            request,
            reason: RemoteDeniedReason::MissingAuthentication,
        }))
    }

    fn require_auth_internal(&self) -> Result<&RemoteCredential, RemoteCacheError> {
        self.credential.as_ref().ok_or_else(|| {
            RemoteCacheError::InvalidRecord("remote transport authentication is not configured".to_string())
        })
    }

    fn require_worker_identity(&self) -> Result<&RemoteWorkerIdentity, RemoteCacheError> {
        self.worker_identity.as_ref().ok_or_else(|| {
            RemoteCacheError::InvalidRecord(
                "remote execution requires a bound worker identity, platform, and ABI"
                    .to_string(),
            )
        })
    }

    fn validate_policy_identity(
        &self,
        policy: &RemoteCachePolicy,
    ) -> Result<(), RemoteCacheError> {
        let Some(expected) = &self.worker_identity else {
            return Ok(());
        };
        let Some(proof) = policy.proof() else {
            return Err(RemoteCacheError::InvalidRecord(
                "remote worker identity requires a sandbox proof".to_string(),
            ));
        };
        self.validate_worker_identity(proof, expected)
    }

    fn validate_worker_identity(
        &self,
        proof: &RemoteSandboxProof,
        expected: &RemoteWorkerIdentity,
    ) -> Result<(), RemoteCacheError> {
        let prefix = format!("remote:{}:{}:", expected.builder, expected.trust_domain);
        if !proof.sandbox_id.starts_with(&prefix)
            || proof.worker_id != expected.worker_id
            || proof.platform != expected.platform
            || proof.abi != expected.abi
            || proof.worker_receipt
                != self.worker_receipt(
                    expected,
                    &proof.sandbox_id,
                    &proof.action_key,
                    &proof.provenance_digest,
                )
                .map_err(RemoteCacheError::InvalidRecord)?
        {
            Err(RemoteCacheError::InvalidRecord(format!(
                "remote sandbox identity does not match builder `{}`, worker `{}`, platform `{}`, or ABI `{}`",
                expected.builder, expected.worker_id, expected.platform, expected.abi
            )))
        } else {
            Ok(())
        }
    }

    fn worker_receipt(
        &self,
        identity: &RemoteWorkerIdentity,
        sandbox_id: &str,
        action_key: &str,
        provenance_digest: &ContentDigest,
    ) -> Result<String, String> {
        let credential = self
            .credential
            .as_ref()
            .ok_or_else(|| "remote transport authentication is not configured".to_string())?;
        let payload = format!(
            "builder={}\ntrust={}\nworker={}\nplatform={}\nabi={}\nsandbox={}\naction={}\nprovenance={}",
            identity.builder,
            identity.trust_domain,
            identity.worker_id,
            identity.platform,
            identity.abi,
            sandbox_id,
            action_key,
            provenance_digest.as_str(),
        );
        Ok(remote_mac(credential.bytes(), "worker-receipt", payload.as_bytes()))
    }

    fn seal(&self, kind: &str, payload: &[u8]) -> Result<Vec<u8>, RemoteCacheError> {
        self.seal_limited(kind, payload, MAX_REMOTE_WIRE_BYTES)
    }

    fn seal_limited(
        &self,
        kind: &str,
        payload: &[u8],
        limit: usize,
    ) -> Result<Vec<u8>, RemoteCacheError> {
        let credential = self.require_auth_internal()?;
        if payload.len() > limit {
            return Err(RemoteCacheError::InvalidRecord(format!(
                "remote {kind} payload exceeds {limit} bytes"
            )));
        }
        let mac = remote_mac(credential.bytes(), kind, payload);
        let mut wire = format!(
            "JET-REMOTE/1\nkind={kind}\nmac={mac}\nlen={}\n\n",
            payload.len()
        )
        .into_bytes();
        wire.extend_from_slice(payload);
        Ok(wire)
    }

    fn open(&self, kind: &str, wire: &[u8]) -> Result<Vec<u8>, RemoteCacheError> {
        self.open_limited(kind, wire, MAX_REMOTE_WIRE_BYTES)
    }

    fn open_limited(
        &self,
        kind: &str,
        wire: &[u8],
        limit: usize,
    ) -> Result<Vec<u8>, RemoteCacheError> {
        let credential = self.require_auth_internal()?;
        if wire.len() > limit + 256 {
            return Err(RemoteCacheError::InvalidRecord(format!(
                "remote {kind} envelope exceeds {limit} bytes"
            )));
        }
        let Some(separator) = wire.windows(2).position(|pair| pair == b"\n\n") else {
            return Err(RemoteCacheError::InvalidRecord(
                "remote envelope header is incomplete".to_string(),
            ));
        };
        let header = std::str::from_utf8(&wire[..separator])
            .map_err(|_| RemoteCacheError::InvalidRecord("remote envelope header is not UTF-8".to_string()))?;
        let payload = &wire[separator + 2..];
        if payload.len() > limit {
            return Err(RemoteCacheError::InvalidRecord(format!(
                "remote {kind} payload exceeds {limit} bytes"
            )));
        }
        let mut version = None;
        let mut found_kind = None;
        let mut mac = None;
        let mut len = None;
        for (index, line) in header.lines().enumerate() {
            if index == 0 {
                if line != "JET-REMOTE/1" {
                    return Err(RemoteCacheError::InvalidRecord("unsupported remote envelope version".to_string()));
                }
                version = Some(());
                continue;
            }
            let (field, value) = line.split_once('=').ok_or_else(|| {
                RemoteCacheError::InvalidRecord("remote envelope field is malformed".to_string())
            })?;
            match field {
                "kind" if found_kind.is_none() => found_kind = Some(value),
                "mac" if mac.is_none() => mac = Some(value),
                "len" if len.is_none() => {
                    len = Some(value.parse::<usize>().map_err(|_| {
                        RemoteCacheError::InvalidRecord("remote envelope length is invalid".to_string())
                    })?)
                }
                _ => return Err(RemoteCacheError::InvalidRecord("remote envelope has duplicate or unknown fields".to_string())),
            }
        }
        if version.is_none() || found_kind != Some(kind) || len != Some(payload.len()) {
            return Err(RemoteCacheError::InvalidRecord("remote envelope identity is invalid".to_string()));
        }
        let expected = remote_mac(credential.bytes(), kind, payload);
        let actual = mac.ok_or_else(|| RemoteCacheError::InvalidRecord("remote envelope has no MAC".to_string()))?;
        if !constant_time_equal(actual.as_bytes(), expected.as_bytes()) {
            return Err(RemoteCacheError::InvalidRecord("remote envelope authentication failed".to_string()));
        }
        Ok(payload.to_vec())
    }

    fn put_remote_blob(&self, bytes: &[u8]) -> Result<ContentDigest, RemoteCacheError> {
        let digest = ContentDigest::from_bytes(bytes);
        let path = self.remote_blob_path(&digest)?;
        let wire = self.seal_limited("blob", bytes, MAX_REMOTE_BLOB_BYTES)?;
        ensure_real_directory(self.cas.root())?;
        atomic_restore_file(self.cas.root(), &path, &wire)?;
        Ok(digest)
    }

    fn read_remote_blob(&self, digest: &ContentDigest) -> Result<Vec<u8>, RemoteCacheError> {
        let path = self.remote_blob_path(digest)?;
        let wire = secure_read_file_bounded(
            self.cas.root(),
            &path,
            MAX_REMOTE_BLOB_BYTES + 256,
        )?;
        let bytes = self.open_limited("blob", &wire, MAX_REMOTE_BLOB_BYTES)?;
        if &ContentDigest::from_bytes(&bytes) != digest {
            return Err(RemoteCacheError::InvalidRecord(
                "remote CAS blob digest mismatch".to_string(),
            ));
        }
        Ok(bytes)
    }

    fn read_remote_blob_with_len(
        &self,
        digest: &ContentDigest,
        byte_len: u64,
    ) -> Result<Vec<u8>, RemoteCacheError> {
        let bytes = self.read_remote_blob(digest)?;
        if bytes.len() as u64 != byte_len {
            return Err(RemoteCacheError::InvalidRecord(format!(
                "remote CAS blob length mismatch: expected {byte_len}, got {}",
                bytes.len()
            )));
        }
        Ok(bytes)
    }

    fn remote_blob_path(&self, digest: &ContentDigest) -> Result<PathBuf, RemoteCacheError> {
        let digest = ContentDigest::parse(digest.as_str())?;
        let hex = digest
            .as_str()
            .strip_prefix("sha256:")
            .ok_or_else(|| RemoteCacheError::InvalidRecord("remote CAS digest has no sha256 prefix".to_string()))?;
        Ok(self.cas.root()
            .join("blobs")
            .join("sha256")
            .join(&hex[..2])
            .join(&hex[2..]))
    }

    pub fn upload_blob(
        &self,
        bytes: &[u8],
        policy: &RemoteCachePolicy,
    ) -> Result<ContentDigest, RemoteCacheError> {
        policy
            .check(RemoteActionRequest::CacheWrite)
            .map_err(RemoteCacheError::Denied)?;
        self.require_auth(RemoteActionRequest::CacheWrite)?;
        self.validate_policy_identity(policy)?;
        self.put_remote_blob(bytes)
    }

    /// Upload a blob as part of an execution exchange. An execute grant is
    /// distinct from a cache-write grant: a worker must be able to return its
    /// declared outputs without also being allowed to publish cache records.
    pub fn upload_execution_blob(
        &self,
        bytes: &[u8],
        policy: &RemoteCachePolicy,
    ) -> Result<ContentDigest, RemoteCacheError> {
        policy
            .check(RemoteActionRequest::Execute)
            .map_err(RemoteCacheError::Denied)?;
        self.require_auth(RemoteActionRequest::Execute)?;
        self.require_worker_identity()?;
        self.validate_policy_identity(policy)?;
        self.put_remote_blob(bytes)
    }

    pub fn download_blob(
        &self,
        digest: &ContentDigest,
        policy: &RemoteCachePolicy,
    ) -> Result<Vec<u8>, RemoteCacheError> {
        policy
            .check(RemoteActionRequest::CacheRead)
            .map_err(RemoteCacheError::Denied)?;
        self.require_auth(RemoteActionRequest::CacheRead)?;
        self.validate_policy_identity(policy)?;
        self.read_remote_blob(digest)
    }

    /// Fetch a blob named by a remote execution result. This is part of the
    /// execution exchange, not a cache hit, so an execute-only grant may read
    /// it without also enabling remote cache reads.
    pub fn download_execution_blob(
        &self,
        digest: &ContentDigest,
        policy: &RemoteCachePolicy,
    ) -> Result<Vec<u8>, RemoteCacheError> {
        policy
            .check(RemoteActionRequest::Execute)
            .map_err(RemoteCacheError::Denied)?;
        self.require_auth(RemoteActionRequest::Execute)?;
        self.require_worker_identity()?;
        self.validate_policy_identity(policy)?;
        self.read_remote_blob(digest)
    }

    pub fn upload_action_record(
        &self,
        record: &ActionResultRecord,
        policy: &RemoteCachePolicy,
    ) -> Result<(), RemoteCacheError> {
        policy
            .check(RemoteActionRequest::CacheWrite)
            .map_err(RemoteCacheError::Denied)?;
        self.require_auth(RemoteActionRequest::CacheWrite)?;
        self.validate_policy_identity(policy)?;
        self.validate_proof_for_key(policy, &record.key, RemoteActionRequest::CacheWrite)?;
        validate_action_key(&record.key)?;
        validate_remote_count(record.outputs.len(), "cache outputs")?;
        for output in &record.outputs {
            ContentDigest::parse(output.digest.as_str())?;
            validate_remote_path(output.path.as_str())?;
        }
        validate_unique_remote_paths(record.outputs.iter().map(|output| output.path.as_str()))?;
        for output in &record.outputs {
            // A record is not publishable until every declared blob is already
            // present and authenticated in the same CAS. This closes the
            // partial-upload window for both cache workers and hostile stores.
            self.read_remote_blob_with_len(&output.digest, output.byte_len)?;
        }
        let path = self.record_path(&record.key)?;
        let bytes = self.seal("cache-record", encode_remote_record(record).as_bytes())?;
        ensure_remote_root(&self.root)?;
        atomic_restore_file(&self.root, &path, &bytes)?;
        Ok(())
    }

    pub fn download_action_record(
        &self,
        key: &ActionKey,
        policy: &RemoteCachePolicy,
    ) -> Result<ActionResultRecord, RemoteCacheError> {
        policy
            .check(RemoteActionRequest::CacheRead)
            .map_err(RemoteCacheError::Denied)?;
        self.require_auth(RemoteActionRequest::CacheRead)?;
        self.validate_policy_identity(policy)?;
        self.validate_proof_for_key(policy, key, RemoteActionRequest::CacheRead)?;
        let path = self.record_path(key)?;
        let bytes = secure_read_file_bounded(
            &self.root,
            &path,
            MAX_REMOTE_WIRE_BYTES + 256,
        )?;
        let record = decode_remote_record(&self.open("cache-record", &bytes)?)?;
        if &record.key != key {
            return Err(RemoteCacheError::InvalidRecord(
                "record key does not match lookup key".to_string(),
            ));
        }
        validate_action_key(&record.key)?;
        validate_remote_count(record.outputs.len(), "cache outputs")?;
        for output in &record.outputs {
            validate_remote_path(output.path.as_str())?;
            ContentDigest::parse(output.digest.as_str())?;
            self.read_remote_blob_with_len(&output.digest, output.byte_len)?;
        }
        validate_unique_remote_paths(record.outputs.iter().map(|output| output.path.as_str()))?;
        Ok(record)
    }

    fn validate_proof_for_key(
        &self,
        policy: &RemoteCachePolicy,
        key: &ActionKey,
        request: RemoteActionRequest,
    ) -> Result<(), RemoteCacheError> {
        if let Some(proof) = policy.proof() {
            if proof.action_key != key.as_str() {
                return Err(RemoteCacheError::Denied(RemoteCacheDenied {
                    request,
                    reason: RemoteDeniedReason::ProofDoesNotMatchAction,
                }));
            }
        }
        Ok(())
    }

    fn validate_sandbox_proof(
        &self,
        policy: &RemoteCachePolicy,
        sandbox: &RemoteSandboxProof,
        request: RemoteActionRequest,
    ) -> Result<(), RemoteCacheError> {
        if let Some(expected) = policy.proof() {
            if expected != sandbox {
                return Err(RemoteCacheError::Denied(RemoteCacheDenied {
                    request,
                    reason: RemoteDeniedReason::ProofDoesNotMatchAction,
                }));
            }
        }
        Ok(())
    }

    fn record_path(&self, key: &ActionKey) -> Result<PathBuf, RemoteCacheError> {
        let digest = ContentDigest::from_bytes(key.as_str().as_bytes());
        let digest = ContentDigest::parse(digest.as_str())?;
        let hex = digest.as_str().strip_prefix("sha256:").ok_or_else(|| {
            RemoteCacheError::InvalidRecord("action key digest has no sha256 prefix".to_string())
        })?;
        Ok(self
            .root
            .join("records")
            .join(&hex[..2])
            .join(&hex[2..]))
    }

    /// Queue one remote execution request. Submission only writes a request;
    /// it never executes locally and never treats a missing remote worker as
    /// success.
    pub fn submit_execution(
        &self,
        request: &RemoteExecutionRequest,
        policy: &RemoteCachePolicy,
    ) -> Result<(), RemoteCacheError> {
        policy
            .check(RemoteActionRequest::Execute)
            .map_err(RemoteCacheError::Denied)?;
        self.require_auth(RemoteActionRequest::Execute)?;
        self.require_worker_identity()?;
        self.validate_policy_identity(policy)?;
        self.validate_proof_for_key(
            policy,
            &request.key,
            RemoteActionRequest::Execute,
        )?;
        self.validate_sandbox_proof(policy, &request.sandbox, RemoteActionRequest::Execute)?;
        if let Some(expected) = &self.worker_identity {
            self.validate_worker_identity(&request.sandbox, expected)?;
        }
        validate_remote_execution_request(request)?;
        for input in &request.inputs {
            self.read_remote_blob_with_len(&input.digest, input.byte_len)?;
        }
        ensure_remote_root(&self.root)?;
        let _commit = remote_execution_commit_lock()?;
        let result_path = self.execution_result_path(&request.key)?;
        let cancelled_path = self.execution_cancel_path(&request.key)?;
        remove_remote_execution_file(&cancelled_path)?;
        if let Ok(metadata) = fs::symlink_metadata(&result_path) {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(RemoteCacheError::Io(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "remote execution result is not a regular file",
                )));
            }
            fs::remove_file(&result_path)?;
        }
        let path = self.execution_request_path(&request.key)?;
        let bytes = self.seal(
            "execution-request",
            encode_remote_execution_request(request).as_bytes(),
        )?;
        atomic_restore_file(&self.root, &path, &bytes)?;
        Ok(())
    }

    /// Cancel a queued execution and invalidate any late worker result. The
    /// authenticated cancellation marker remains until the next submission
    /// for the action, so a worker that already read the request cannot publish
    /// after the deadline.
    pub fn cancel_execution(
        &self,
        key: &ActionKey,
        policy: &RemoteCachePolicy,
    ) -> Result<(), RemoteCacheError> {
        policy
            .check(RemoteActionRequest::Execute)
            .map_err(RemoteCacheError::Denied)?;
        self.require_auth(RemoteActionRequest::Execute)?;
        self.require_worker_identity()?;
        self.validate_policy_identity(policy)?;
        self.validate_proof_for_key(policy, key, RemoteActionRequest::Execute)?;
        ensure_remote_root(&self.root)?;
        let _commit = remote_execution_commit_lock()?;
        let marker = self.execution_cancel_path(key)?;
        let bytes = self.seal("execution-cancel", key.as_str().as_bytes())?;
        atomic_restore_file(&self.root, &marker, &bytes)?;
        remove_remote_execution_file(&self.execution_result_path(key)?)?;
        remove_remote_execution_file(&self.execution_request_path(key)?)?;
        Ok(())
    }

    /// Publish a result produced by a remote worker. The worker must return
    /// the exact action, toolchain, sandbox, and provenance identities that
    /// were submitted; a mismatched result is rejected before it is visible.
    pub fn publish_execution_result(
        &self,
        result: &RemoteExecutionResult,
        policy: &RemoteCachePolicy,
    ) -> Result<(), RemoteCacheError> {
        policy
            .check(RemoteActionRequest::Execute)
            .map_err(RemoteCacheError::Denied)?;
        self.require_auth(RemoteActionRequest::Execute)?;
        self.require_worker_identity()?;
        self.validate_policy_identity(policy)?;
        self.validate_proof_for_key(
            policy,
            &result.key,
            RemoteActionRequest::Execute,
        )?;
        self.validate_sandbox_proof(policy, &result.sandbox, RemoteActionRequest::Execute)?;
        if let Some(expected) = &self.worker_identity {
            self.validate_worker_identity(&result.sandbox, expected)?;
        }
        validate_remote_execution_result(result)?;
        let _commit = remote_execution_commit_lock()?;
        if self.execution_is_cancelled(&result.key)? {
            return Err(RemoteCacheError::InvalidRecord(
                "remote execution was cancelled before this result was published".to_string(),
            ));
        }
        validate_remote_count(result.outputs.len(), "execution outputs")?;
        let request = self.read_execution_request(&result.key)?;
        if self.execution_is_cancelled(&result.key)? {
            return Err(RemoteCacheError::InvalidRecord(
                "remote execution was cancelled before this result was published".to_string(),
            ));
        }
        validate_execution_parity(&request, result)?;
        for output in &result.outputs {
            self.read_remote_blob_with_len(&output.digest, output.byte_len)?;
        }
        ensure_remote_root(&self.root)?;
        let path = self.execution_result_path(&result.key)?;
        let bytes = self.seal(
            "execution-result",
            encode_remote_execution_result(result).as_bytes(),
        )?;
        atomic_restore_file(&self.root, &path, &bytes)?;
        Ok(())
    }

    /// Read a worker result for a submitted action. This is the explicit
    /// remote-execution read path; it does not run a local action on a miss.
    pub fn download_execution_result(
        &self,
        key: &ActionKey,
        policy: &RemoteCachePolicy,
    ) -> Result<RemoteExecutionResult, RemoteCacheError> {
        policy
            .check(RemoteActionRequest::Execute)
            .map_err(RemoteCacheError::Denied)?;
        self.require_auth(RemoteActionRequest::Execute)?;
        self.require_worker_identity()?;
        self.validate_policy_identity(policy)?;
        self.validate_proof_for_key(policy, key, RemoteActionRequest::Execute)?;
        let _commit = remote_execution_commit_lock()?;
        if self.execution_is_cancelled(key)? {
            remove_remote_execution_file(&self.execution_result_path(key)?)?;
            return Err(RemoteCacheError::Io(io::Error::new(
                io::ErrorKind::NotFound,
                "remote execution was cancelled",
            )));
        }
        let request = self.read_execution_request(key)?;
        self.validate_sandbox_proof(policy, &request.sandbox, RemoteActionRequest::Execute)?;
        let path = self.execution_result_path(key)?;
        let bytes = secure_read_file_bounded(
            &self.root,
            &path,
            MAX_REMOTE_WIRE_BYTES + 256,
        )?;
        let result = decode_remote_execution_result(&self.open("execution-result", &bytes)?)?;
        if self.execution_is_cancelled(key)? {
            remove_remote_execution_file(&path)?;
            return Err(RemoteCacheError::Io(io::Error::new(
                io::ErrorKind::NotFound,
                "remote execution was cancelled",
            )));
        }
        if &result.key != key {
            return Err(RemoteCacheError::InvalidRecord(
                "execution result key does not match lookup key".to_string(),
            ));
        }
        validate_execution_parity(&request, &result)?;
        for output in &result.outputs {
            self.read_remote_blob_with_len(&output.digest, output.byte_len)?;
        }
        Ok(result)
    }

    /// Read and validate a queued request for a remote worker. This is the
    /// worker-facing half of the transport; it returns the immutable request
    /// envelope, while the worker must upload declared input/output blobs and
    /// publish a parity-checked result through `publish_execution_result`.
    pub fn read_execution_request(
        &self,
        key: &ActionKey,
    ) -> Result<RemoteExecutionRequest, RemoteCacheError> {
        self.require_auth(RemoteActionRequest::Execute)?;
        self.require_worker_identity()?;
        if self.execution_is_cancelled(key)? {
            return Err(RemoteCacheError::Io(io::Error::new(
                io::ErrorKind::NotFound,
                "remote execution was cancelled",
            )));
        }
        let path = self.execution_request_path(key)?;
        let bytes = secure_read_file_bounded(
            &self.root,
            &path,
            MAX_REMOTE_WIRE_BYTES + 256,
        )?;
        let request = decode_remote_execution_request(&self.open("execution-request", &bytes)?)?;
        if &request.key != key {
            return Err(RemoteCacheError::InvalidRecord(
                "execution request key does not match lookup key".to_string(),
            ));
        }
        if let Some(expected) = &self.worker_identity {
            self.validate_worker_identity(&request.sandbox, expected)?;
        }
        Ok(request)
    }

    fn execution_request_path(&self, key: &ActionKey) -> Result<PathBuf, RemoteCacheError> {
        self.execution_path("requests", key)
    }

    fn execution_result_path(&self, key: &ActionKey) -> Result<PathBuf, RemoteCacheError> {
        self.execution_path("results", key)
    }

    fn execution_cancel_path(&self, key: &ActionKey) -> Result<PathBuf, RemoteCacheError> {
        self.execution_path("cancelled", key)
    }

    fn execution_is_cancelled(&self, key: &ActionKey) -> Result<bool, RemoteCacheError> {
        let path = self.execution_cancel_path(key)?;
        let bytes = match secure_read_file_bounded(&self.root, &path, MAX_REMOTE_WIRE_BYTES + 256) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(RemoteCacheError::Io(error)),
        };
        let payload = self.open("execution-cancel", &bytes)?;
        if payload != key.as_str().as_bytes() {
            return Err(RemoteCacheError::InvalidRecord(
                "remote cancellation marker does not match its action".to_string(),
            ));
        }
        Ok(true)
    }

    fn execution_path(
        &self,
        kind: &str,
        key: &ActionKey,
    ) -> Result<PathBuf, RemoteCacheError> {
        let digest = ContentDigest::from_bytes(key.as_str().as_bytes());
        let hex = digest
            .as_str()
            .strip_prefix("sha256:")
            .ok_or_else(|| {
                RemoteCacheError::InvalidRecord(
                    "execution key digest has no sha256 prefix".to_string(),
                )
            })?;
        Ok(self
            .root
            .join("execution")
            .join(kind)
            .join(&hex[..2])
            .join(&hex[2..]))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteExecutionRequest {
    pub key: ActionKey,
    pub argv: Vec<String>,
    pub inputs: Vec<ActionInputSnapshot>,
    pub outputs: Vec<BuildPath>,
    pub toolchain_digest: ContentDigest,
    pub sandbox: RemoteSandboxProof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteExecutionResult {
    pub key: ActionKey,
    pub outcome: ActionOutcome,
    pub outputs: Vec<ActionOutputRecord>,
    pub toolchain_digest: ContentDigest,
    pub sandbox: RemoteSandboxProof,
}

fn validate_remote_execution_request(
    request: &RemoteExecutionRequest,
) -> Result<(), RemoteCacheError> {
    validate_action_key(&request.key)?;
    validate_remote_count(request.argv.len(), "argv")?;
    validate_remote_count(request.inputs.len(), "inputs")?;
    validate_remote_count(request.outputs.len(), "outputs")?;
    validate_remote_argv(&request.argv)?;
    ContentDigest::parse(request.toolchain_digest.as_str())?;
    if request.sandbox.action_key != request.key.as_str() {
        return Err(RemoteCacheError::Denied(RemoteCacheDenied {
            request: RemoteActionRequest::Execute,
            reason: RemoteDeniedReason::ProofDoesNotMatchAction,
        }));
    }
    if !request.sandbox.is_complete() {
        return Err(RemoteCacheError::Denied(RemoteCacheDenied {
            request: RemoteActionRequest::Execute,
            reason: RemoteDeniedReason::MissingGrantAndSandboxProof,
        }));
    }
    for input in &request.inputs {
        validate_remote_path(input.path.as_str())?;
        ContentDigest::parse(input.digest.as_str())?;
    }
    validate_unique_remote_paths(request.inputs.iter().map(|input| input.path.as_str()))?;
    for output in &request.outputs {
        validate_remote_path(output.as_str())?;
    }
    validate_unique_remote_paths(request.outputs.iter().map(|output| output.as_str()))?;
    Ok(())
}

fn validate_remote_execution_result(
    result: &RemoteExecutionResult,
) -> Result<(), RemoteCacheError> {
    validate_action_key(&result.key)?;
    validate_remote_count(result.outputs.len(), "execution outputs")?;
    ContentDigest::parse(result.toolchain_digest.as_str())?;
    if !result.sandbox.is_complete() {
        return Err(RemoteCacheError::Denied(RemoteCacheDenied {
            request: RemoteActionRequest::Execute,
            reason: RemoteDeniedReason::MissingGrantAndSandboxProof,
        }));
    }
    if result.sandbox.action_key != result.key.as_str() {
        return Err(RemoteCacheError::Denied(RemoteCacheDenied {
            request: RemoteActionRequest::Execute,
            reason: RemoteDeniedReason::ProofDoesNotMatchAction,
        }));
    }
    for output in &result.outputs {
        validate_remote_path(output.path.as_str())?;
        ContentDigest::parse(output.digest.as_str())?;
    }
    validate_unique_remote_paths(result.outputs.iter().map(|output| output.path.as_str()))?;
    Ok(())
}

fn validate_execution_parity(
    request: &RemoteExecutionRequest,
    result: &RemoteExecutionResult,
) -> Result<(), RemoteCacheError> {
    if request.key != result.key
        || request.toolchain_digest != result.toolchain_digest
        || request.sandbox != result.sandbox
    {
        return Err(RemoteCacheError::InvalidRecord(
            "remote execution result changed action, toolchain, or sandbox provenance"
                .to_string(),
        ));
    }
    if request.outputs.len() != result.outputs.len()
        || request
            .outputs
            .iter()
            .zip(&result.outputs)
            .any(|(declared, actual)| declared != &actual.path)
    {
        return Err(RemoteCacheError::InvalidRecord(
            "remote execution result outputs do not match declarations".to_string(),
        ));
    }
    Ok(())
}

fn encode_remote_execution_request(request: &RemoteExecutionRequest) -> String {
    let mut encoded = format!(
        "version=1\nkey={}\ntoolchain={}\nsandbox={}\nproof_action={}\nproof_provenance={}\nworker_id={}\nplatform={}\nabi={}\nworker_receipt={}\nargv={}\n",
        hex_encode(request.key.as_str().as_bytes()),
        request.toolchain_digest.as_str(),
        hex_encode(request.sandbox.sandbox_id.as_bytes()),
        hex_encode(request.sandbox.action_key.as_bytes()),
        request.sandbox.provenance_digest.as_str(),
        hex_encode(request.sandbox.worker_id.as_bytes()),
        hex_encode(request.sandbox.platform.as_bytes()),
        hex_encode(request.sandbox.abi.as_bytes()),
        hex_encode(request.sandbox.worker_receipt.as_bytes()),
        request.argv.len(),
    );
    for arg in &request.argv {
        encoded.push_str("arg=");
        encoded.push_str(&hex_encode(arg.as_bytes()));
        encoded.push('\n');
    }
    encoded.push_str(&format!("inputs={}\n", request.inputs.len()));
    for input in &request.inputs {
        encoded.push_str(&format!(
            "input\t{}\t{}\t{}\n",
            hex_encode(input.path.as_str().as_bytes()),
            input.digest.as_str(),
            input.byte_len
        ));
    }
    encoded.push_str(&format!("outputs={}\n", request.outputs.len()));
    for output in &request.outputs {
        encoded.push_str("output=");
        encoded.push_str(&hex_encode(output.as_str().as_bytes()));
        encoded.push('\n');
    }
    encoded
}

fn decode_remote_execution_request(
    bytes: &[u8],
) -> Result<RemoteExecutionRequest, RemoteCacheError> {
    if bytes.len() > MAX_REMOTE_WIRE_BYTES {
        return Err(RemoteCacheError::InvalidRecord(
            "execution request exceeds the wire-size limit".to_string(),
        ));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| RemoteCacheError::InvalidRecord("execution request is not UTF-8".to_string()))?;
    let mut seen = BTreeSet::new();
    let mut version = None;
    let mut key = None;
    let mut toolchain_digest = None;
    let mut sandbox_id = None;
    let mut proof_action = None;
    let mut proof_provenance = None;
    let mut worker_id = None;
    let mut platform = None;
    let mut abi = None;
    let mut worker_receipt = None;
    let mut argv_count = None;
    let mut argv = Vec::new();
    let mut input_count = None;
    let mut inputs = Vec::new();
    let mut output_count = None;
    let mut outputs = Vec::new();
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("version=") {
            if !seen.insert("version") { return Err(duplicate_remote_field("version")); }
            version = Some(parse_remote_version(value, "execution request")?);
        } else if let Some(value) = line.strip_prefix("key=") {
            if !seen.insert("key") { return Err(duplicate_remote_field("key")); }
            key = Some(ActionKey::new(String::from_utf8(hex_decode(value)?).map_err(|_| {
                RemoteCacheError::InvalidRecord("execution key is not UTF-8".to_string())
            })?));
        } else if let Some(value) = line.strip_prefix("toolchain=") {
            if !seen.insert("toolchain") { return Err(duplicate_remote_field("toolchain")); }
            toolchain_digest = Some(ContentDigest::parse(value)?);
        } else if let Some(value) = line.strip_prefix("sandbox=") {
            if !seen.insert("sandbox") { return Err(duplicate_remote_field("sandbox")); }
            sandbox_id = Some(String::from_utf8(hex_decode(value)?).map_err(|_| {
                RemoteCacheError::InvalidRecord("sandbox id is not UTF-8".to_string())
            })?);
        } else if let Some(value) = line.strip_prefix("proof_action=") {
            if !seen.insert("proof_action") { return Err(duplicate_remote_field("proof_action")); }
            proof_action = Some(String::from_utf8(hex_decode(value)?).map_err(|_| {
                RemoteCacheError::InvalidRecord("proof action key is not UTF-8".to_string())
            })?);
        } else if let Some(value) = line.strip_prefix("proof_provenance=") {
            if !seen.insert("proof_provenance") { return Err(duplicate_remote_field("proof_provenance")); }
            proof_provenance = Some(ContentDigest::parse(value)?);
        } else if let Some(value) = line.strip_prefix("worker_id=") {
            if !seen.insert("worker_id") { return Err(duplicate_remote_field("worker_id")); }
            worker_id = Some(String::from_utf8(hex_decode(value)?).map_err(|_| {
                RemoteCacheError::InvalidRecord("worker id is not UTF-8".to_string())
            })?);
        } else if let Some(value) = line.strip_prefix("platform=") {
            if !seen.insert("platform") { return Err(duplicate_remote_field("platform")); }
            platform = Some(String::from_utf8(hex_decode(value)?).map_err(|_| {
                RemoteCacheError::InvalidRecord("worker platform is not UTF-8".to_string())
            })?);
        } else if let Some(value) = line.strip_prefix("abi=") {
            if !seen.insert("abi") { return Err(duplicate_remote_field("abi")); }
            abi = Some(String::from_utf8(hex_decode(value)?).map_err(|_| {
                RemoteCacheError::InvalidRecord("worker ABI is not UTF-8".to_string())
            })?);
        } else if let Some(value) = line.strip_prefix("worker_receipt=") {
            if !seen.insert("worker_receipt") { return Err(duplicate_remote_field("worker_receipt")); }
            worker_receipt = Some(String::from_utf8(hex_decode(value)?).map_err(|_| {
                RemoteCacheError::InvalidRecord("worker receipt is not UTF-8".to_string())
            })?);
        } else if let Some(value) = line.strip_prefix("argv=") {
            if !seen.insert("argv") { return Err(duplicate_remote_field("argv")); }
            argv_count = Some(parse_remote_count(value, "argv")?);
        } else if let Some(value) = line.strip_prefix("arg=") {
            argv.push(String::from_utf8(hex_decode(value)?).map_err(|_| {
                RemoteCacheError::InvalidRecord("execution argument is not UTF-8".to_string())
            })?);
        } else if let Some(value) = line.strip_prefix("inputs=") {
            if !seen.insert("inputs") { return Err(duplicate_remote_field("inputs")); }
            input_count = Some(parse_remote_count(value, "inputs")?);
        } else if let Some(value) = line.strip_prefix("input\t") {
            let fields = value.split('\t').collect::<Vec<_>>();
            if fields.len() != 3 {
                return Err(RemoteCacheError::InvalidRecord(
                    "execution input needs path, digest, and byte length".to_string(),
                ));
            }
            inputs.push(ActionInputSnapshot {
                path: BuildPath(String::from_utf8(hex_decode(fields[0])?).map_err(|_| {
                    RemoteCacheError::InvalidRecord("execution input path is not UTF-8".to_string())
                })?),
                digest: ContentDigest::parse(fields[1])?,
                byte_len: fields[2].parse::<u64>().map_err(|_| {
                    RemoteCacheError::InvalidRecord("execution input byte length is not a number".to_string())
                })?,
            });
        } else if let Some(value) = line.strip_prefix("outputs=") {
            if !seen.insert("outputs") { return Err(duplicate_remote_field("outputs")); }
            output_count = Some(parse_remote_count(value, "outputs")?);
        } else if let Some(value) = line.strip_prefix("output=") {
            outputs.push(BuildPath(String::from_utf8(hex_decode(value)?).map_err(|_| {
                RemoteCacheError::InvalidRecord("execution output path is not UTF-8".to_string())
            })?));
        } else if !line.trim().is_empty() {
            return Err(RemoteCacheError::InvalidRecord(format!(
                "unknown execution request field `{line}`"
            )));
        }
    }
    if version != Some(1)
        || argv_count != Some(argv.len())
        || input_count != Some(inputs.len())
        || output_count != Some(outputs.len())
    {
        return Err(RemoteCacheError::InvalidRecord(
            "execution request count does not match records".to_string(),
        ));
    }
    let key = key.ok_or_else(|| RemoteCacheError::InvalidRecord("missing execution key".to_string()))?;
    let sandbox = RemoteSandboxProof::new(
        sandbox_id.ok_or_else(|| RemoteCacheError::InvalidRecord("missing sandbox id".to_string()))?,
        proof_action.ok_or_else(|| RemoteCacheError::InvalidRecord("missing proof action key".to_string()))?,
        proof_provenance.ok_or_else(|| RemoteCacheError::InvalidRecord("missing proof provenance".to_string()))?,
    )
    .with_worker_identity(
        worker_id.ok_or_else(|| RemoteCacheError::InvalidRecord("missing worker id".to_string()))?,
        platform.ok_or_else(|| RemoteCacheError::InvalidRecord("missing worker platform".to_string()))?,
        abi.ok_or_else(|| RemoteCacheError::InvalidRecord("missing worker ABI".to_string()))?,
        worker_receipt.ok_or_else(|| RemoteCacheError::InvalidRecord("missing worker receipt".to_string()))?,
    );
    let request = RemoteExecutionRequest {
        key,
        argv,
        inputs,
        outputs,
        toolchain_digest: toolchain_digest
            .ok_or_else(|| RemoteCacheError::InvalidRecord("missing toolchain digest".to_string()))?,
        sandbox,
    };
    validate_remote_execution_request(&request)?;
    Ok(request)
}

fn encode_remote_execution_result(result: &RemoteExecutionResult) -> String {
    let mut encoded = format!(
        "version=1\nkey={}\noutcome={}\ntoolchain={}\nsandbox={}\nproof_action={}\nproof_provenance={}\nworker_id={}\nplatform={}\nabi={}\nworker_receipt={}\noutputs={}\n",
        hex_encode(result.key.as_str().as_bytes()),
        encode_remote_outcome(result.outcome),
        result.toolchain_digest.as_str(),
        hex_encode(result.sandbox.sandbox_id.as_bytes()),
        hex_encode(result.sandbox.action_key.as_bytes()),
        result.sandbox.provenance_digest.as_str(),
        hex_encode(result.sandbox.worker_id.as_bytes()),
        hex_encode(result.sandbox.platform.as_bytes()),
        hex_encode(result.sandbox.abi.as_bytes()),
        hex_encode(result.sandbox.worker_receipt.as_bytes()),
        result.outputs.len(),
    );
    for output in &result.outputs {
        encoded.push_str(&format!(
            "output\t{}\t{}\t{}\n",
            hex_encode(output.path.as_str().as_bytes()),
            output.digest.as_str(),
            output.byte_len
        ));
    }
    encoded
}

fn decode_remote_execution_result(
    bytes: &[u8],
) -> Result<RemoteExecutionResult, RemoteCacheError> {
    if bytes.len() > MAX_REMOTE_WIRE_BYTES {
        return Err(RemoteCacheError::InvalidRecord(
            "execution result exceeds the wire-size limit".to_string(),
        ));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| RemoteCacheError::InvalidRecord("execution result is not UTF-8".to_string()))?;
    let mut seen = BTreeSet::new();
    let mut version = None;
    let mut key = None;
    let mut outcome = None;
    let mut toolchain_digest = None;
    let mut sandbox_id = None;
    let mut proof_action = None;
    let mut proof_provenance = None;
    let mut worker_id = None;
    let mut platform = None;
    let mut abi = None;
    let mut worker_receipt = None;
    let mut output_count = None;
    let mut outputs = Vec::new();
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("version=") {
            if !seen.insert("version") { return Err(duplicate_remote_field("version")); }
            version = Some(parse_remote_version(value, "execution result")?);
        } else if let Some(value) = line.strip_prefix("key=") {
            if !seen.insert("key") { return Err(duplicate_remote_field("key")); }
            key = Some(ActionKey::new(String::from_utf8(hex_decode(value)?).map_err(|_| {
                RemoteCacheError::InvalidRecord("execution result key is not UTF-8".to_string())
            })?));
        } else if let Some(value) = line.strip_prefix("outcome=") {
            if !seen.insert("outcome") { return Err(duplicate_remote_field("outcome")); }
            outcome = Some(parse_remote_outcome(value)?);
        } else if let Some(value) = line.strip_prefix("toolchain=") {
            if !seen.insert("toolchain") { return Err(duplicate_remote_field("toolchain")); }
            toolchain_digest = Some(ContentDigest::parse(value)?);
        } else if let Some(value) = line.strip_prefix("sandbox=") {
            if !seen.insert("sandbox") { return Err(duplicate_remote_field("sandbox")); }
            sandbox_id = Some(String::from_utf8(hex_decode(value)?).map_err(|_| {
                RemoteCacheError::InvalidRecord("execution result sandbox is not UTF-8".to_string())
            })?);
        } else if let Some(value) = line.strip_prefix("proof_action=") {
            if !seen.insert("proof_action") { return Err(duplicate_remote_field("proof_action")); }
            proof_action = Some(String::from_utf8(hex_decode(value)?).map_err(|_| {
                RemoteCacheError::InvalidRecord("execution result action key is not UTF-8".to_string())
            })?);
        } else if let Some(value) = line.strip_prefix("proof_provenance=") {
            if !seen.insert("proof_provenance") { return Err(duplicate_remote_field("proof_provenance")); }
            proof_provenance = Some(ContentDigest::parse(value)?);
        } else if let Some(value) = line.strip_prefix("worker_id=") {
            if !seen.insert("worker_id") { return Err(duplicate_remote_field("worker_id")); }
            worker_id = Some(String::from_utf8(hex_decode(value)?).map_err(|_| {
                RemoteCacheError::InvalidRecord("execution result worker id is not UTF-8".to_string())
            })?);
        } else if let Some(value) = line.strip_prefix("platform=") {
            if !seen.insert("platform") { return Err(duplicate_remote_field("platform")); }
            platform = Some(String::from_utf8(hex_decode(value)?).map_err(|_| {
                RemoteCacheError::InvalidRecord("execution result worker platform is not UTF-8".to_string())
            })?);
        } else if let Some(value) = line.strip_prefix("abi=") {
            if !seen.insert("abi") { return Err(duplicate_remote_field("abi")); }
            abi = Some(String::from_utf8(hex_decode(value)?).map_err(|_| {
                RemoteCacheError::InvalidRecord("execution result worker ABI is not UTF-8".to_string())
            })?);
        } else if let Some(value) = line.strip_prefix("worker_receipt=") {
            if !seen.insert("worker_receipt") { return Err(duplicate_remote_field("worker_receipt")); }
            worker_receipt = Some(String::from_utf8(hex_decode(value)?).map_err(|_| {
                RemoteCacheError::InvalidRecord("execution result worker receipt is not UTF-8".to_string())
            })?);
        } else if let Some(value) = line.strip_prefix("outputs=") {
            if !seen.insert("outputs") { return Err(duplicate_remote_field("outputs")); }
            output_count = Some(parse_remote_count(value, "outputs")?);
        } else if let Some(value) = line.strip_prefix("output\t") {
            let fields = value.split('\t').collect::<Vec<_>>();
            if fields.len() != 3 {
                return Err(RemoteCacheError::InvalidRecord(
                    "execution output needs path, digest, and byte length".to_string(),
                ));
            }
            outputs.push(ActionOutputRecord {
                path: BuildPath(String::from_utf8(hex_decode(fields[0])?).map_err(|_| {
                    RemoteCacheError::InvalidRecord("execution output path is not UTF-8".to_string())
                })?),
                digest: ContentDigest::parse(fields[1])?,
                byte_len: fields[2].parse::<u64>().map_err(|_| {
                    RemoteCacheError::InvalidRecord("execution output byte length is not a number".to_string())
                })?,
            });
        } else if !line.trim().is_empty() {
            return Err(RemoteCacheError::InvalidRecord(format!(
                "unknown execution result field `{line}`"
            )));
        }
    }
    if version != Some(1) || output_count != Some(outputs.len()) {
        return Err(RemoteCacheError::InvalidRecord(
            "execution result version or output count is invalid".to_string(),
        ));
    }
    let key = key.ok_or_else(|| RemoteCacheError::InvalidRecord("missing execution result key".to_string()))?;
    let result = RemoteExecutionResult {
        key,
        outcome: outcome
            .ok_or_else(|| RemoteCacheError::InvalidRecord("missing execution outcome".to_string()))?,
        outputs,
        toolchain_digest: toolchain_digest
            .ok_or_else(|| RemoteCacheError::InvalidRecord("missing execution toolchain digest".to_string()))?,
        sandbox: RemoteSandboxProof::new(
            sandbox_id.ok_or_else(|| RemoteCacheError::InvalidRecord("missing execution sandbox".to_string()))?,
            proof_action.ok_or_else(|| RemoteCacheError::InvalidRecord("missing execution proof action".to_string()))?,
            proof_provenance
                .ok_or_else(|| RemoteCacheError::InvalidRecord("missing execution proof provenance".to_string()))?,
        )
        .with_worker_identity(
            worker_id.ok_or_else(|| RemoteCacheError::InvalidRecord("missing execution worker id".to_string()))?,
            platform.ok_or_else(|| RemoteCacheError::InvalidRecord("missing execution worker platform".to_string()))?,
            abi.ok_or_else(|| RemoteCacheError::InvalidRecord("missing execution worker ABI".to_string()))?,
            worker_receipt.ok_or_else(|| RemoteCacheError::InvalidRecord("missing execution worker receipt".to_string()))?,
        ),
    };
    validate_remote_execution_result(&result)?;
    Ok(result)
}

fn encode_remote_outcome(outcome: ActionOutcome) -> String {
    match outcome {
        ActionOutcome::Succeeded { exit_code } => format!("succeeded:{exit_code}"),
        ActionOutcome::Failed { exit_code } => format!("failed:{exit_code}"),
        ActionOutcome::RestoredFromCache => "restored".to_string(),
    }
}

fn parse_remote_count(value: &str, field: &str) -> Result<usize, RemoteCacheError> {
    let count = value.parse::<usize>().map_err(|_| {
        RemoteCacheError::InvalidRecord(format!("execution {field} count is not a number"))
    })?;
    if count > MAX_REMOTE_ITEMS {
        return Err(RemoteCacheError::InvalidRecord(format!(
            "execution {field} count exceeds {MAX_REMOTE_ITEMS}"
        )));
    }
    Ok(count)
}

fn validate_remote_count(count: usize, field: &str) -> Result<(), RemoteCacheError> {
    if count > MAX_REMOTE_ITEMS {
        return Err(RemoteCacheError::InvalidRecord(format!(
            "remote {field} count exceeds {MAX_REMOTE_ITEMS}"
        )));
    }
    Ok(())
}

fn parse_remote_version(value: &str, record: &str) -> Result<u32, RemoteCacheError> {
    value.parse::<u32>().map_err(|_| {
        RemoteCacheError::InvalidRecord(format!("{record} version is not a number"))
    })
}

fn validate_action_key(key: &ActionKey) -> Result<(), RemoteCacheError> {
    if key.as_str().is_empty()
        || key.as_str().len() > 4096
        || key
            .as_str()
            .chars()
            .any(|character| character == '\n' || character == '\r' || character == '\0')
    {
        return Err(RemoteCacheError::InvalidRecord(
            "action key is empty or contains a record delimiter".to_string(),
        ));
    }
    Ok(())
}

fn validate_remote_name(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 256
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(format!(
            "remote {label} must be 1..256 ASCII letters, digits, `.`, `_`, or `-`"
        ));
    }
    Ok(())
}

fn validate_remote_argv(argv: &[String]) -> Result<(), RemoteCacheError> {
    if argv.is_empty() || argv.iter().any(|arg| arg.trim().is_empty()) {
        return Err(RemoteCacheError::InvalidRecord(
            "remote execution argv must contain a non-empty command and arguments".to_string(),
        ));
    }
    Ok(())
}

fn validate_unique_remote_paths<'a>(
    paths: impl IntoIterator<Item = &'a str>,
) -> Result<(), RemoteCacheError> {
    let mut seen = BTreeSet::new();
    for path in paths {
        if !seen.insert(path) {
            return Err(RemoteCacheError::InvalidRecord(format!(
                "remote execution path `{path}` is declared more than once"
            )));
        }
    }
    Ok(())
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn hex_decode(value: &str) -> Result<Vec<u8>, RemoteCacheError> {
    if value.len() % 2 != 0 {
        return Err(RemoteCacheError::InvalidRecord(
            "hex field has odd length".to_string(),
        ));
    }
    let mut out = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().chunks_exact(2) {
        let high = hex_digit(pair[0]).ok_or_else(|| {
            RemoteCacheError::InvalidRecord("hex field contains a non-hex digit".to_string())
        })?;
        let low = hex_digit(pair[1]).ok_or_else(|| {
            RemoteCacheError::InvalidRecord("hex field contains a non-hex digit".to_string())
        })?;
        out.push((high << 4) | low);
    }
    Ok(out)
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn remote_mac(key: &[u8], kind: &str, payload: &[u8]) -> String {
    // HMAC-SHA256 keeps the envelope authentication independent of the
    // Merkle/content digest and avoids the length-extension weakness of a
    // plain `sha256(secret || message)` construction.
    const BLOCK: usize = 64;
    let mut key_block = [0u8; BLOCK];
    if key.len() > BLOCK {
        key_block[..32].copy_from_slice(&SHA256::sha256(key));
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }
    let mut inner_input = Vec::with_capacity(BLOCK + kind.len() + payload.len() + 2);
    let mut outer_input = Vec::with_capacity(BLOCK + 32);
    for byte in key_block {
        inner_input.push(byte ^ 0x36);
        outer_input.push(byte ^ 0x5c);
    }
    inner_input.extend_from_slice(kind.as_bytes());
    inner_input.push(0);
    inner_input.extend_from_slice(payload);
    outer_input.extend_from_slice(&SHA256::sha256(&inner_input));
    hex_encode(&SHA256::sha256(&outer_input))
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        difference |= u8::from(left.get(index).copied().unwrap_or(0)
            != right.get(index).copied().unwrap_or(0)) as usize;
    }
    difference == 0
}

fn ensure_remote_root(root: &Path) -> io::Result<()> {
    ensure_real_directory(root)
}

fn remove_remote_execution_file(path: &Path) -> Result<(), RemoteCacheError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(RemoteCacheError::Io(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("remote execution path `{}` is not a regular file", path.display()),
            )))
        }
        Ok(_) => fs::remove_file(path).map_err(RemoteCacheError::Io),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(RemoteCacheError::Io(error)),
    }
}

/// Create a directory tree without ever following an existing symlink. The
/// remote transport, CAS, and action-record paths all share this rule so a
/// cache root cannot redirect writes into an unrelated host directory.
pub(super) fn ensure_real_directory(path: &Path) -> io::Result<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(std::path::MAIN_SEPARATOR.to_string()),
            Component::CurDir => continue,
            Component::ParentDir => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "directory path contains `..`",
                ));
            }
            Component::Normal(part) => current.push(part),
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("directory path `{}` is a symlink", current.display()),
                ));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(io::Error::new(
                    io::ErrorKind::NotADirectory,
                    format!("directory path `{}` is not a directory", current.display()),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                match fs::create_dir(&current) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                        let metadata = fs::symlink_metadata(&current)?;
                        if metadata.file_type().is_symlink() || !metadata.is_dir() {
                            return Err(io::Error::new(
                                io::ErrorKind::PermissionDenied,
                                format!("directory path `{}` is not a real directory", current.display()),
                            ));
                        }
                    }
                    Err(error) => return Err(error),
                }
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn ensure_existing_real_directory(path: &Path) -> io::Result<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(std::path::MAIN_SEPARATOR.to_string()),
            Component::CurDir => continue,
            Component::ParentDir => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "directory path contains `..`",
                ));
            }
            Component::Normal(part) => current.push(part),
        }
        let metadata = fs::symlink_metadata(&current)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("directory path `{}` is not a real directory", current.display()),
            ));
        }
    }
    Ok(())
}

fn validate_remote_path(path: &str) -> Result<(), RemoteCacheError> {
    if path.is_empty()
        || path.len() > 4096
        || Path::new(path).is_absolute()
        || path
            .chars()
            .any(|character| matches!(character, '\t' | '\n' | '\r' | '\0'))
        || Path::new(path)
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(RemoteCacheError::InvalidRecord(format!(
            "output path `{path}` is not relative and normal"
        )));
    }
    Ok(())
}

fn encode_remote_record(record: &ActionResultRecord) -> String {
    let outcome = match record.outcome {
        ActionOutcome::Succeeded { exit_code } => format!("succeeded:{exit_code}"),
        ActionOutcome::Failed { exit_code } => format!("failed:{exit_code}"),
        ActionOutcome::RestoredFromCache => "restored".to_string(),
    };
    let status = match record.provenance.status {
        ActionCacheStatus::Hit(reason) => format!("hit:{reason:?}"),
        ActionCacheStatus::Miss(reason) => format!("miss:{reason:?}"),
    };
    let mut encoded = format!(
        "version=1\nkey={}\noutcome={}\nstatus={}\noutputs={}\n",
        hex_encode(record.key.as_str().as_bytes()),
        outcome,
        status,
        record.outputs.len()
    );
    for output in &record.outputs {
        encoded.push_str(&format!(
            "output\t{}\t{}\t{}\n",
            output.path.as_str(),
            output.digest.as_str(),
            output.byte_len
        ));
    }
    encoded
}

fn decode_remote_record(bytes: &[u8]) -> Result<ActionResultRecord, RemoteCacheError> {
    if bytes.len() > MAX_REMOTE_WIRE_BYTES {
        return Err(RemoteCacheError::InvalidRecord(
            "cache record exceeds the wire-size limit".to_string(),
        ));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| RemoteCacheError::InvalidRecord("record is not UTF-8".to_string()))?;
    let mut seen = BTreeSet::new();
    let mut version = None;
    let mut key = None;
    let mut outcome = None;
    let mut status = None;
    let mut output_count = None;
    let mut outputs = Vec::new();
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("version=") {
            if !seen.insert("version") { return Err(duplicate_remote_field("version")); }
            version = Some(parse_remote_version(value, "cache record")?);
        } else if let Some(value) = line.strip_prefix("key=") {
            if !seen.insert("key") { return Err(duplicate_remote_field("key")); }
            key = Some(ActionKey::new(String::from_utf8(hex_decode(value)?).map_err(|_| {
                RemoteCacheError::InvalidRecord("cache record key is not UTF-8".to_string())
            })?));
        } else if let Some(value) = line.strip_prefix("outcome=") {
            if !seen.insert("outcome") { return Err(duplicate_remote_field("outcome")); }
            outcome = Some(parse_remote_outcome(value)?);
        } else if let Some(value) = line.strip_prefix("status=") {
            if !seen.insert("status") { return Err(duplicate_remote_field("status")); }
            status = Some(parse_remote_status(value)?);
        } else if let Some(value) = line.strip_prefix("outputs=") {
            if !seen.insert("outputs") { return Err(duplicate_remote_field("outputs")); }
            output_count = Some(parse_remote_count(value, "cache outputs")?);
        } else if let Some(value) = line.strip_prefix("output\t") {
            let fields = value.split('\t').collect::<Vec<_>>();
            if fields.len() != 3 {
                return Err(RemoteCacheError::InvalidRecord(
                    "output record needs path, digest, and byte length".to_string(),
                ));
            }
            let digest = ContentDigest::parse(fields[1])?;
            let byte_len = fields[2].parse::<u64>().map_err(|_| {
                RemoteCacheError::InvalidRecord("output byte length is not a number".to_string())
            })?;
            outputs.push(ActionOutputRecord {
                path: BuildPath(fields[0].to_string()),
                digest,
                byte_len,
            });
        } else if !line.trim().is_empty() {
            return Err(RemoteCacheError::InvalidRecord(format!(
                "unknown cache record field `{line}`"
            )));
        }
    }
    if version != Some(1) || output_count != Some(outputs.len()) {
        return Err(RemoteCacheError::InvalidRecord(
            "cache record version or output count is invalid".to_string(),
        ));
    }
    let key = key.ok_or_else(|| RemoteCacheError::InvalidRecord("missing action key".to_string()))?;
    validate_action_key(&key)?;
    Ok(ActionResultRecord {
        key,
        outcome: outcome
            .ok_or_else(|| RemoteCacheError::InvalidRecord("missing action outcome".to_string()))?,
        outputs,
        provenance: ActionCacheProvenance {
            status: status
                .ok_or_else(|| RemoteCacheError::InvalidRecord("missing cache status".to_string()))?,
            remote_policy: RemoteCachePolicy::disabled_until_grant_and_sandbox_proof(),
        },
    })
}

fn duplicate_remote_field(field: &str) -> RemoteCacheError {
    RemoteCacheError::InvalidRecord(format!("remote record field `{field}` appears more than once"))
}

fn parse_remote_outcome(value: &str) -> Result<ActionOutcome, RemoteCacheError> {
    if value == "restored" {
        return Ok(ActionOutcome::RestoredFromCache);
    }
    let (kind, code) = value.split_once(':').ok_or_else(|| {
        RemoteCacheError::InvalidRecord("outcome has no exit code".to_string())
    })?;
    let exit_code = code.parse::<i32>().map_err(|_| {
        RemoteCacheError::InvalidRecord("outcome exit code is not a number".to_string())
    })?;
    match kind {
        "succeeded" => Ok(ActionOutcome::Succeeded { exit_code }),
        "failed" => Ok(ActionOutcome::Failed { exit_code }),
        _ => Err(RemoteCacheError::InvalidRecord(
            "unknown action outcome".to_string(),
        )),
    }
}

fn parse_remote_status(value: &str) -> Result<ActionCacheStatus, RemoteCacheError> {
    let (kind, name) = value.split_once(':').ok_or_else(|| {
        RemoteCacheError::InvalidRecord("cache status has no reason".to_string())
    })?;
    match (kind, name) {
        ("hit", "LocalActionRecordMatched") => {
            Ok(ActionCacheStatus::Hit(CacheHitReason::LocalActionRecordMatched))
        }
        ("hit", "DeclaredOutputsRestored") => {
            Ok(ActionCacheStatus::Hit(CacheHitReason::DeclaredOutputsRestored))
        }
        ("miss", "NoLocalActionRecord") => {
            Ok(ActionCacheStatus::Miss(CacheMissReason::NoLocalActionRecord))
        }
        ("miss", "ActionKeyChanged") => {
            Ok(ActionCacheStatus::Miss(CacheMissReason::ActionKeyChanged))
        }
        ("miss", "DeclaredOutputMissing") => {
            Ok(ActionCacheStatus::Miss(CacheMissReason::DeclaredOutputMissing))
        }
        ("miss", "CacheRecordInvalid") => {
            Ok(ActionCacheStatus::Miss(CacheMissReason::CacheRecordInvalid))
        }
        ("miss", "CacheRestoreFailed") => {
            Ok(ActionCacheStatus::Miss(CacheMissReason::CacheRestoreFailed))
        }
        ("miss", "RemoteDenied") => {
            Ok(ActionCacheStatus::Miss(CacheMissReason::RemoteDenied))
        }
        ("miss", "UncachedAction") => {
            Ok(ActionCacheStatus::Miss(CacheMissReason::UncachedAction))
        }
        ("miss", "FrontEndIncomplete") => {
            Ok(ActionCacheStatus::Miss(CacheMissReason::FrontEndIncomplete))
        }
        _ => Err(RemoteCacheError::InvalidRecord(
            "cache status kind does not match reason".to_string(),
        )),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionOutputRecord {
    pub path: BuildPath,
    pub digest: ContentDigest,
    pub byte_len: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionInputSnapshot {
    pub path: BuildPath,
    pub digest: ContentDigest,
    pub byte_len: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionResultRecord {
    pub key: ActionKey,
    pub outcome: ActionOutcome,
    pub outputs: Vec<ActionOutputRecord>,
    pub provenance: ActionCacheProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalCas {
    root: PathBuf,
}

impl LocalCas {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        LocalCas { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn put_blob(&self, bytes: &[u8]) -> io::Result<ContentDigest> {
        let digest = ContentDigest::from_bytes(bytes);
        let path = self.blob_path(&digest)?;
        ensure_real_directory(&self.root)?;
        match secure_read_file(&self.root, &path) {
            Ok(existing) => if ContentDigest::from_bytes(&existing) != digest {
                atomic_restore_file(&self.root, &path, bytes)?;
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                atomic_restore_file(&self.root, &path, bytes)?;
            }
            Err(error) => return Err(error),
        }
        Ok(digest)
    }

    pub fn read_blob(&self, digest: &ContentDigest) -> io::Result<Vec<u8>> {
        let bytes = secure_read_file(&self.root, &self.blob_path(digest)?)?;
        let actual = ContentDigest::from_bytes(&bytes);
        if &actual != digest {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("CAS blob digest mismatch: expected {}", digest.as_str()),
            ));
        }
        Ok(bytes)
    }

    pub fn snapshot_declared_inputs(
        &self,
        base: &Path,
        action: &BuildAction,
    ) -> io::Result<Vec<ActionInputSnapshot>> {
        let mut inputs = Vec::new();
        for input in &action.inputs {
            let path = resolve_under(base, input.as_str())?;
            let bytes = secure_read_file(base, &path)?;
            let digest = self.put_blob(&bytes)?;
            inputs.push(ActionInputSnapshot {
                path: input.clone(),
                digest,
                byte_len: bytes.len() as u64,
            });
        }
        Ok(inputs)
    }

    pub fn capture_declared_outputs(
        &self,
        base: &Path,
        action: &BuildAction,
        key: ActionKey,
        outcome: ActionOutcome,
        provenance: ActionCacheProvenance,
    ) -> io::Result<ActionResultRecord> {
        let mut outputs = Vec::new();
        for output in &action.outputs {
            let path = resolve_under(base, output.as_str())?;
            let bytes = secure_read_file(base, &path)?;
            let digest = self.put_blob(&bytes)?;
            outputs.push(ActionOutputRecord {
                path: output.clone(),
                digest,
                byte_len: bytes.len() as u64,
            });
        }
        Ok(ActionResultRecord {
            key,
            outcome,
            outputs,
            provenance,
        })
    }

    pub fn restore_declared_outputs(
        &self,
        base: &Path,
        record: &ActionResultRecord,
    ) -> io::Result<()> {
        self.restore_outputs(base, record)
    }

    pub fn restore_action_outputs(
        &self,
        base: &Path,
        action: &BuildAction,
        record: &ActionResultRecord,
    ) -> io::Result<()> {
        if record.outputs.len() != action.outputs.len()
            || record.outputs.iter().zip(&action.outputs).any(|(recorded, declared)| recorded.path != *declared)
        {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "cached output record does not exactly match action declarations"));
        }
        self.restore_outputs(base, record)
    }

    fn restore_outputs(&self, base: &Path, record: &ActionResultRecord) -> io::Result<()> {
        for output in &record.outputs {
            let path = resolve_under(base, output.path.as_str())?;
            let bytes = self.read_blob(&output.digest)?;
            atomic_restore_file(base, &path, &bytes)?;
        }
        Ok(())
    }

    fn blob_path(&self, digest: &ContentDigest) -> io::Result<PathBuf> {
        let digest = ContentDigest::parse(digest.as_str())?;
        let hex = digest.0.strip_prefix("sha256:").expect("validated prefix");
        let (prefix, rest) = hex.split_at(2);
        Ok(self.root
            .join("blobs")
            .join("sha256")
            .join(prefix)
            .join(rest))
    }
}

#[cfg(unix)]
pub(super) fn secure_read_file(base: &Path, path: &Path) -> io::Result<Vec<u8>> {
    use std::ffi::CString;
    use std::io::Read;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;
    const O_RDONLY: i32 = 0;
    const O_CLOEXEC: i32 = 0o2000000;
    const O_DIRECTORY: i32 = 0o200000;
    const O_NOFOLLOW: i32 = 0o400000;
    extern "C" { fn openat(dirfd: i32, pathname: *const i8, flags: i32, mode: u32) -> i32; }
    let name = |value: &std::ffi::OsStr| CString::new(value.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in cache path"));
    let relative = path.strip_prefix(base).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "cache path escapes root"))?;
    let file_name = relative.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "cache path has no file name"))?;
    let root = fs::OpenOptions::new().read(true).custom_flags(O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC).open(base)?;
    let mut held = Vec::<OwnedFd>::new();
    let mut dirfd = root.as_raw_fd();
    if let Some(parent) = relative.parent() {
        for component in parent.components() {
            let Component::Normal(part) = component else { continue };
            let part = name(part)?;
            let fd = unsafe { openat(dirfd, part.as_ptr(), O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC, 0) };
            if fd < 0 { return Err(io::Error::last_os_error()); }
            held.push(unsafe { OwnedFd::from_raw_fd(fd) });
            dirfd = held.last().unwrap().as_raw_fd();
        }
    }
    let file_name = name(file_name)?;
    let fd = unsafe { openat(dirfd, file_name.as_ptr(), O_RDONLY | O_NOFOLLOW | O_CLOEXEC, 0) };
    if fd < 0 { return Err(io::Error::last_os_error()); }
    let mut file = unsafe { fs::File::from_raw_fd(fd) };
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "cache entry is not a regular file"));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(not(unix))]
pub(super) fn secure_read_file(base: &Path, path: &Path) -> io::Result<Vec<u8>> {
    let relative = path.strip_prefix(base).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "cache path escapes root"))?;
    let mut current = base.to_path_buf();
    for component in relative.components() {
        let Component::Normal(part) = component else { continue };
        current.push(part);
        if fs::symlink_metadata(&current)?.file_type().is_symlink() {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "cache path contains a symlink"));
        }
    }
    fs::read(current)
}

#[cfg(unix)]
pub(super) fn secure_read_file_bounded(
    base: &Path,
    path: &Path,
    limit: usize,
) -> io::Result<Vec<u8>> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;
    const O_RDONLY: i32 = 0;
    const O_CLOEXEC: i32 = 0o2000000;
    const O_DIRECTORY: i32 = 0o200000;
    const O_NOFOLLOW: i32 = 0o400000;
    extern "C" {
        fn openat(dirfd: i32, pathname: *const i8, flags: i32, mode: u32) -> i32;
    }
    let name = |value: &std::ffi::OsStr| {
        CString::new(value.as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in cache path"))
    };
    let relative = path.strip_prefix(base).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "cache path escapes root")
    })?;
    let file_name = relative.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "cache path has no file name")
    })?;
    let root = fs::OpenOptions::new()
        .read(true)
        .custom_flags(O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
        .open(base)?;
    let mut held = Vec::<OwnedFd>::new();
    let mut dirfd = root.as_raw_fd();
    if let Some(parent) = relative.parent() {
        for component in parent.components() {
            let Component::Normal(part) = component else { continue };
            let part = name(part)?;
            let fd = unsafe {
                openat(
                    dirfd,
                    part.as_ptr(),
                    O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC,
                    0,
                )
            };
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            held.push(unsafe { OwnedFd::from_raw_fd(fd) });
            dirfd = held.last().unwrap().as_raw_fd();
        }
    }
    let file_name = name(file_name)?;
    let fd = unsafe {
        openat(
            dirfd,
            file_name.as_ptr(),
            O_RDONLY | O_NOFOLLOW | O_CLOEXEC,
            0,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let file = unsafe { fs::File::from_raw_fd(fd) };
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "cache entry is not a regular file",
        ));
    }
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "cache entry exceeds its byte limit",
        ));
    }
    Ok(bytes)
}

#[cfg(not(unix))]
pub(super) fn secure_read_file_bounded(
    base: &Path,
    path: &Path,
    limit: usize,
) -> io::Result<Vec<u8>> {
    let relative = path.strip_prefix(base).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "cache path escapes root")
    })?;
    let mut current = base.to_path_buf();
    for component in relative.components() {
        let Component::Normal(part) = component else { continue };
        current.push(part);
        if fs::symlink_metadata(&current)?.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "cache path contains a symlink",
            ));
        }
    }
    let mut file = fs::File::open(current)?;
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "cache entry exceeds its byte limit",
        ));
    }
    Ok(bytes)
}

#[cfg(all(test, unix))]
mod hostile_tests {
    use super::*;
    use super::super::execution_runtime::read_action_record;
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    fn temp(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("jet-cas-{name}-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn put_blob_rejects_cache_root_and_blob_tree_symlinks() {
        let outside = temp("outside");
        let root_parent = temp("root-link");
        symlink(&outside, root_parent.join("cache")).unwrap();
        assert!(LocalCas::new(root_parent.join("cache")).put_blob(b"secret").is_err());
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);

        let root = temp("blob-link");
        let cas = root.join("cache");
        fs::create_dir_all(&cas).unwrap();
        symlink(&outside, cas.join("blobs")).unwrap();
        assert!(LocalCas::new(&cas).put_blob(b"secret").is_err());
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
    }

    #[test]
    fn known_digest_blob_and_action_record_symlinks_are_never_read() {
        let root = temp("read-link");
        let cas = LocalCas::new(root.join("cas"));
        let digest = cas.put_blob(b"host-bytes").unwrap();
        let blob = cas.blob_path(&digest).unwrap();
        fs::remove_file(&blob).unwrap();
        let host = root.join("host");
        fs::write(&host, b"host-bytes").unwrap();
        symlink(&host, &blob).unwrap();
        assert!(cas.read_blob(&digest).is_err());

        let records = root.join("records");
        fs::create_dir_all(&records).unwrap();
        let key = ActionKey("act-sha256:known".to_string());
        let host_record = root.join("host-record");
        fs::write(&host_record, format!("{}\n", key.as_str())).unwrap();
        let record = records.join("known");
        symlink(host_record, &record).unwrap();
        assert!(read_action_record(&records, &record, key).is_err());
    }

    #[test]
    fn concurrent_same_size_restores_use_unique_create_new_temps() {
        let root = temp("concurrent-restore");
        let output = root.join("out");
        let payloads = (0..16).map(|index| format!("payload-{index:08}").into_bytes()).collect::<Vec<_>>();
        std::thread::scope(|scope| {
            let jobs = payloads.iter().map(|payload| {
                let root = &root;
                let output = &output;
                scope.spawn(move || atomic_restore_file(root, output, payload))
            }).collect::<Vec<_>>();
            for job in jobs { job.join().unwrap().unwrap(); }
        });
        let final_bytes = fs::read(&output).unwrap();
        assert!(payloads.contains(&final_bytes));
        assert!(!fs::read_dir(&root).unwrap().flatten().any(|entry| entry.file_name().to_string_lossy().starts_with(".jet-restore-")));
    }
}

#[cfg(unix)]
pub(super) fn atomic_restore_file(base: &Path, path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::ffi::CString;
    use std::io::Write;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;

    const O_RDONLY: i32 = 0;
    const O_WRONLY: i32 = 1;
    const O_CREAT: i32 = 0o100;
    const O_EXCL: i32 = 0o200;
    const O_CLOEXEC: i32 = 0o2000000;
    const O_DIRECTORY: i32 = 0o200000;
    const O_NOFOLLOW: i32 = 0o400000;
    extern "C" {
        fn openat(dirfd: i32, pathname: *const i8, flags: i32, mode: u32) -> i32;
        fn mkdirat(dirfd: i32, pathname: *const i8, mode: u32) -> i32;
        fn unlinkat(dirfd: i32, pathname: *const i8, flags: i32) -> i32;
        fn renameat(olddirfd: i32, oldpath: *const i8, newdirfd: i32, newpath: *const i8) -> i32;
    }
    fn name(value: &std::ffi::OsStr) -> io::Result<CString> {
        CString::new(value.as_bytes()).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in build output path"))
    }

    let relative = path.strip_prefix(base).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "build output escapes root"))?;
    let file_name = relative.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "build output has no file name"))?;
    let root = fs::OpenOptions::new().read(true).custom_flags(O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC).open(base)?;
    let mut held = Vec::<OwnedFd>::new();
    let mut dirfd = root.as_raw_fd();
    if let Some(parent) = relative.parent() {
        for component in parent.components() {
            let Component::Normal(part) = component else { continue };
            let part = name(part)?;
            let mut fd = unsafe { openat(dirfd, part.as_ptr(), O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC, 0) };
            if fd < 0 && io::Error::last_os_error().kind() == io::ErrorKind::NotFound {
                if unsafe { mkdirat(dirfd, part.as_ptr(), 0o755) } != 0 {
                    let error = io::Error::last_os_error();
                    if error.kind() != io::ErrorKind::AlreadyExists { return Err(error); }
                }
                fd = unsafe { openat(dirfd, part.as_ptr(), O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC, 0) };
            }
            if fd < 0 { return Err(io::Error::last_os_error()); }
            held.push(unsafe { OwnedFd::from_raw_fd(fd) });
            dirfd = held.last().unwrap().as_raw_fd();
        }
    }
    let final_name = name(file_name)?;
    let (temp_name, fd) = loop {
        let mut random = [0u8; 16];
        std::io::Read::read_exact(&mut fs::File::open("/dev/urandom")?, &mut random)?;
        let nonce = random.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
        let temp_name = CString::new(format!(".jet-restore-{nonce}")).unwrap();
        let fd = unsafe { openat(dirfd, temp_name.as_ptr(), O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0o600) };
        if fd >= 0 { break (temp_name, fd); }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::AlreadyExists { return Err(error); }
    };
    let mut temp = unsafe { fs::File::from_raw_fd(fd) };
    temp.write_all(bytes)?;
    temp.sync_all()?;
    drop(temp);
    if unsafe { renameat(dirfd, temp_name.as_ptr(), dirfd, final_name.as_ptr()) } != 0 {
        let error = io::Error::last_os_error();
        let _ = unsafe { unlinkat(dirfd, temp_name.as_ptr(), 0) };
        return Err(error);
    }
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn atomic_restore_file(base: &Path, path: &Path, bytes: &[u8]) -> io::Result<()> {
    prepare_output_destination(base, path)?;
    let mut random = [0u8; 16];
    std::io::Read::read_exact(&mut fs::File::open("/dev/urandom")?, &mut random)?;
    let nonce = random.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    let temp = path.with_extension(format!("jet-cache-restore-{nonce}.tmp"));
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&temp)?;
    std::io::Write::write_all(&mut file, bytes)?;
    if fs::symlink_metadata(path).is_ok() { fs::remove_file(path)?; }
    fs::rename(temp, path)
}
