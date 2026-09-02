#![deny(warnings)]

use jet_foundation::JSON::{json_escape, parse_json, JSONValue};
use jet_foundation::SHA256::{sha256, sha256_hex};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub mod runtime;
pub const STORE_VERSION: &str = "jet.store.v1";
pub const STORE_ENV: &str = "JET_STORE_DIR";
pub const DEFAULT_CAP_BYTES: u64 = 20 * 1024 * 1024 * 1024;
pub const DEFAULT_RESERVE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Maximum time a store operation waits for a live lock owner by default.
pub const DEFAULT_LOCK_WAIT: Duration = Duration::from_secs(120);
const LOCK_RETRY: Duration = Duration::from_millis(10);
const LOCK_RETRY_MAX: Duration = Duration::from_millis(250);
const ACTION_MAGIC: &[u8] = b"jet.store.v1\0action\0";
const ARTIFACT_MAGIC: &[u8] = b"jet.store.v1\0artifact\0";
const MAX_JOURNAL_BYTES: u64 = 64 * 1024 * 1024;
static FILE_COUNTER: AtomicU64 = AtomicU64::new(1);

/// A canonical SHA-256 digest used by every local artifact namespace.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Digest([u8; 32]);

impl Digest {
    pub fn hash(bytes: &[u8]) -> Self {
        Self(sha256(bytes))
    }

    pub fn from_hex(value: &str) -> Result<Self, StoreError> {
        if value.len() != 64 {
            return Err(StoreError::InvalidDigest(value.to_string()));
        }
        let bytes = value.as_bytes();
        let mut digest = [0u8; 32];
        for index in 0..32 {
            let high = hex_value(bytes[index * 2])
                .ok_or_else(|| StoreError::InvalidDigest(value.to_string()))?;
            let low = hex_value(bytes[index * 2 + 1])
                .ok_or_else(|| StoreError::InvalidDigest(value.to_string()))?;
            digest[index] = (high << 4) | low;
        }
        Ok(Self(digest))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(self) -> String {
        let mut output = String::with_capacity(64);
        for byte in self.0 {
            output.push(hex_digit(byte >> 4));
            output.push(hex_digit(byte & 0x0f));
        }
        output
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&(*self).to_hex())
    }
}

/// A verified immutable blob reference. The length is part of the handle so a
/// caller cannot accidentally consume a digest with a different payload size.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ObjectHandle {
    digest: Digest,
    length: u64,
}

impl ObjectHandle {
    pub fn new(digest: Digest, length: u64) -> Self {
        Self { digest, length }
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self::new(Digest::hash(bytes), bytes.len() as u64)
    }

    pub fn digest(self) -> Digest {
        self.digest
    }

    pub fn length(self) -> u64 {
        self.length
    }

    pub fn key(self) -> String {
        self.digest.to_hex()
    }
}

impl fmt::Display for ObjectHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}", self.digest, self.length)
    }
}

/// A typed action-cache key. Action records are immutable per key and carry a
/// second digest in their envelope so replacement with another valid record is
/// still detected as corruption.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ActionHandle {
    key: Digest,
}

impl ActionHandle {
    pub fn new(key: Digest) -> Self {
        Self { key }
    }

    pub fn from_key(key: &str) -> Self {
        Self::from_bytes(key.as_bytes())
    }

    pub fn from_bytes(key: &[u8]) -> Self {
        Self::new(Digest::hash(key))
    }

    pub fn digest(self) -> Digest {
        self.key
    }

    pub fn key(self) -> String {
        self.key.to_hex()
    }
}

impl fmt::Display for ActionHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.key.fmt(formatter)
    }
}

/// A validated `lto/<namespace>/` directory name.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LtoNamespace(String);

impl LtoNamespace {
    pub fn new(value: &str) -> Result<Self, StoreError> {
        validate_component(value, "LTO namespace")?;
        Ok(Self(value.to_string()))
    }

    pub fn name(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for LtoNamespace {
    fn as_ref(&self) -> &str {
        self.name()
    }
}

impl fmt::Display for LtoNamespace {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

/// An artifact that a live build lease pins from pruning.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum LeaseTarget {
    Blob(ObjectHandle),
    Action(ActionHandle),
    Lto {
        namespace: LtoNamespace,
        object: ObjectHandle,
    },
}

impl LeaseTarget {
    pub fn blob(object: ObjectHandle) -> Self {
        Self::Blob(object)
    }

    pub fn action(action: ActionHandle) -> Self {
        Self::Action(action)
    }

    pub fn lto(namespace: &str, object: ObjectHandle) -> Result<Self, StoreError> {
        Ok(Self::Lto {
            namespace: LtoNamespace::new(namespace)?,
            object,
        })
    }

    fn entry_key(&self) -> EntryKey {
        match self {
            Self::Blob(object) => EntryKey::Blob(object.key()),
            Self::Action(action) => EntryKey::Action(action.key()),
            Self::Lto { namespace, object } => EntryKey::Lto(namespace.name().to_string(), object.key()),
        }
    }
}

/// The machine-wide store configuration. The optional cap is an explicit host
/// override; without it the effective limit is adaptive. Lock acquisition
/// waits up to [`DEFAULT_LOCK_WAIT`] unless overridden.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreConfig {
    root: PathBuf,
    cap_bytes: Option<u64>,
    reserve_bytes: u64,
    lock_wait: Duration,
}

impl StoreConfig {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            cap_bytes: None,
            reserve_bytes: DEFAULT_RESERVE_BYTES,
            lock_wait: DEFAULT_LOCK_WAIT,
        }
    }

    pub fn from_env() -> Result<Self, StoreError> {
        let root = match std::env::var_os(STORE_ENV) {
            Some(value) if !value.is_empty() => PathBuf::from(value),
            Some(_) => return Err(StoreError::Config(format!("{STORE_ENV} cannot be empty"))),
            None => default_store_root()?,
        };
        let cap_bytes = parse_optional_size("JET_STORE_CAP_BYTES")?;
        let reserve_bytes = parse_optional_size("JET_STORE_RESERVE_BYTES")?.unwrap_or(DEFAULT_RESERVE_BYTES);
        Ok(Self {
            root,
            cap_bytes,
            reserve_bytes,
            lock_wait: DEFAULT_LOCK_WAIT,
        })
    }

    pub fn with_cap_bytes(mut self, cap_bytes: u64) -> Self {
        self.cap_bytes = Some(cap_bytes);
        self
    }

    pub fn with_reserve_bytes(mut self, reserve_bytes: u64) -> Self {
        self.reserve_bytes = reserve_bytes;
        self
    }

    /// Set the maximum time to wait for a live lock owner.
    pub fn with_lock_wait(mut self, lock_wait: Duration) -> Self {
        self.lock_wait = lock_wait;
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn cap_bytes(&self) -> Option<u64> {
        self.cap_bytes
    }

    pub fn reserve_bytes(&self) -> u64 {
        self.reserve_bytes
    }

    pub fn lock_wait(&self) -> Duration {
        self.lock_wait
    }
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self::from_env().unwrap_or_else(|_| Self::new(PathBuf::from(".cache").join("jet/store")))
    }
}

/// Ownership identity recorded in lock and lease files. PID alone is not
/// sufficient because a later process may reuse it.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProcessIdentity {
    pid: u32,
    start: String,
}

impl ProcessIdentity {
    pub fn current() -> Self {
        current_process_identity()
    }

    pub fn new(pid: u32, start: impl Into<String>) -> Self {
        Self {
            pid,
            start: start.into(),
        }
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn start(&self) -> &str {
        &self.start
    }
}

/// A deterministic store listing entry.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct EntryStatus {
    pub kind: EntryKind,
    pub key: String,
    pub namespace: Option<String>,
    pub size_bytes: u64,
    pub last_use: u64,
    pub pinned: bool,
    pub path: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum EntryKind {
    Blob,
    Action,
    Lto,
}

/// Store state suitable for status UIs and deterministic tests.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreStatus {
    pub root: PathBuf,
    pub footprint_bytes: u64,
    pub limit_bytes: u64,
    pub reserve_bytes: u64,
    pub available_bytes: Option<u64>,
    pub live_leases: usize,
    pub host_limit_bytes: Option<u64>,
    pub entries: Vec<EntryStatus>,
    pub tiers: Vec<String>,
}

/// The result of one deterministic prune operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PruneReport {
    pub target_bytes: u64,
    pub before_bytes: u64,
    pub after_bytes: u64,
    pub removed: Vec<EntryStatus>,
    pub pinned_bytes: u64,
    pub blocked: bool,
}

/// A verified action record that points at one immutable CAS object.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredArtifact {
    pub action: ActionHandle,
    pub object: ObjectHandle,
    pub path: PathBuf,
    pub executable: bool,
}
/// One compiler-owned node execution recorded for `explain-build`.
#[derive(Clone, Debug, PartialEq)]
pub struct BuildNodeRecord {
    pub kind: String,
    pub key: String,
    pub subject: String,
    pub duration_ms: f64,
    pub why_ran: String,
    pub inputs: Vec<String>,
}

/// Immutable per-build node evidence stored in the machine-wide CAS.
#[derive(Clone, Debug, PartialEq)]
pub struct BuildRecord {
    pub schema: String,
    pub program: String,
    pub nodes: Vec<BuildNodeRecord>,
}

impl BuildRecord {
    pub const SCHEMA: &'static str = "jet.build-record/v1";

    pub fn new(program: impl Into<String>, nodes: Vec<BuildNodeRecord>) -> Self {
        Self {
            schema: Self::SCHEMA.to_string(),
            program: program.into(),
            nodes,
        }
    }

    /// The CAS payload is JSON so tools can inspect it without a second codec.
    pub fn to_json(&self) -> String {
        let mut output = format!(
            "{{\"schema\":\"{}\",\"program\":\"{}\",\"nodes\":[",
            json_escape(&self.schema),
            json_escape(&self.program)
        );
        for (index, node) in self.nodes.iter().enumerate() {
            if index != 0 {
                output.push(',');
            }
            output.push_str(&format!(
                "{{\"kind\":\"{}\",\"key\":\"{}\",\"subject\":\"{}\",\"duration_ms\":{},\"why_ran\":\"{}\",\"inputs\":[",
                json_escape(&node.kind),
                json_escape(&node.key),
                json_escape(&node.subject),
                if node.duration_ms.is_finite() && node.duration_ms >= 0.0 {
                    node.duration_ms
                } else {
                    0.0
                },
                json_escape(&node.why_ran)
            ));
            for (input_index, input) in node.inputs.iter().enumerate() {
                if input_index != 0 {
                    output.push(',');
                }
                output.push('"');
                output.push_str(&json_escape(input));
                output.push('"');
            }
            output.push_str("]}");
        }
        output.push_str("]}");
        output
    }

    pub fn from_json(payload: &str) -> Result<Self, String> {
        let value = parse_json(payload).map_err(|_| "build record is not valid JSON".to_string())?;
        let object = value
            .as_object()
            .map_err(|error| format!("build record must be an object: {error}"))?;
        let schema = object
            .get("schema")
            .ok_or_else(|| "build record is missing schema".to_string())?
            .as_str()
            .map_err(|error| format!("build record schema is invalid: {error}"))?
            .to_string();
        if schema != Self::SCHEMA {
            return Err(format!("unsupported build record schema `{schema}`"));
        }
        let program = object
            .get("program")
            .ok_or_else(|| "build record is missing program".to_string())?
            .as_str()
            .map_err(|error| format!("build record program is invalid: {error}"))?
            .to_string();
        let values = object
            .get("nodes")
            .ok_or_else(|| "build record is missing nodes".to_string())?
            .as_array()
            .map_err(|error| format!("build record nodes are invalid: {error}"))?;
        let mut nodes = Vec::with_capacity(values.len());
        for value in values {
            let node = value
                .as_object()
                .map_err(|error| format!("build record node is invalid: {error}"))?;
            let string_field = |name: &str| -> Result<String, String> {
                node.get(name)
                    .ok_or_else(|| format!("build record node is missing {name}"))?
                    .as_str()
                    .map(|value| value.to_string())
                    .map_err(|error| format!("build record node {name} is invalid: {error}"))
            };
            let duration_ms = match node
                .get("duration_ms")
                .ok_or_else(|| "build record node is missing duration_ms".to_string())?
            {
                JSONValue::Number(value) => *value as f64,
                JSONValue::Flt(value) => *value,
                _ => return Err("build record node duration_ms is invalid".to_string()),
            };
            if !duration_ms.is_finite() || duration_ms < 0.0 {
                return Err("build record node duration_ms is out of range".to_string());
            }
            let inputs = node
                .get("inputs")
                .ok_or_else(|| "build record node is missing inputs".to_string())?
                .as_array()
                .map_err(|error| format!("build record node inputs are invalid: {error}"))?
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .map(|value| value.to_string())
                        .map_err(|error| format!("build record node input is invalid: {error}"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            nodes.push(BuildNodeRecord {
                kind: string_field("kind")?,
                key: string_field("key")?,
                subject: string_field("subject")?,
                duration_ms,
                why_ran: string_field("why_ran")?,
                inputs,
            });
        }
        Ok(Self {
            schema,
            program,
            nodes,
        })
    }
}

/// Result of looking up a named artifact. Corrupt records and objects are
/// quarantined before `Corrupt` is returned.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArtifactLookup {
    Missing,
    Corrupt,
    Hit(StoredArtifact),
}

/// Result of restoring a named artifact to a display path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArtifactRestore {
    Missing,
    Corrupt,
    Hit {
        object: ObjectHandle,
        executable: bool,
    },
}

#[derive(Debug)]
pub enum StoreError {
    Io(io::Error),
    Config(String),
    InvalidDigest(String),
    InvalidKey(String),
    Corrupt { path: PathBuf, reason: String },
    Conflict { path: PathBuf },
    Locked {
        path: PathBuf,
        owner_pid: Option<u32>,
        elapsed: Duration,
    },
    Capacity {
        path: PathBuf,
        needed_bytes: u64,
        available_bytes: Option<u64>,
        limit_bytes: u64,
    },
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::Config(message) => formatter.write_str(message),
            Self::InvalidDigest(value) => write!(formatter, "invalid SHA-256 digest `{value}`"),
            Self::InvalidKey(value) => write!(formatter, "invalid store key `{value}`"),
            Self::Corrupt { path, reason } => {
                write!(formatter, "corrupt store entry {}: {reason}", path.display())
            }
            Self::Conflict { path } => write!(formatter, "immutable store entry already differs: {}", path.display()),
            Self::Locked {
                path,
                owner_pid,
                elapsed,
            } => {
                let owner = owner_pid.map_or_else(|| "unknown".to_string(), |pid| pid.to_string());
                write!(
                    formatter,
                    "store lock is owned by a live process (owner pid {owner}) after waiting {:.3}s: {}",
                    elapsed.as_secs_f64(),
                    path.display()
                )
            }
            Self::Capacity {
                path,
                needed_bytes,
                available_bytes,
                limit_bytes,
            } => write!(
                formatter,
                "store cannot admit {} bytes for {} (available {:?}, limit {})",
                needed_bytes,
                path.display(),
                available_bytes,
                limit_bytes
            ),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for StoreError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// The local content-addressed artifact store.
#[derive(Clone, Debug)]
pub struct Store {
    config: StoreConfig,
}

impl Store {
    pub fn open(config: StoreConfig) -> Result<Self, StoreError> {
        if config.root.as_os_str().is_empty() {
            return Err(StoreError::Config("store root cannot be empty".to_string()));
        }
        let store = Self { config };
        store.ensure_layout()?;
        Ok(store)
    }

    pub fn new(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        Self::open(StoreConfig::new(root))
    }

    pub fn from_env() -> Result<Self, StoreError> {
        Self::open(StoreConfig::from_env()?)
    }

    pub fn config(&self) -> &StoreConfig {
        &self.config
    }

    pub fn root(&self) -> &Path {
        self.config.root()
    }

    pub fn limit(&self) -> u64 {
        self.limit_with_disk(filesystem_stats(self.root()).map(|stats| stats.total_bytes))
    }

    pub fn effective_limit(&self) -> u64 {
        self.limit()
    }

    pub fn object_path(&self, object: &ObjectHandle) -> PathBuf {
        self.root().join("cas").join(object.key())
    }

    pub fn action_path(&self, action: &ActionHandle) -> PathBuf {
        self.root().join("ac").join(action.key())
    }

    pub fn lto_path<N: AsRef<str>>(&self, namespace: N, object: &ObjectHandle) -> PathBuf {
        self.root().join("lto").join(namespace.as_ref()).join(object.key())
    }

    pub fn publish_blob(&self, bytes: &[u8]) -> Result<ObjectHandle, StoreError> {
        let object = ObjectHandle::from_bytes(bytes);
        self.publish_object(object, bytes)?;
        Ok(object)
    }

    pub fn publish_object(&self, object: ObjectHandle, bytes: &[u8]) -> Result<(), StoreError> {
        let actual = ObjectHandle::from_bytes(bytes);
        if actual != object {
            return Err(StoreError::Corrupt {
                path: self.object_path(&object),
                reason: format!("published bytes are {actual}, expected {object}"),
            });
        }
        self.publish_data(EntryKey::Blob(object.key()), self.object_path(&object), bytes)
    }

    pub fn get_blob(&self, object: &ObjectHandle) -> Result<Option<Vec<u8>>, StoreError> {
        let key = EntryKey::Blob(object.key());
        let path = self.object_path(object);
        match read_file(&path)? {
            RawRead::Missing => Ok(None),
            RawRead::Corrupt(reason) => {
                self.quarantine_after_read(&key, &path, &reason);
                Ok(None)
            }
            RawRead::Bytes(bytes) => {
                if bytes.len() as u64 != object.length() || Digest::hash(&bytes) != object.digest() {
                    self.quarantine_after_read(&key, &path, "digest or length mismatch");
                    return Ok(None);
                }
                self.touch_after_read(&key, bytes.len() as u64);
                Ok(Some(bytes))
            }
        }
    }
    /// Read a blob when only its digest is available. This is the lookup used
    /// by BuildRecord pointers; the bytes remain verified before returning.
    pub fn blob_by_digest(&self, digest: &Digest) -> Result<Option<Vec<u8>>, StoreError> {
        let key = EntryKey::Blob(digest.to_hex());
        let path = self.root().join("cas").join(digest.to_hex());
        match read_file(&path)? {
            RawRead::Missing => Ok(None),
            RawRead::Corrupt(reason) => {
                self.quarantine_after_read(&key, &path, &reason);
                Ok(None)
            }
            RawRead::Bytes(bytes) => {
                if Digest::hash(&bytes) != *digest {
                    self.quarantine_after_read(&key, &path, "digest mismatch");
                    return Ok(None);
                }
                self.touch_after_read(&key, bytes.len() as u64);
                Ok(Some(bytes))
            }
        }
    }
    /// Publish one immutable build record through the ordinary blob CAS.
    ///
    /// `key` is the caller's content key. The small index pointer is mutable
    /// only so `latest_build_record` can find the newest content key.
    pub fn publish_build_record(
        &self,
        key: &str,
        record: &BuildRecord,
    ) -> Result<ObjectHandle, StoreError> {
        let object = self.publish_blob(record.to_json().as_bytes())?;
        self.ensure_layout()?;
        let pointer = self.build_record_pointer_path(key);
        if let Some(existing) = self.read_build_pointer(&pointer)? {
            if existing != object {
                if self.build_record(key)?.as_ref() != Some(record) {
                    return Err(StoreError::Conflict { path: pointer });
                }
            }
        }
        let latest = self.latest_build_record_pointer_path(&record.program);
        let _lock = self.acquire_lock()?;
        atomic_write(&pointer, object.to_string().as_bytes())?;
        atomic_write(&latest, key.as_bytes())?;
        Ok(object)
    }

    /// Load an immutable build record addressed by its content key.
    pub fn build_record(&self, key: &str) -> Result<Option<BuildRecord>, StoreError> {
        let pointer = self.build_record_pointer_path(key);
        let Some(object) = self.read_build_pointer(&pointer)? else {
            return Ok(None);
        };
        let Some(bytes) = self.get_blob(&object)? else {
            return Ok(None);
        };
        let payload = String::from_utf8(bytes).map_err(|_| StoreError::Corrupt {
            path: pointer,
            reason: "build record blob is not UTF-8".to_string(),
        })?;
        BuildRecord::from_json(&payload)
            .map(Some)
            .map_err(|reason| StoreError::Corrupt {
                path: self.build_record_pointer_path(key),
                reason,
            })
    }

    /// Load the most recent record for a logical program path.
    pub fn latest_build_record(&self, program: &str) -> Result<Option<BuildRecord>, StoreError> {
        let path = self.latest_build_record_pointer_path(program);
        let key = match read_file(&path)? {
            RawRead::Missing => return Ok(None),
            RawRead::Corrupt(reason) => {
                return Err(StoreError::Corrupt { path, reason });
            }
            RawRead::Bytes(bytes) => String::from_utf8(bytes)
                .map_err(|_| StoreError::Corrupt {
                    path: path.clone(),
                    reason: "latest build pointer is not UTF-8".to_string(),
                })?
                .trim()
                .to_string(),
        };
        self.build_record(&key)
    }

    fn build_record_pointer_path(&self, key: &str) -> PathBuf {
        self.root()
            .join("build-records")
            .join("records")
            .join(Digest::hash(key.as_bytes()).to_hex())
    }

    fn latest_build_record_pointer_path(&self, program: &str) -> PathBuf {
        self.root()
            .join("build-records")
            .join("latest")
            .join(Digest::hash(program.as_bytes()).to_hex())
    }

    fn read_build_pointer(&self, path: &Path) -> Result<Option<ObjectHandle>, StoreError> {
        let bytes = match read_file(path)? {
            RawRead::Missing => return Ok(None),
            RawRead::Corrupt(reason) => {
                return Err(StoreError::Corrupt {
                    path: path.to_path_buf(),
                    reason,
                });
            }
            RawRead::Bytes(bytes) => bytes,
        };
        let value = String::from_utf8(bytes)
            .map_err(|_| StoreError::Corrupt {
                path: path.to_path_buf(),
                reason: "build record pointer is not UTF-8".to_string(),
            })?
            .trim()
            .to_string();
        let (digest, length) = value.split_once(':').ok_or_else(|| StoreError::Corrupt {
            path: path.to_path_buf(),
            reason: "build record pointer has invalid handle".to_string(),
        })?;
        let digest = Digest::from_hex(digest)?;
        let length = length.parse::<u64>().map_err(|_| StoreError::Corrupt {
            path: path.to_path_buf(),
            reason: "build record pointer has invalid length".to_string(),
        })?;
        Ok(Some(ObjectHandle::new(digest, length)))
    }

    pub fn get_object(&self, object: &ObjectHandle) -> Result<Option<Vec<u8>>, StoreError> {
        self.get_blob(object)
    }

    pub fn contains(&self, object: &ObjectHandle) -> Result<bool, StoreError> {
        Ok(self.get_blob(object)?.is_some())
    }

    pub fn publish_action(&self, action: ActionHandle, record: &[u8]) -> Result<(), StoreError> {
        let path = self.action_path(&action);
        let key = EntryKey::Action(action.key());
        let encoded = encode_action(action, record);
        self.ensure_layout()?;
        let _lock = self.acquire_lock()?;
        match read_file(&path)? {
            RawRead::Missing => {}
            RawRead::Corrupt(reason) => self.quarantine_locked(&key, &path, &reason)?,
            RawRead::Bytes(existing) => match decode_action(action, &existing) {
                Ok(value) if value == record => {
                    self.append_journal_locked("touch", &key, existing.len() as u64)?;
                    return Ok(());
                }
                Ok(_) => return Err(StoreError::Conflict { path }),
                Err(reason) => self.quarantine_locked(&key, &path, &reason)?,
            },
        }
        self.ensure_capacity_locked(encoded.len() as u64, &path)?;
        atomic_write(&path, &encoded)?;
        self.append_journal_locked("put", &key, encoded.len() as u64)
    }

    pub fn get_action(&self, action: &ActionHandle) -> Result<Option<Vec<u8>>, StoreError> {
        let key = EntryKey::Action(action.key());
        let path = self.action_path(action);
        match read_file(&path)? {
            RawRead::Missing => Ok(None),
            RawRead::Corrupt(reason) => {
                self.quarantine_after_read(&key, &path, &reason);
                Ok(None)
            }
            RawRead::Bytes(bytes) => match decode_action(*action, &bytes) {
                Ok(record) => {
                    self.touch_after_read(&key, bytes.len() as u64);
                    Ok(Some(record))
                }
                Err(reason) => {
                    self.quarantine_after_read(&key, &path, &reason);
                    Ok(None)
                }
            },
        }
    }

    pub fn action(&self, key: &str) -> ActionHandle {
        ActionHandle::from_key(key)
    }

    pub fn publish_lto<N: AsRef<str>>(&self, namespace: N, bytes: &[u8]) -> Result<ObjectHandle, StoreError> {
        let namespace = LtoNamespace::new(namespace.as_ref())?;
        let object = ObjectHandle::from_bytes(bytes);
        let key = EntryKey::Lto(namespace.name().to_string(), object.key());
        self.publish_data(key, self.lto_path(&namespace, &object), bytes)?;
        Ok(object)
    }

    pub fn get_lto<N: AsRef<str>>(
        &self,
        namespace: N,
        object: &ObjectHandle,
    ) -> Result<Option<Vec<u8>>, StoreError> {
        let namespace = LtoNamespace::new(namespace.as_ref())?;
        let key = EntryKey::Lto(namespace.name().to_string(), object.key());
        let path = self.lto_path(&namespace, object);
        match read_file(&path)? {
            RawRead::Missing => Ok(None),
            RawRead::Corrupt(reason) => {
                self.quarantine_after_read(&key, &path, &reason);
                Ok(None)
            }
            RawRead::Bytes(bytes) => {
                if bytes.len() as u64 != object.length() || Digest::hash(&bytes) != object.digest() {
                    self.quarantine_after_read(&key, &path, "digest or length mismatch");
                    return Ok(None);
                }
                self.touch_after_read(&key, bytes.len() as u64);
                Ok(Some(bytes))
            }
        }
    }

    pub fn lto_namespace(&self, namespace: &str) -> Result<LtoNamespace, StoreError> {
        LtoNamespace::new(namespace)
    }

    pub fn acquire_lease(&self, objects: &[ObjectHandle]) -> Result<Lease, StoreError> {
        let targets: Vec<LeaseTarget> = objects.iter().copied().map(LeaseTarget::Blob).collect();
        self.acquire_artifact_lease(&targets)
    }

    pub fn lease(&self, objects: &[ObjectHandle]) -> Result<Lease, StoreError> {
        self.acquire_lease(objects)
    }

    pub fn acquire_artifact_lease(&self, targets: &[LeaseTarget]) -> Result<Lease, StoreError> {
        self.ensure_layout()?;
        let _lock = self.acquire_lock()?;
        let owner = ProcessIdentity::current();
        let path = self
            .root()
            .join("leases")
            .join(format!("lease-{}-{}", owner.pid(), next_counter()));
        let mut keys = BTreeSet::new();
        for target in targets {
            keys.insert(target.entry_key());
        }
        let mut bytes = format!("{STORE_VERSION}|lease|{}|{}\n", owner.pid(), owner.start()).into_bytes();
        for key in keys {
            let (kind, value) = key.wire();
            bytes.extend_from_slice(format!("{kind}|{value}\n").as_bytes());
        }
        create_new_synced(&path, &bytes)?;
        sync_dir(path.parent().expect("lease path has parent"))?;
        Ok(Lease { path, owner })
    }

    pub fn status(&self) -> Result<StoreStatus, StoreError> {
        self.ensure_layout()?;
        let _lock = self.acquire_lock()?;
        self.status_locked()
    }

    /// Return the persisted machine-level cap, if one was configured.
    pub fn host_limit(&self) -> Result<Option<u64>, StoreError> {
        let path = self.host_limit_path();
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(StoreError::Io(error)),
        };
        let value = std::str::from_utf8(&bytes)
            .map_err(|_| StoreError::Config(format!("host limit is not valid UTF-8: {}", path.display())))?
            .trim();
        if value.is_empty() {
            return Err(StoreError::Config(format!("host limit is empty: {}", path.display())));
        }
        Ok(Some(parse_size(value)?))
    }

    /// Persist or clear the stricter machine-level cap.
    pub fn set_host_limit(&self, limit_bytes: Option<u64>) -> Result<(), StoreError> {
        self.ensure_layout()?;
        let _lock = self.acquire_lock()?;
        let path = self.host_limit_path();
        match limit_bytes {
            Some(limit) => atomic_write(&path, format!("{limit}\n").as_bytes()),
            None => match fs::remove_file(&path) {
                Ok(()) => sync_dir(self.root()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(StoreError::Io(error)),
            },
        }
    }

    pub fn prune(&self, target_bytes: Option<u64>) -> Result<PruneReport, StoreError> {
        self.ensure_layout()?;
        let _lock = self.acquire_lock()?;
        let target = target_bytes.unwrap_or_else(|| self.limit());
        self.prune_locked(target)
    }

    pub fn prune_to(&self, target_bytes: u64) -> Result<PruneReport, StoreError> {
        self.prune(Some(target_bytes))
    }

    /// Publish a file as one CAS blob and an immutable action record keyed by
    /// the caller's already-computed lower-case SHA-256 build key.
    pub fn publish_file(&self, key: &str, source: &Path) -> Result<StoredArtifact, StoreError> {
        let action = action_for_key(key)?;
        let metadata = fs::symlink_metadata(source)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(StoreError::Corrupt {
                path: source.to_path_buf(),
                reason: "published artifact is not a regular file".to_string(),
            });
        }
        let bytes = fs::read(source)?;
        let object = self.publish_blob(&bytes)?;
        let record = encode_artifact(object, is_executable(&metadata));
        self.publish_action(action, &record)?;
        match self.lookup_artifact(key)? {
            ArtifactLookup::Hit(artifact) => Ok(artifact),
            ArtifactLookup::Missing | ArtifactLookup::Corrupt => Err(StoreError::Corrupt {
                path: self.action_path(&action),
                reason: "published artifact disappeared before verification".to_string(),
            }),
        }
    }

    /// Verify and locate a named artifact without copying it.
    pub fn lookup_artifact(&self, key: &str) -> Result<ArtifactLookup, StoreError> {
        let action = action_for_key(key)?;
        let action_path = self.action_path(&action);
        let record = match read_file(&action_path)? {
            RawRead::Missing => return Ok(ArtifactLookup::Missing),
            RawRead::Corrupt(reason) => {
                self.quarantine_after_read(&EntryKey::Action(action.key()), &action_path, &reason);
                return Ok(ArtifactLookup::Corrupt);
            }
            RawRead::Bytes(bytes) => match decode_action(action, &bytes) {
                Ok(record) => record,
                Err(reason) => {
                    self.quarantine_after_read(&EntryKey::Action(action.key()), &action_path, &reason);
                    return Ok(ArtifactLookup::Corrupt);
                }
            },
        };
        let Some((object, executable)) = decode_artifact(&record) else {
            self.quarantine_after_read(
                &EntryKey::Action(action.key()),
                &action_path,
                "artifact record has an unknown version or short header",
            );
            return Ok(ArtifactLookup::Corrupt);
        };
        let object_path = self.object_path(&object);
        match read_file(&object_path)? {
            RawRead::Missing => {
                self.quarantine_after_read(
                    &EntryKey::Action(action.key()),
                    &action_path,
                    "artifact record references a missing object",
                );
                Ok(ArtifactLookup::Corrupt)
            }
            RawRead::Corrupt(reason) => {
                self.quarantine_after_read(&EntryKey::Blob(object.key()), &object_path, &reason);
                self.quarantine_after_read(
                    &EntryKey::Action(action.key()),
                    &action_path,
                    "artifact record references a corrupt object",
                );
                Ok(ArtifactLookup::Corrupt)
            }
            RawRead::Bytes(bytes) => {
                if bytes.len() as u64 != object.length() || Digest::hash(&bytes) != object.digest() {
                    self.quarantine_after_read(
                        &EntryKey::Blob(object.key()),
                        &object_path,
                        "digest or length mismatch",
                    );
                    self.quarantine_after_read(
                        &EntryKey::Action(action.key()),
                        &action_path,
                        "artifact record references a corrupt object",
                    );
                    return Ok(ArtifactLookup::Corrupt);
                }
                self.touch_after_read(&EntryKey::Blob(object.key()), bytes.len() as u64);
                self.touch_after_read(&EntryKey::Action(action.key()), record.len() as u64);
                Ok(ArtifactLookup::Hit(StoredArtifact {
                    action,
                    object,
                    path: object_path,
                    executable,
                }))
            }
        }
    }

    /// Verify and copy a named artifact to `destination`, preserving its
    /// executable bit. Corrupt entries are removed from the active namespace.
    pub fn restore_file(&self, key: &str, destination: &Path) -> Result<ArtifactRestore, StoreError> {
        let lookup = self.lookup_artifact(key)?;
        match lookup {
            ArtifactLookup::Missing => Ok(ArtifactRestore::Missing),
            ArtifactLookup::Corrupt => Ok(ArtifactRestore::Corrupt),
            ArtifactLookup::Hit(artifact) => {
                let _lease = self.acquire_artifact_lease(&[
                    LeaseTarget::Action(artifact.action),
                    LeaseTarget::Blob(artifact.object),
                ])?;
                let bytes = fs::read(&artifact.path)?;
                atomic_write(destination, &bytes)?;
                set_executable(destination, artifact.executable)?;
                Ok(ArtifactRestore::Hit {
                    object: artifact.object,
                    executable: artifact.executable,
                })
            }
        }
    }

    fn publish_data(&self, key: EntryKey, path: PathBuf, bytes: &[u8]) -> Result<(), StoreError> {
        self.ensure_layout()?;
        let _lock = self.acquire_lock()?;
        match read_file(&path)? {
            RawRead::Missing => {}
            RawRead::Corrupt(reason) => self.quarantine_locked(&key, &path, &reason)?,
            RawRead::Bytes(existing) => {
                if existing == bytes {
                    self.append_journal_locked("touch", &key, existing.len() as u64)?;
                    return Ok(());
                }
                self.quarantine_locked(&key, &path, "existing bytes do not match their key")?;
            }
        }
        self.ensure_capacity_locked(bytes.len() as u64, &path)?;
        atomic_write(&path, bytes)?;
        self.append_journal_locked("put", &key, bytes.len() as u64)
    }

    fn status_locked(&self) -> Result<StoreStatus, StoreError> {
        let mut journal = read_journal(&self.root().join("journal"))?;
        if !journal.valid {
            self.compact_journal_locked()?;
            journal = read_journal(&self.root().join("journal"))?;
        }
        let leases = self.read_leases_locked(true)?;
        let pinned: BTreeSet<EntryKey> = leases
            .iter()
            .flat_map(|lease| lease.targets.iter().cloned())
            .collect();
        let mut entries = self.scan_entries()?;
        for entry in &mut entries {
            entry.last_use = journal
                .entries
                .get(&entry.key)
                .map(|meta| meta.last_use)
                .unwrap_or(0);
            entry.pinned = pinned.contains(&entry.key);
        }
        entries.sort_by(|left, right| left.key.cmp(&right.key));
        let footprint = entries
            .iter()
            .fold(0u64, |sum, entry| sum.saturating_add(entry.size_bytes));
        let disk = filesystem_stats(self.root());
        let entries = entries.iter().map(StoredEntry::status).collect();
        Ok(StoreStatus {
            root: self.root().to_path_buf(),
            footprint_bytes: footprint,
            limit_bytes: self.limit_with_disk(disk.as_ref().map(|stats| stats.total_bytes)),
            reserve_bytes: self.config.reserve_bytes(),
            available_bytes: disk.map(|stats| stats.available_bytes),
            live_leases: leases.len(),
            host_limit_bytes: self.host_limit()?,
            entries,
            tiers: vec!["local".to_string()],
        })
    }

    fn prune_locked(&self, target_bytes: u64) -> Result<PruneReport, StoreError> {
        let journal = read_journal(&self.root().join("journal"))?;
        let leases = self.read_leases_locked(true)?;
        let pinned: BTreeSet<EntryKey> = leases
            .iter()
            .flat_map(|lease| lease.targets.iter().cloned())
            .collect();
        let mut entries = self.scan_entries()?;
        for entry in &mut entries {
            entry.last_use = journal
                .entries
                .get(&entry.key)
                .map(|meta| meta.last_use)
                .unwrap_or(0);
            entry.pinned = pinned.contains(&entry.key);
        }
        let before = entries
            .iter()
            .fold(0u64, |sum, entry| sum.saturating_add(entry.size_bytes));
        let pinned_bytes = entries
            .iter()
            .filter(|entry| entry.pinned)
            .fold(0u64, |sum, entry| sum.saturating_add(entry.size_bytes));
        let mut order = entries.clone();
        order.sort_by(|left, right| {
            (left.pinned, left.last_use, &left.key).cmp(&(right.pinned, right.last_use, &right.key))
        });
        let mut after = before;
        let mut removed = Vec::new();
        for entry in order {
            if after <= target_bytes {
                break;
            }
            if entry.pinned {
                continue;
            }
            match fs::remove_file(&entry.path) {
                Ok(()) => {
                    sync_dir(entry.path.parent().expect("store entry has parent"))?;
                    after = after.saturating_sub(entry.size_bytes);
                    self.append_journal_locked("remove", &entry.key, entry.size_bytes)?;
                    removed.push(entry.status());
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(StoreError::Io(error)),
            }
        }
        if !removed.is_empty() || journal.bytes > MAX_JOURNAL_BYTES {
            self.compact_journal_locked()?;
        }
        Ok(PruneReport {
            target_bytes,
            before_bytes: before,
            after_bytes: after,
            removed,
            pinned_bytes,
            blocked: after > target_bytes,
        })
    }

    fn ensure_capacity_locked(&self, incoming: u64, path: &Path) -> Result<(), StoreError> {
        let disk = filesystem_stats(self.root());
        let limit = self.limit_with_disk(disk.as_ref().map(|stats| stats.total_bytes));
        if incoming > limit {
            return Err(StoreError::Capacity {
                path: path.to_path_buf(),
                needed_bytes: incoming,
                available_bytes: disk.map(|stats| stats.available_bytes),
                limit_bytes: limit,
            });
        }
        let footprint = self
            .scan_entries()?
            .iter()
            .fold(0u64, |sum, entry| sum.saturating_add(entry.size_bytes));
        if footprint.saturating_add(incoming) > limit {
            let _ = self.prune_locked(limit.saturating_sub(incoming))?;
        }
        let footprint = self
            .scan_entries()?
            .iter()
            .fold(0u64, |sum, entry| sum.saturating_add(entry.size_bytes));
        let available = disk.map(|stats| stats.available_bytes);
        let disk_needed = incoming
            .saturating_mul(2)
            .saturating_add(self.config.reserve_bytes());
        let disk_ok = available.is_none_or(|free| free >= disk_needed);
        if footprint.saturating_add(incoming) > limit || !disk_ok {
            return Err(StoreError::Capacity {
                path: path.to_path_buf(),
                needed_bytes: incoming,
                available_bytes: available,
                limit_bytes: limit,
            });
        }
        Ok(())
    }
    fn limit_with_disk(&self, total_bytes: Option<u64>) -> u64 {
        let adaptive = total_bytes
            .map(|total| (total / 10).min(DEFAULT_CAP_BYTES).max(1))
            .unwrap_or(DEFAULT_CAP_BYTES);
        let configured = self.config.cap_bytes().unwrap_or(adaptive);
        self.host_limit()
            .ok()
            .flatten()
            .map_or(configured, |host| configured.min(host))
    }

    fn host_limit_path(&self) -> PathBuf {
        self.root().join("host-limit")
    }

    fn ensure_layout(&self) -> Result<(), StoreError> {
        fs::create_dir_all(self.root())?;
        for name in ["cas", "ac", "lto", "locks", "leases", "quarantine", "build-records"] {
            ensure_dir(&self.root().join(name))?;
        }
        ensure_dir(&self.root().join("build-records").join("records"))?;
        ensure_dir(&self.root().join("build-records").join("latest"))?;
        Ok(())
    }
    fn quarantine_after_read(&self, key: &EntryKey, path: &Path, reason: &str) {
        let Ok(_lock) = self.acquire_lock() else {
            return;
        };
        let _ = self.quarantine_locked(key, path, reason);
    }

    fn touch_after_read(&self, key: &EntryKey, length: u64) {
        let Ok(_lock) = self.acquire_lock() else {
            return;
        };
        let _ = self.append_journal_locked("touch", key, length);
    }
    fn quarantine_locked(&self, key: &EntryKey, path: &Path, _reason: &str) -> Result<(), StoreError> {
        if !path.exists() && fs::symlink_metadata(path).is_err() {
            return Ok(());
        }
        let quarantine = self.root().join("quarantine");
        ensure_dir(&quarantine)?;
        let name = format!(
            "{}-{}-{}",
            key.kind_name(),
            key.storage_key(),
            next_counter()
        );
        let destination = quarantine.join(name);
        match fs::rename(path, &destination) {
            Ok(()) => {
                sync_dir(path.parent().expect("quarantine source has parent"))?;
                sync_dir(&quarantine)?;
                let length = fs::metadata(&destination).map(|metadata| metadata.len()).unwrap_or(0);
                self.append_journal_locked("remove", key, length)?;
                Ok(())
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(StoreError::Io(error)),
        }
    }

    fn append_journal_locked(&self, operation: &str, key: &EntryKey, length: u64) -> Result<(), StoreError> {
        let journal_path = self.root().join("journal");
        let mut state = read_journal(&journal_path)?;
        if !state.valid {
            self.compact_journal_locked()?;
            state = read_journal(&journal_path)?;
        }
        let sequence = state.last_sequence.saturating_add(1);
        let (kind, value) = key.wire();
        let prefix = format!("{STORE_VERSION}|{operation}|{kind}|{value}|{length}|{sequence}|");
        let checksum = sha256_hex(prefix.as_bytes());
        let line = format!("{prefix}{checksum}\n");
        append_synced(&journal_path, line.as_bytes())
    }

    fn compact_journal_locked(&self) -> Result<(), StoreError> {
        let journal_path = self.root().join("journal");
        let journal = read_journal(&journal_path)?;
        let mut entries = self.scan_entries()?;
        for entry in &mut entries {
            entry.last_use = journal
                .entries
                .get(&entry.key)
                .map(|meta| meta.last_use)
                .unwrap_or(0);
        }
        entries.sort_by(|left, right| (left.last_use, &left.key).cmp(&(right.last_use, &right.key)));
        let mut contents = Vec::new();
        for (index, entry) in entries.iter().enumerate() {
            let sequence = index as u64 + 1;
            let (kind, value) = entry.key.wire();
            let prefix = format!("{STORE_VERSION}|put|{kind}|{value}|{}|{sequence}|", entry.size_bytes);
            let checksum = sha256_hex(prefix.as_bytes());
            contents.extend_from_slice(format!("{prefix}{checksum}\n").as_bytes());
        }
        atomic_write(&journal_path, &contents)
    }

    fn read_leases_locked(&self, clean_stale: bool) -> Result<Vec<LeaseRecord>, StoreError> {
        let directory = self.root().join("leases");
        let mut paths = sorted_children(&directory)?;
        paths.retain(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("lease-"))
        });
        let mut live = Vec::new();
        for path in paths {
            let bytes = match fs::read(&path) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(StoreError::Io(error)),
            };
            let Some(record) = parse_lease(&path, &bytes) else {
                continue;
            };
            match owner_liveness(&record.owner) {
                Some(false) if clean_stale => {
                    if fs::remove_file(&path).is_ok() {
                        sync_dir(&directory)?;
                    }
                }
                Some(false) => {}
                Some(true) | None => live.push(record),
            }
        }
        Ok(live)
    }

    fn scan_entries(&self) -> Result<Vec<StoredEntry>, StoreError> {
        let mut entries = Vec::new();
        scan_flat_namespace(
            &self.root().join("cas"),
            EntryKind::Blob,
            None,
            &mut entries,
        )?;
        scan_flat_namespace(&self.root().join("ac"), EntryKind::Action, None, &mut entries)?;
        let lto_root = self.root().join("lto");
        for namespace_path in sorted_children(&lto_root)? {
            let metadata = match fs::symlink_metadata(&namespace_path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(StoreError::Io(error)),
            };
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                continue;
            }
            let Some(namespace) = namespace_path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if LtoNamespace::new(namespace).is_err() {
                continue;
            }
            scan_flat_namespace(
                &namespace_path,
                EntryKind::Lto,
                Some(namespace.to_string()),
                &mut entries,
            )?;
        }
        entries.sort_by(|left, right| left.key.cmp(&right.key));
        Ok(entries)
    }


    fn acquire_lock(&self) -> Result<StoreLock, StoreError> {
        let path = self.root().join("locks").join("store.lock");
        let owner = ProcessIdentity::current();
        let owner_line = format!("{STORE_VERSION}|lock|{}|{}\n", owner.pid(), owner.start());
        let started = Instant::now();
        let deadline = started + self.config.lock_wait();
        let mut retry = LOCK_RETRY;
        loop {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    file.write_all(owner_line.as_bytes())?;
                    file.sync_all()?;
                    sync_dir(path.parent().expect("lock path has parent"))?;
                    return Ok(StoreLock {
                        path,
                        owner_line: owner_line.into_bytes(),
                        _file: file,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    let existing = fs::read(&path).ok();
                    let existing_owner = existing.as_deref().and_then(parse_owner_line);
                    let owner_pid = existing_owner.as_ref().map(ProcessIdentity::pid);
                    let is_live = existing_owner
                        .as_ref()
                        .map(|identity| owner_liveness(identity).unwrap_or(true))
                        .unwrap_or(true);
                    if !is_live && fs::remove_file(&path).is_ok() {
                        sync_dir(path.parent().expect("lock path has parent"))?;
                        retry = LOCK_RETRY;
                        continue;
                    }
                    let now = Instant::now();
                    if now >= deadline {
                        return Err(StoreError::Locked {
                            path,
                            owner_pid,
                            elapsed: now.saturating_duration_since(started),
                        });
                    }
                    thread::sleep(retry.min(deadline.saturating_duration_since(now)));
                    retry = retry.saturating_mul(2).min(LOCK_RETRY_MAX);
                }
                Err(error) => return Err(StoreError::Io(error)),
            }
        }
    }
}

/// A live lease. Dropping it releases the file only if its ownership record is
/// unchanged; a later process can therefore never remove a live lease by PID
/// coincidence.
pub struct Lease {
    path: PathBuf,
    owner: ProcessIdentity,
}

impl Lease {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn owner(&self) -> &ProcessIdentity {
        &self.owner
    }

    pub fn is_live(&self) -> bool {
        owner_liveness(&self.owner).unwrap_or(true)
    }
}

impl fmt::Debug for Lease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Lease")
            .field("path", &self.path)
            .field("owner", &self.owner)
            .finish()
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        let expected = format!("{STORE_VERSION}|lease|{}|{}\n", self.owner.pid(), self.owner.start());
        let owned = fs::read(&self.path)
            .ok()
            .is_some_and(|bytes| bytes.starts_with(expected.as_bytes()));
        if owned && fs::remove_file(&self.path).is_ok() {
            if let Some(parent) = self.path.parent() {
                let _ = sync_dir(parent);
            }
        }
    }
}

struct StoreLock {
    path: PathBuf,
    owner_line: Vec<u8>,
    _file: File,
}

impl Drop for StoreLock {
    fn drop(&mut self) {
        if fs::read(&self.path).ok().as_deref() == Some(self.owner_line.as_slice()) {
            if fs::remove_file(&self.path).is_ok() {
                if let Some(parent) = self.path.parent() {
                    let _ = sync_dir(parent);
                }
            }
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum EntryKey {
    Blob(String),
    Action(String),
    Lto(String, String),
}

impl EntryKey {
    fn kind(&self) -> EntryKind {
        match self {
            Self::Blob(_) => EntryKind::Blob,
            Self::Action(_) => EntryKind::Action,
            Self::Lto(_, _) => EntryKind::Lto,
        }
    }

    fn kind_name(&self) -> &'static str {
        match self {
            Self::Blob(_) => "blob",
            Self::Action(_) => "action",
            Self::Lto(_, _) => "lto",
        }
    }

    fn storage_key(&self) -> String {
        match self {
            Self::Blob(key) | Self::Action(key) => key.clone(),
            Self::Lto(namespace, key) => format!("{namespace}-{key}"),
        }
    }

    fn wire(&self) -> (&'static str, String) {
        match self {
            Self::Blob(key) => ("blob", key.clone()),
            Self::Action(key) => ("action", key.clone()),
            Self::Lto(namespace, key) => ("lto", format!("{namespace}~{key}")),
        }
    }
}

#[derive(Clone, Debug)]
struct StoredEntry {
    key: EntryKey,
    path: PathBuf,
    size_bytes: u64,
    last_use: u64,
    pinned: bool,
}

impl StoredEntry {
    fn status(&self) -> EntryStatus {
        let (key, namespace) = match &self.key {
            EntryKey::Blob(key) | EntryKey::Action(key) => (key.clone(), None),
            EntryKey::Lto(namespace, key) => (key.clone(), Some(namespace.clone())),
        };
        EntryStatus {
            kind: self.key.kind(),
            key,
            namespace,
            size_bytes: self.size_bytes,
            last_use: self.last_use,
            pinned: self.pinned,
            path: self.path.clone(),
        }
    }
}

#[derive(Clone, Debug)]
struct LeaseRecord {
    owner: ProcessIdentity,
    targets: Vec<EntryKey>,
}

#[derive(Clone, Debug, Default)]
struct JournalState {
    entries: BTreeMap<EntryKey, JournalMeta>,
    last_sequence: u64,
    bytes: u64,
    valid: bool,
}

#[derive(Clone, Debug)]
struct JournalMeta {
    last_use: u64,
}

#[derive(Clone, Debug)]
struct DiskStats {
    total_bytes: u64,
    available_bytes: u64,
}

enum RawRead {
    Missing,
    Bytes(Vec<u8>),
    Corrupt(String),
}

fn read_file(path: &Path) -> Result<RawRead, StoreError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(RawRead::Missing),
        Err(error) => return Err(StoreError::Io(error)),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Ok(RawRead::Corrupt("entry is not a regular file".to_string()));
    }
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(RawRead::Corrupt("entry disappeared while opening".to_string()));
        }
        Err(error) => return Err(StoreError::Io(error)),
    };
    let held = file.metadata()?;
    if !same_file(&metadata, &held) {
        return Ok(RawRead::Corrupt("entry changed while opening".to_string()));
    }
    let mut bytes = Vec::with_capacity((held.len() as usize).min(64 * 1024));
    file.read_to_end(&mut bytes)?;
    let after = file.metadata()?;
    let path_after = fs::symlink_metadata(path)?;
    if bytes.len() as u64 != held.len() || !same_file(&held, &after) || !same_file(&held, &path_after) {
        return Ok(RawRead::Corrupt("entry changed while reading".to_string()));
    }
    Ok(RawRead::Bytes(bytes))
}

fn encode_action(action: ActionHandle, record: &[u8]) -> Vec<u8> {
    let digest = sha256(record);
    let mut output = Vec::with_capacity(ACTION_MAGIC.len() + 32 + 8 + 32 + record.len());
    output.extend_from_slice(ACTION_MAGIC);
    output.extend_from_slice(action.digest().as_bytes());
    output.extend_from_slice(&(record.len() as u64).to_le_bytes());
    output.extend_from_slice(&digest);
    output.extend_from_slice(record);
    output
}

fn decode_action(action: ActionHandle, bytes: &[u8]) -> Result<Vec<u8>, String> {
    let header_len = ACTION_MAGIC.len() + 32 + 8 + 32;
    if bytes.len() < header_len || !bytes.starts_with(ACTION_MAGIC) {
        return Err("action record has an unknown version or short header".to_string());
    }
    let key_start = ACTION_MAGIC.len();
    let key_end = key_start + 32;
    if bytes[key_start..key_end] != *action.digest().as_bytes() {
        return Err("action key does not match its path".to_string());
    }
    let length_start = key_end;
    let length_end = length_start + 8;
    let length_bytes: [u8; 8] = bytes[length_start..length_end]
        .try_into()
        .map_err(|_| "action length header is truncated".to_string())?;
    let length = u64::from_le_bytes(length_bytes);
    let digest_start = length_end;
    let digest_end = digest_start + 32;
    let payload = &bytes[digest_end..];
    if length != payload.len() as u64 {
        return Err("action record length does not match its payload".to_string());
    }
    if sha256(payload) != bytes[digest_start..digest_end] {
        return Err("action record payload digest mismatch".to_string());
    }
    Ok(payload.to_vec())
}

fn action_for_key(key: &str) -> Result<ActionHandle, StoreError> {
    Ok(ActionHandle::new(Digest::from_hex(key)?))
}

fn encode_artifact(object: ObjectHandle, executable: bool) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(ARTIFACT_MAGIC.len() + 32 + 8 + 1);
    bytes.extend_from_slice(ARTIFACT_MAGIC);
    bytes.extend_from_slice(object.digest().as_bytes());
    bytes.extend_from_slice(&object.length().to_le_bytes());
    bytes.push(u8::from(executable));
    bytes
}

fn decode_artifact(bytes: &[u8]) -> Option<(ObjectHandle, bool)> {
    let expected = ARTIFACT_MAGIC.len() + 32 + 8 + 1;
    if bytes.len() != expected || !bytes.starts_with(ARTIFACT_MAGIC) {
        return None;
    }
    let digest_start = ARTIFACT_MAGIC.len();
    let digest_end = digest_start + 32;
    let length_end = digest_end + 8;
    let digest: [u8; 32] = bytes[digest_start..digest_end].try_into().ok()?;
    let length = u64::from_le_bytes(bytes[digest_end..length_end].try_into().ok()?);
    let executable = match bytes[length_end] {
        0 => false,
        1 => true,
        _ => return None,
    };
    Some((ObjectHandle::new(Digest(digest), length), executable))
}

fn is_executable(metadata: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        false
    }
}

fn parse_lease(_path: &Path, bytes: &[u8]) -> Option<LeaseRecord> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next()?.split('|').collect();
    if header.len() != 4 || header[0] != STORE_VERSION || header[1] != "lease" {
        return None;
    }
    let pid = header[2].parse().ok()?;
    if header[3].is_empty() {
        return None;
    }
    let owner = ProcessIdentity::new(pid, header[3]);
    let mut targets = BTreeSet::new();
    for line in lines {
        let fields: Vec<&str> = line.split('|').collect();
        if fields.len() != 2 {
            return None;
        }
        let key = parse_wire_key(fields[0], fields[1])?;
        targets.insert(key);
    }
    Some(LeaseRecord {
        owner,
        targets: targets.into_iter().collect(),
    })
}

fn parse_owner_line(bytes: &[u8]) -> Option<ProcessIdentity> {
    let text = std::str::from_utf8(bytes).ok()?;
    let fields: Vec<&str> = text.trim_end_matches('\n').split('|').collect();
    if fields.len() != 4 || fields[0] != STORE_VERSION || fields[1] != "lock" || fields[3].is_empty() {
        return None;
    }
    Some(ProcessIdentity::new(fields[2].parse().ok()?, fields[3]))
}

fn read_journal(path: &Path) -> Result<JournalState, StoreError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(JournalState {
                valid: true,
                ..JournalState::default()
            })
        }
        Err(error) => return Err(StoreError::Io(error)),
    };
    let mut state = JournalState {
        valid: true,
        bytes: bytes.len() as u64,
        ..JournalState::default()
    };
    for line in bytes.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let text = match std::str::from_utf8(line) {
            Ok(text) => text,
            Err(_) => {
                state.valid = false;
                break;
            }
        };
        let fields: Vec<&str> = text.split('|').collect();
        if fields.len() != 7 || fields[0] != STORE_VERSION {
            state.valid = false;
            break;
        }
        let prefix = format!(
            "{}|{}|{}|{}|{}|{}|",
            fields[0], fields[1], fields[2], fields[3], fields[4], fields[5]
        );
        if sha256_hex(prefix.as_bytes()) != fields[6] {
            state.valid = false;
            break;
        }
        let length = match fields[4].parse::<u64>() {
            Ok(value) => value,
            Err(_) => {
                state.valid = false;
                break;
            }
        };
        let sequence = match fields[5].parse::<u64>() {
            Ok(value) => value,
            Err(_) => {
                state.valid = false;
                break;
            }
        };
        let Some(key) = parse_wire_key(fields[2], fields[3]) else {
            state.valid = false;
            break;
        };
        state.last_sequence = state.last_sequence.max(sequence);
        match fields[1] {
            "put" | "touch" => {
                state.entries.insert(key, JournalMeta { last_use: sequence });
            }
            "remove" => {
                state.entries.remove(&key);
            }
            _ => {
                state.valid = false;
                break;
            }
        }
        let _ = length;
    }
    Ok(state)
}

fn parse_wire_key(kind: &str, value: &str) -> Option<EntryKey> {
    match kind {
        "blob" => Digest::from_hex(value).ok().map(|_| EntryKey::Blob(value.to_string())),
        "action" => Digest::from_hex(value).ok().map(|_| EntryKey::Action(value.to_string())),
        "lto" => {
            let (namespace, digest) = value.split_once('~')?;
            LtoNamespace::new(namespace).ok()?;
            Digest::from_hex(digest).ok()?;
            Some(EntryKey::Lto(namespace.to_string(), digest.to_string()))
        }
        _ => None,
    }
}

fn scan_flat_namespace(
    directory: &Path,
    kind: EntryKind,
    namespace: Option<String>,
    output: &mut Vec<StoredEntry>,
) -> Result<(), StoreError> {
    for path in sorted_children(directory)? {
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(StoreError::Io(error)),
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if Digest::from_hex(name).is_err() {
            continue;
        }
        let key = match kind {
            EntryKind::Blob => EntryKey::Blob(name.to_string()),
            EntryKind::Action => EntryKey::Action(name.to_string()),
            EntryKind::Lto => EntryKey::Lto(namespace.clone().expect("LTO namespace"), name.to_string()),
        };
        output.push(StoredEntry {
            key,
            path,
            size_bytes: metadata.len(),
            last_use: 0,
            pinned: false,
        });
    }
    Ok(())
}

fn sorted_children(directory: &Path) -> Result<Vec<PathBuf>, StoreError> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory)? {
        paths.push(entry?.path());
    }
    paths.sort_by(|left, right| left.as_os_str().cmp(right.as_os_str()));
    Ok(paths)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let parent = path
        .parent()
        .ok_or_else(|| StoreError::Config(format!("store path has no parent: {}", path.display())))?;
    ensure_dir(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| StoreError::InvalidKey(path.display().to_string()))?;
    let temporary = parent.join(format!(".{file_name}.tmp-{}-{}", std::process::id(), next_counter()));
    let result = (|| -> Result<(), StoreError> {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)?;
        sync_dir(parent)?;

        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
fn set_executable(path: &Path, executable: bool) -> Result<(), StoreError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::metadata(path)?;
        let mut permissions = metadata.permissions();
        let mode = permissions.mode();
        permissions.set_mode(if executable { mode | 0o111 } else { mode & !0o111 });
        fs::set_permissions(path, permissions)?;
    }
    #[cfg(not(unix))]
    {
        let _ = (path, executable);
    }
    Ok(())
}

fn create_new_synced(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn append_synced(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(StoreError::Corrupt {
                path: path.to_path_buf(),
                reason: "journal is not a regular file".to_string(),
            });
        }
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    sync_dir(path.parent().expect("journal path has parent"))?;
    Ok(())
}

fn ensure_dir(path: &Path) -> Result<(), StoreError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(StoreError::Config(format!(
            "store directory is a symlink: {}",
            path.display()
        ))),
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(StoreError::Config(format!("store path is not a directory: {}", path.display()))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(path)?;
            sync_dir(path.parent().expect("created directory has parent"))
        }
        Err(error) => Err(StoreError::Io(error)),
    }
}

fn sync_dir(path: &Path) -> Result<(), StoreError> {
    match File::open(path).and_then(|file| file.sync_all()) {
        Ok(()) => Ok(()),
        Err(error) if matches!(error.kind(), io::ErrorKind::Unsupported | io::ErrorKind::InvalidInput) => Ok(()),
        Err(error) => Err(StoreError::Io(error)),
    }
}

fn same_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        left.dev() == right.dev() && left.ino() == right.ino() && left.len() == right.len()
    }
    #[cfg(not(unix))]
    {
        left.len() == right.len() && left.modified().ok() == right.modified().ok()
    }
}

fn validate_component(value: &str, label: &str) -> Result<(), StoreError> {
    if value.is_empty() || value == "." || value == ".." || value.len() > 128 {
        return Err(StoreError::InvalidKey(format!("invalid {label} `{value}`")));
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(StoreError::InvalidKey(format!("invalid {label} `{value}`")));
    }
    Ok(())
}

fn parse_optional_size(name: &str) -> Result<Option<u64>, StoreError> {
    match std::env::var(name) {
        Ok(value) => parse_size(&value).map(Some).map_err(|error| match error {
            StoreError::Config(message) => StoreError::Config(format!("{name}: {message}")),
            other => other,
        }),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(StoreError::Config(format!("{name} is not valid UTF-8"))),
    }
}

/// Parse a byte size such as `4G`, `512MiB`, or a bare byte count.
pub fn parse_size(value: &str) -> Result<u64, StoreError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(StoreError::Config("size cannot be empty".to_string()));
    }
    let upper = value.to_ascii_uppercase();
    let suffixes = [
        ("TIB", 1024u64.pow(4)),
        ("TB", 1024u64.pow(4)),
        ("GIB", 1024u64.pow(3)),
        ("GB", 1024u64.pow(3)),
        ("MIB", 1024u64.pow(2)),
        ("MB", 1024u64.pow(2)),
        ("KIB", 1024u64),
        ("KB", 1024u64),
        ("T", 1024u64.pow(4)),
        ("G", 1024u64.pow(3)),
        ("M", 1024u64.pow(2)),
        ("K", 1024u64),
        ("B", 1u64),
    ];
    let (number, multiplier) = suffixes
        .iter()
        .find_map(|(suffix, multiplier)| upper.strip_suffix(suffix).map(|number| (number, *multiplier)))
        .unwrap_or((upper.as_str(), 1));
    let number = number
        .trim()
        .parse::<u64>()
        .map_err(|_| StoreError::Config(format!("invalid byte size `{value}`")))?;
    number
        .checked_mul(multiplier)
        .ok_or_else(|| StoreError::Config(format!("byte size `{value}` is too large")))
}

fn default_store_root() -> Result<PathBuf, StoreError> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| StoreError::Config("cannot determine home directory".to_string()))?;
    Ok(PathBuf::from(home).join(".cache").join("jet").join("store"))
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn hex_digit(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        10..=15 => (b'a' + value - 10) as char,
        _ => unreachable!("hex nibble is bounded"),
    }
}

fn next_counter() -> u64 {
    FILE_COUNTER.fetch_add(1, Ordering::Relaxed)
}

fn current_process_identity() -> ProcessIdentity {
    let pid = std::process::id();
    let start = process_start_token(pid).unwrap_or_else(|| {
        static FALLBACK: OnceLock<String> = OnceLock::new();
        FALLBACK
            .get_or_init(|| {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos();
                format!("fallback-{pid}-{now}")
            })
            .clone()
    });
    ProcessIdentity::new(pid, start)
}

fn owner_liveness(owner: &ProcessIdentity) -> Option<bool> {
    let current = current_process_identity();
    if owner.pid() == current.pid() {
        return Some(owner.start() == current.start());
    }
    match lookup_process_start(owner.pid()) {
        IdentityLookup::Found(start) => Some(start == owner.start()),
        IdentityLookup::Missing => Some(false),
        IdentityLookup::Unknown => None,
    }
}

enum IdentityLookup {
    Found(String),
    Missing,
    Unknown,
}

fn process_start_token(pid: u32) -> Option<String> {
    match lookup_process_start(pid) {
        IdentityLookup::Found(start) => Some(start),
        IdentityLookup::Missing | IdentityLookup::Unknown => None,
    }
}

fn lookup_process_start(pid: u32) -> IdentityLookup {
    #[cfg(target_os = "linux")]
    {
        let path = format!("/proc/{pid}/stat");
        let content = match fs::read_to_string(path) {
            Ok(content) => content,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return IdentityLookup::Missing,
            Err(_) => return IdentityLookup::Unknown,
        };
        let Some(close) = content.rfind(") ") else {
            return IdentityLookup::Unknown;
        };
        let fields: Vec<&str> = content[close + 2..].split_whitespace().collect();
        let Some(start) = fields.get(19) else {
            return IdentityLookup::Unknown;
        };
        return IdentityLookup::Found((*start).to_string());
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        IdentityLookup::Unknown
    }
}

fn filesystem_stats(path: &Path) -> Option<DiskStats> {
    let output = Command::new("df")
        .arg("-Pk")
        .arg(path)
        .env("LC_ALL", "C")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let output_text = String::from_utf8_lossy(&output.stdout);
    let line = output_text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .last()?;
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 5 {
        return None;
    }
    let total = fields[1].parse::<u64>().ok()?.checked_mul(1024)?;
    let available = fields[3].parse::<u64>().ok()?.checked_mul(1024)?;
    Some(DiskStats {
        total_bytes: total,
        available_bytes: available,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::sync::{Arc, Barrier};

    fn test_store(cap: u64) -> (Store, PathBuf) {
        let root = std::env::temp_dir().join(format!("jet-store-test-{}-{}", std::process::id(), next_counter()));
        let config = StoreConfig::new(root.clone())
            .with_cap_bytes(cap)
            .with_reserve_bytes(0);
        (Store::open(config).expect("store opens"), root)
    }

    fn cleanup(root: &Path) {
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn lock_timeout_reports_owner_and_elapsed_wait() {
        let (_, root) = test_store(1024);
        let store = Store::open(
            StoreConfig::new(root.clone())
                .with_cap_bytes(1024)
                .with_reserve_bytes(0)
                .with_lock_wait(Duration::from_millis(25)),
        )
        .expect("store opens");
        let lock = store.acquire_lock().expect("test lock");
        let error = match store.acquire_lock() {
            Err(error) => error,
            Ok(_) => panic!("live lock unexpectedly acquired"),
        };
        let text = error.to_string();
        match error {
            StoreError::Locked {
                path,
                owner_pid,
                elapsed,
            } => {
                assert_eq!(path, root.join("locks/store.lock"));
                assert_eq!(owner_pid, Some(ProcessIdentity::current().pid()));
                assert!(elapsed >= Duration::from_millis(25));
            }
            other => panic!("unexpected lock error: {other:?}"),
        }
        assert!(text.contains("owner pid"));
        assert!(text.contains("after waiting"));
        drop(lock);
        cleanup(&root);
    }

    #[test]
    fn concurrent_publishers_wait_for_shared_lock() {
        let root = std::env::temp_dir().join(format!(
            "jet-store-concurrent-{}-{}",
            std::process::id(),
            next_counter()
        ));
        let store = Store::open(
            StoreConfig::new(root.clone())
                .with_cap_bytes(1024 * 1024)
                .with_reserve_bytes(0)
                .with_lock_wait(Duration::from_secs(3)),
        )
        .expect("store opens");
        let lock = store.acquire_lock().expect("test lock");
        let barrier = Arc::new(Barrier::new(3));
        let left_store = store.clone();
        let left_barrier = barrier.clone();
        let left = thread::spawn(move || {
            left_barrier.wait();
            left_store.publish_blob(b"left")
        });
        let right_store = store.clone();
        let right_barrier = barrier.clone();
        let right = thread::spawn(move || {
            right_barrier.wait();
            right_store.publish_blob(b"right")
        });
        barrier.wait();
        thread::sleep(Duration::from_millis(2200));
        drop(lock);
        let left = left.join().expect("left publisher thread");
        let right = right.join().expect("right publisher thread");
        assert!(left.is_ok(), "left publish failed: {left:?}");
        assert!(right.is_ok(), "right publish failed: {right:?}");
        cleanup(&root);
    }

    #[test]
    fn blob_round_trip_and_layout() {
        let (store, root) = test_store(1024);
        let bytes = b"hello store";
        let object = store.publish_blob(bytes).expect("publish");
        assert_eq!(object.length(), bytes.len() as u64);
        assert_eq!(store.get_blob(&object).expect("get"), Some(bytes.to_vec()));
        assert!(store.object_path(&object).is_file());
        assert!(root.join("cas").is_dir());
        assert!(root.join("ac").is_dir());
        assert!(root.join("lto").is_dir());
        assert!(root.join("locks").is_dir());
        assert!(root.join("leases").is_dir());
        cleanup(&root);
    }

    #[test]
    fn corrupt_blob_is_quarantined_and_can_be_republished() {
        let (store, root) = test_store(1024);
        let object = store.publish_blob(b"good").expect("publish");
        fs::write(store.object_path(&object), b"bad").expect("corrupt");
        assert_eq!(store.get_blob(&object).expect("miss"), None);
        let quarantined: Vec<_> = fs::read_dir(root.join("quarantine"))
            .expect("quarantine directory")
            .collect();
        assert_eq!(quarantined.len(), 1);
        store.publish_object(object, b"good").expect("repair");
        assert_eq!(store.get_blob(&object).expect("get"), Some(b"good".to_vec()));
        cleanup(&root);
    }

    #[test]
    fn action_envelope_is_verified_and_immutable() {
        let (store, root) = test_store(1024);
        let action = store.action("compile|unit-a");
        store.publish_action(action, b"record").expect("publish");
        assert_eq!(store.get_action(&action).expect("get"), Some(b"record".to_vec()));
        assert!(matches!(
            store.publish_action(action, b"different"),
            Err(StoreError::Conflict { .. })
        ));
        fs::write(store.action_path(&action), b"bad").expect("corrupt");
        assert_eq!(store.get_action(&action).expect("miss"), None);
        assert_eq!(fs::read_dir(root.join("quarantine")).expect("quarantine").count(), 1);
        cleanup(&root);
    }

    #[test]
    fn lto_namespace_round_trip() {
        let (store, root) = test_store(1024);
        let object = store.publish_lto("output-identity", b"thin-lto").expect("publish");
        assert_eq!(
            store.get_lto("output-identity", &object).expect("get"),
            Some(b"thin-lto".to_vec())
        );
        assert!(store.lto_path("output-identity", &object).is_file());
        cleanup(&root);
    }

    #[test]
    fn journal_replays_and_prune_is_lru_and_deterministic() {
        let (store, root) = test_store(1024);
        let first = store.publish_blob(b"1111").expect("first");
        let second = store.publish_blob(b"2222").expect("second");
        let third = store.publish_blob(b"3333").expect("third");
        let _ = store.get_blob(&second).expect("touch");
        let reopened = Store::open(store.config().clone()).expect("reopen");
        let status = reopened.status().expect("status");
        assert_eq!(status.entries.len(), 3);
        let report = reopened.prune_to(4).expect("prune");
        assert_eq!(report.after_bytes, 4);
        assert!(reopened.get_blob(&second).expect("second").is_some());
        assert!(reopened.get_blob(&first).expect("first").is_none());
        assert!(reopened.get_blob(&third).expect("third").is_none());
        cleanup(&root);
    }

    #[test]
    fn live_lease_pins_blob_and_stale_lease_is_reclaimed() {
        let (store, root) = test_store(1024);
        let first = store.publish_blob(b"1111").expect("first");
        let second = store.publish_blob(b"2222").expect("second");
        let lease = store.acquire_lease(&[first]).expect("lease");
        let stale = root.join("leases").join("lease-stale");
        fs::write(&stale, format!("{STORE_VERSION}|lease|4294967295|dead\nblob|{}\n", second.key()))
            .expect("stale lease");
        let report = store.prune_to(0).expect("prune");
        assert!(report.blocked);
        assert!(store.get_blob(&first).expect("pinned").is_some());
        assert!(!stale.exists());
        drop(lease);
        let report = store.prune_to(0).expect("prune after release");
        assert!(!report.blocked);
        assert_eq!(report.after_bytes, 0);
        cleanup(&root);
    }

    #[test]
    fn cap_refuses_oversized_write_without_partial_files() {
        let (store, root) = test_store(3);
        let result = store.publish_blob(b"four");
        assert!(matches!(result, Err(StoreError::Capacity { .. })));
        assert_eq!(fs::read_dir(root.join("cas")).expect("cas").count(), 0);
        cleanup(&root);
    }

    #[test]
    fn malformed_journal_replays_valid_prefix() {
        let (store, root) = test_store(1024);
        let object = store.publish_blob(b"payload").expect("publish");
        let mut journal = fs::OpenOptions::new()
            .append(true)
            .open(root.join("journal"))
            .expect("journal");
        journal.write_all(b"not a journal record\n").expect("append");
        drop(journal);
        let reopened = Store::open(store.config().clone()).expect("reopen");
        assert_eq!(reopened.get_blob(&object).expect("get"), Some(b"payload".to_vec()));
        assert!(reopened.status().expect("status").entries.len() == 1);
        cleanup(&root);
    }

    #[test]
    fn size_parser_accepts_human_units() {
        assert_eq!(parse_size("42").unwrap(), 42);
        assert_eq!(parse_size("1k").unwrap(), 1024);
        assert_eq!(parse_size("2KB").unwrap(), 2 * 1024);
        assert_eq!(parse_size("3MiB").unwrap(), 3 * 1024 * 1024);
        assert_eq!(parse_size("4gb").unwrap(), 4 * 1024 * 1024 * 1024);
        assert!(parse_size("nope").is_err());
        assert!(parse_size("18446744073709551615K").is_err());
    }

    #[test]
    fn host_limit_overrides_adaptive_and_persists() {
        let (store, root) = test_store(1024);
        assert_eq!(store.host_limit().unwrap(), None);
        store.set_host_limit(Some(512)).unwrap();
        assert_eq!(store.host_limit().unwrap(), Some(512));
        assert_eq!(store.limit(), 512);
        let reopened = Store::open(store.config().clone()).unwrap();
        assert_eq!(reopened.host_limit().unwrap(), Some(512));
        reopened.set_host_limit(None).unwrap();
        assert_eq!(reopened.host_limit().unwrap(), None);
        cleanup(&root);
    }

    #[test]
    fn reserve_admission_refuses_when_disk_reserve_would_be_breached() {
        let (_, root) = test_store(u64::MAX);
        let gated = Store::open(
            StoreConfig::new(root.clone())
                .with_cap_bytes(u64::MAX)
                .with_reserve_bytes(u64::MAX),
        )
        .unwrap();
        assert!(matches!(
            gated.publish_blob(b"reserve"),
            Err(StoreError::Capacity { .. })
        ));
        cleanup(&root);
    }

    #[test]
    fn artifact_round_trip_repairs_corrupt_object() {
        let (store, root) = test_store(1024 * 1024);
        let source = root.join("input");
        let destination = root.join("output");
        fs::write(&source, b"artifact").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&source).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&source, permissions).unwrap();
        }
        let key = Digest::hash(b"artifact-key").to_hex();
        let artifact = store.publish_file(&key, &source).unwrap();
        assert!(artifact.path.starts_with(root.join("cas")));
        assert!(matches!(
            store.restore_file(&key, &destination).unwrap(),
            ArtifactRestore::Hit {
                executable: true,
                ..
            }
        ));
        assert_eq!(fs::read(&destination).unwrap(), b"artifact");
        #[cfg(unix)]
        assert_ne!(
            fs::metadata(&destination)
                .unwrap()
                .permissions()
                .mode()
                & 0o111,
            0
        );
        fs::write(&artifact.path, b"broken").unwrap();
        assert!(matches!(
            store.restore_file(&key, &destination).unwrap(),
            ArtifactRestore::Corrupt
        ));
        store.publish_file(&key, &source).unwrap();
        assert!(matches!(
            store.restore_file(&key, &destination).unwrap(),
            ArtifactRestore::Hit { .. }
        ));
        cleanup(&root);
    }

    #[cfg(unix)]
    #[test]
    fn second_process_live_lease_blocks_prune() {
        const CHILD_ROOT: &str = "JET_STORE_LEASE_CHILD_ROOT";
        const CHILD_KEY: &str = "JET_STORE_LEASE_CHILD_KEY";
        const CHILD_LENGTH: &str = "JET_STORE_LEASE_CHILD_LENGTH";
        const CHILD_READY: &str = "JET_STORE_LEASE_CHILD_READY";
        const CHILD_RELEASE: &str = "JET_STORE_LEASE_CHILD_RELEASE";

        if let Some(child_root) = std::env::var_os(CHILD_ROOT) {
            let root = PathBuf::from(child_root);
            let key = std::env::var(CHILD_KEY).unwrap();
            let length = std::env::var(CHILD_LENGTH).unwrap().parse().unwrap();
            let object = ObjectHandle::new(Digest::from_hex(&key).unwrap(), length);
            let store =
                Store::open(StoreConfig::new(root).with_cap_bytes(1024).with_reserve_bytes(0)).unwrap();
            let lease = store.acquire_lease(&[object]).unwrap();
            fs::write(std::env::var_os(CHILD_READY).unwrap(), b"ready").unwrap();
            let release = PathBuf::from(std::env::var_os(CHILD_RELEASE).unwrap());
            while !release.is_file() {
                thread::sleep(Duration::from_millis(5));
            }
            drop(lease);
            return;
        }

        let (store, root) = test_store(1024);
        let object = store.publish_blob(b"leased").unwrap();
        let ready = root.join("child-ready");
        let release = root.join("child-release");
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "tests::second_process_live_lease_blocks_prune", "--nocapture"])
            .env(CHILD_ROOT, &root)
            .env(CHILD_KEY, object.digest().to_hex())
            .env(CHILD_LENGTH, object.length().to_string())
            .env(CHILD_READY, &ready)
            .env(CHILD_RELEASE, &release)
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready.is_file() {
            if let Some(status) = child.try_wait().unwrap() {
                panic!("lease child exited early: {status}");
            }
            assert!(Instant::now() < deadline, "timed out waiting for lease child");
            thread::sleep(Duration::from_millis(5));
        }
        let report = store.prune_to(0).unwrap();
        assert!(report.blocked);
        assert!(store.object_path(&object).is_file());
        fs::write(&release, b"release").unwrap();
        assert!(child.wait().unwrap().success());
        let report = store.prune_to(0).unwrap();
        assert!(!report.blocked);
        cleanup(&root);
    }
}
