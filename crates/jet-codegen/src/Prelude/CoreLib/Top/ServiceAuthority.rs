// D-SERVICE-AUTHORITY1: one durable authority for AOT and ambient execution.
// The log is append-only, length/hex framed, and fsync'd after every commit.
// Process and filesystem locks close read/append and same-operation races.

use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SERVICE_AUTH_MAX_STORE: usize = 4096;
const SERVICE_AUTH_MAX_KEY: usize = 1024;
const SERVICE_AUTH_MAX_MESSAGE: usize = 1024 * 1024;
const SERVICE_AUTH_MAX_RECORDS: usize = 100_000;
const SERVICE_AUTH_MAX_BYTES: u64 = 128 * 1024 * 1024;
const SERVICE_AUTH_MAX_PENDING: usize = 100_000;
const SERVICE_AUTHORITY_TOKEN_PREFIX: &str = "sa-";
const SERVICE_AUTHORITY_TOKEN_BYTES: usize = 32;
const SERVICE_AUTH_LOCK_TIMEOUT_MS: i64 = 30_000;
const SERVICE_AUTH_LOCK_STALE_MS: i64 = 120_000;

enum JetServiceChannelError<T> {
    Full(T),
    Empty,
    Closed,
}

/// One bounded FIFO shared by a typed endpoint and its owning tree mailbox.
/// Keeping it in the authority fragment lets endpoint methods and tree methods
/// use the same queue on AOT and ambient tiers.
#[derive(Debug)]
struct JetServiceChannel<T> {
    capacity: usize,
    values: std::sync::Mutex<std::collections::VecDeque<T>>,
    closed: std::sync::atomic::AtomicBool,
    wake: std::sync::Condvar,
}

impl<T> JetServiceChannel<T> {
    fn new(capacity: usize, values: impl IntoIterator<Item = T>) -> Result<Self, ()> {
        if capacity == 0 {
            return Err(());
        }
        let values = values
            .into_iter()
            .collect::<std::collections::VecDeque<_>>();
        if values.len() > capacity {
            return Err(());
        }
        Ok(Self {
            capacity,
            values: std::sync::Mutex::new(values),
            closed: std::sync::atomic::AtomicBool::new(false),
            wake: std::sync::Condvar::new(),
        })
    }

    fn try_send(&self, value: T) -> Result<(), JetServiceChannelError<T>> {
        let mut values = self.values.lock().unwrap();
        if self.closed.load(std::sync::atomic::Ordering::Acquire) {
            return Err(JetServiceChannelError::Closed);
        }
        if values.len() >= self.capacity {
            return Err(JetServiceChannelError::Full(value));
        }
        values.push_back(value);
        self.wake.notify_one();
        Ok(())
    }

    fn try_recv(&self) -> Result<T, JetServiceChannelError<T>> {
        let mut values = self.values.lock().unwrap();
        if let Some(value) = values.pop_front() {
            return Ok(value);
        }
        if self.closed.load(std::sync::atomic::Ordering::Acquire) {
            Err(JetServiceChannelError::Closed)
        } else {
            Err(JetServiceChannelError::Empty)
        }
    }

    fn snapshot(&self) -> Vec<T>
    where
        T: Clone,
    {
        self.values.lock().unwrap().iter().cloned().collect()
    }

    fn depth(&self) -> usize {
        self.values.lock().unwrap().len()
    }

    fn clear(&self) {
        self.values.lock().unwrap().clear();
    }

    fn close(&self) {
        let _values = self.values.lock().unwrap();
        self.closed
            .store(true, std::sync::atomic::Ordering::Release);
        self.wake.notify_all();
    }
}

#[derive(Clone, Debug)]
pub struct JetServiceRuntime {
    pub store: String,
    pub retention_ms: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetDeliveryState {
    Pending,
    Accepted,
    Delivering,
    Delivered,
    DeadLettered,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetDelivery {
    pub id: String,
    pub store: String,
    pub duplicate: bool,
    pub authority: String,
    pub generation: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetDeliveryReceipt {
    pub id: String,
    pub state: JetDeliveryState,
    pub attempts: i64,
    pub retention_until: i64,
    pub deadline: i64,
    pub idempotency_key: String,
    pub duplicate: bool,
    pub authority: String,
    pub generation: i64,
    pub signature: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetDeliveryEvent {
    pub sequence: i64,
    pub state: JetDeliveryState,
    pub attempts: i64,
    pub timestamp: i64,
    pub signature: String,
}

impl Clone for JetServiceEndpoint {
    fn clone(&self) -> Self {
        Self {
            tree: self.tree.clone(),
            worker: self.worker.clone(),
            generation: self.generation,
            authority: self.authority.clone(),
            channel: self.channel.clone(),
        }
    }
}

impl std::fmt::Debug for JetServiceEndpoint {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JetServiceEndpoint")
            .field("tree", &self.tree)
            .field("worker", &self.worker)
            .field("generation", &self.generation)
            .field("authority", &"<redacted>")
            .finish()
    }
}

impl PartialEq for JetServiceEndpoint {
    fn eq(&self, other: &Self) -> bool {
        self.tree == other.tree
            && self.worker == other.worker
            && self.generation == other.generation
            && self.authority == other.authority
    }
}

impl Eq for JetServiceEndpoint {}

pub struct JetServiceEndpoint {
    pub tree: String,
    pub worker: String,
    pub generation: i64,
    /// Opaque provider-issued authority proof. It is carried by the endpoint value but
    /// never exposed as a user-selectable field.
    pub authority: String,
    channel: Option<std::sync::Arc<JetServiceChannel<String>>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JetServiceError {
    Full(String),
    Ambiguous(String),
    Unknown(String),
    NotStarted(String),
    Policy(String),
    Unavailable(String),
    /// The authority is not reachable in this process/partition. Durable
    /// receipts remain the only retry path; no ambient reconnect is attempted.
    Partitioned(String),
    /// The endpoint was issued by another tree/authority or its proof no
    /// longer validates.
    Revoked(String),
    /// The endpoint generation is no longer the active routing generation.
    Stale(String),
    /// A bounded directory/retention window has elapsed.
    Expired(String),
}

#[derive(Clone, Debug)]
struct ServiceAuthorityEntry {
    id: String,
    key: String,
    tree: String,
    worker: String,
    authority: String,
    generation: i64,
    message: String,
    created: i64,
    expires: i64,
    retained_until: Option<i64>,
    attempts: i64,
    state: JetDeliveryState,
    event_sequence: i64,
    delivered_to_worker: bool,
    delivered: bool,
    dead: bool,
}

#[derive(Clone, Debug)]
struct ServiceAuthorityEndpointState {
    tree: String,
    worker: String,
    authority: String,
    generation: i64,
    started: bool,
    draining: bool,
    partitioned: bool,
    store: Option<(String, i64)>,
    channel: Option<std::sync::Arc<JetServiceChannel<String>>>,
}

static SERVICE_AUTHORITY_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> =
    std::sync::OnceLock::new();

fn service_authority_lock() -> &'static std::sync::Mutex<()> {
    SERVICE_AUTHORITY_LOCK.get_or_init(std::sync::Mutex::default)
}

static SERVICE_ENDPOINTS: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<String, ServiceAuthorityEndpointState>>,
> = std::sync::OnceLock::new();
static SERVICE_PENDING: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<String, Vec<(String, String, String)>>>,
> = std::sync::OnceLock::new();

fn service_endpoint_registry(
) -> &'static std::sync::Mutex<std::collections::HashMap<String, ServiceAuthorityEndpointState>> {
    SERVICE_ENDPOINTS.get_or_init(std::sync::Mutex::default)
}

fn service_pending_registry(
) -> &'static std::sync::Mutex<std::collections::HashMap<String, Vec<(String, String, String)>>> {
    SERVICE_PENDING.get_or_init(std::sync::Mutex::default)
}

fn service_authority_require_issued(authority: &str) -> Result<(), JetServiceError> {
    // Issuance is recorded by the one endpoint authority registry. Do not
    // maintain a second process-local rights table for directory proofs.
    let registry = service_endpoint_registry()
        .lock()
        .map_err(|_| service_authority_error("service endpoint registry lock is poisoned"))?;
    if registry.values().any(|state| state.authority == authority) {
        Ok(())
    } else {
        Err(JetServiceError::Revoked(
            "service authority is not registered by this provider".to_string(),
        ))
    }
}

fn service_authority_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(i64::MAX)
}

fn service_authority_error(message: impl Into<String>) -> JetServiceError {
    JetServiceError::Unavailable(message.into())
}

struct ServiceAuthorityFileLock {
    path: PathBuf,
}

impl Drop for ServiceAuthorityFileLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn service_authority_file_lock(
    store: &str,
    suffix: &str,
) -> Result<ServiceAuthorityFileLock, JetServiceError> {
    service_authority_validate_store_path(store)?;
    let path = PathBuf::from(format!("{store}.{suffix}.lock"));
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).map_err(|error| {
            service_authority_error(format!("could not create service lock directory: {error}"))
        })?;
    }
    let started = service_authority_now();
    loop {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(mut file) => {
                let marker = service_authority_now().to_string();
                file.write_all(marker.as_bytes()).map_err(|error| {
                    service_authority_error(format!("could not initialize service lock: {error}"))
                })?;
                file.sync_all().map_err(|error| {
                    service_authority_error(format!("could not commit service lock: {error}"))
                })?;
                return Ok(ServiceAuthorityFileLock { path });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let now = service_authority_now();
                let stale = std::fs::metadata(&path)
                    .and_then(|metadata| metadata.modified())
                    .ok()
                    .and_then(|modified| modified.elapsed().ok())
                    .is_some_and(|age| {
                        age >= Duration::from_millis(SERVICE_AUTH_LOCK_STALE_MS as u64)
                    });
                if stale {
                    let _ = std::fs::remove_file(&path);
                    continue;
                }
                if now.saturating_sub(started) >= SERVICE_AUTH_LOCK_TIMEOUT_MS {
                    return Err(service_authority_error(
                        "service authority lock remained held beyond its recovery budget",
                    ));
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(error) => {
                return Err(service_authority_error(format!(
                    "could not create service authority lock: {error}"
                )))
            }
        }
    }
}

fn service_authority_operation_lock(
    runtime: &JetServiceRuntime,
    operation: &str,
) -> Result<ServiceAuthorityFileLock, JetServiceError> {
    let mut input = Vec::new();
    let store = service_authority_store_identity(&runtime.store);
    for field in [store.as_bytes(), operation.as_bytes()] {
        input.extend_from_slice(&(field.len() as u64).to_be_bytes());
        input.extend_from_slice(field);
    }
    let digest = jet_sha256_raw(&input);
    service_authority_file_lock(
        &runtime.store,
        &format!("op-{}", service_authority_hex(&digest)),
    )
}

fn service_authority_validate_text(
    value: &str,
    label: &str,
    max: usize,
    allow_empty: bool,
) -> Result<(), JetServiceError> {
    if (!allow_empty && value.is_empty())
        || value.len() > max
        || value.chars().any(char::is_control)
    {
        return Err(JetServiceError::Policy(format!(
            "{label} is empty, too long, or contains control characters"
        )));
    }
    Ok(())
}

fn service_authority_validate_runtime(runtime: &JetServiceRuntime) -> Result<(), JetServiceError> {
    service_authority_validate_text(
        &runtime.store,
        "service store",
        SERVICE_AUTH_MAX_STORE,
        false,
    )?;
    if runtime.retention_ms < 0 {
        return Err(JetServiceError::Policy(
            "service retention must be zero or positive".to_string(),
        ));
    }
    service_authority_validate_store_path(&runtime.store)
}

fn service_authority_store_identity(store: &str) -> String {
    let path = Path::new(store);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|directory| directory.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    normalized.to_string_lossy().into_owned()
}

fn service_authority_retention_deadline(now: i64, retention_ms: i64) -> i64 {
    if retention_ms == 0 {
        i64::MAX
    } else {
        now.saturating_add(retention_ms)
    }
}

fn service_authority_entry_expired(entry: &ServiceAuthorityEntry, now: i64) -> bool {
    !matches!(
        entry.state,
        JetDeliveryState::DeadLettered | JetDeliveryState::Cancelled
    ) && match entry.retained_until {
        Some(until) => until <= now,
        None => entry.expires <= now,
    }
}

fn service_authority_validate_store_path(store: &str) -> Result<(), JetServiceError> {
    let path = Path::new(store);
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.file_type().is_file() => {
            return Err(JetServiceError::Policy(
                "service authority store must be a regular file, not a symlink".to_string(),
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(service_authority_error(format!(
                "could not inspect service authority store: {error}"
            )))
        }
    }
    let mut parent = path.parent();
    while let Some(directory) = parent {
        match std::fs::symlink_metadata(directory) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(JetServiceError::Policy(
                    "service authority store parent must be a real directory".to_string(),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(service_authority_error(format!(
                    "could not inspect service store parent: {error}"
                )))
            }
        }
        parent = directory.parent();
    }
    Ok(())
}

fn service_authority_validate_endpoint(
    endpoint: &JetServiceEndpoint,
) -> Result<(), JetServiceError> {
    service_authority_validate_text(&endpoint.tree, "service tree", 256, false)?;
    service_authority_validate_text(&endpoint.worker, "service worker", 256, false)?;
    service_authority_validate_opaque_token(&endpoint.authority)?;
    let _ = service_authority_signing_key(&endpoint.authority)?;
    if endpoint.generation < 1 {
        return Err(JetServiceError::Policy(
            "service endpoint generation must be positive".to_string(),
        ));
    }
    Ok(())
}

/// A crossed-tier endpoint may carry a valid-looking token, but only an
/// endpoint already issued into this authority registry can be rehydrated or
/// validate a directory proof.  This keeps issuance, revocation, rotation,
/// and routing on the same substrate instead of letting a serialized value
/// mint a registry entry.
fn service_authority_require_registered_endpoint(
    endpoint: &JetServiceEndpoint,
) -> Result<(), JetServiceError> {
    service_authority_validate_endpoint(endpoint)?;
    let key = service_endpoint_key(&endpoint.authority, &endpoint.worker, endpoint.generation);
    let registry = service_endpoint_registry()
        .lock()
        .map_err(|_| service_authority_error("service endpoint registry lock is poisoned"))?;
    let state = registry.get(&key).ok_or_else(|| {
        JetServiceError::Revoked(
            "service endpoint authority was not issued in this process".to_string(),
        )
    })?;
    if state.tree != endpoint.tree
        || state.worker != endpoint.worker
        || state.authority != endpoint.authority
        || state.generation != endpoint.generation
    {
        return Err(JetServiceError::Revoked(
            "service endpoint authority does not match its issued identity".to_string(),
        ));
    }
    Ok(())
}

fn service_authority_validate_opaque_token(value: &str) -> Result<(), JetServiceError> {
    let token = value
        .strip_prefix(SERVICE_AUTHORITY_TOKEN_PREFIX)
        .filter(|token| token.len() == SERVICE_AUTHORITY_TOKEN_BYTES * 2)
        .filter(|token| {
            token
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        })
        .ok_or_else(|| {
            JetServiceError::Policy(
                "service authority must be a provider-issued opaque token".to_string(),
            )
        })?;
    if token.is_empty() {
        return Err(JetServiceError::Policy(
            "service authority must be a provider-issued opaque token".to_string(),
        ));
    }
    Ok(())
}

fn service_endpoint_key(authority: &str, worker: &str, generation: i64) -> String {
    format!("{authority}\u{1f}{worker}\u{1f}{generation}")
}

fn service_pending_key(store: &str, endpoint: &JetServiceEndpoint) -> String {
    format!(
        "{}\u{1e}{}",
        service_authority_store_identity(store),
        service_endpoint_key(&endpoint.authority, &endpoint.worker, endpoint.generation),
    )
}

fn service_authority_provider_issue() -> Result<String, JetServiceError> {
    let token = jet_crypto_entropy_bytes(SERVICE_AUTHORITY_TOKEN_BYTES as i64).map_err(|_| {
        JetServiceError::Policy("service authority cannot obtain cryptographic entropy".to_string())
    })?;
    let mut authority = String::with_capacity(
        SERVICE_AUTHORITY_TOKEN_PREFIX.len() + SERVICE_AUTHORITY_TOKEN_BYTES * 2,
    );
    authority.push_str(SERVICE_AUTHORITY_TOKEN_PREFIX);
    for byte in token {
        authority.push_str(&format!("{byte:02x}"));
    }
    Ok(authority)
}

fn service_authority_signing_key(
    authority: &str,
) -> Result<[u8; SERVICE_AUTHORITY_TOKEN_BYTES], JetServiceError> {
    let encoded = authority
        .strip_prefix(SERVICE_AUTHORITY_TOKEN_PREFIX)
        .ok_or_else(|| {
            JetServiceError::Revoked("service authority proof is malformed".to_string())
        })?;
    let bytes = service_authority_unhex(encoded)
        .filter(|bytes| bytes.len() == SERVICE_AUTHORITY_TOKEN_BYTES)
        .ok_or_else(|| {
            JetServiceError::Revoked("service authority proof is malformed".to_string())
        })?;
    bytes.try_into().map_err(|_| {
        JetServiceError::Revoked("service authority proof has the wrong size".to_string())
    })
}

fn service_authority_directory_payload(
    authority: &str,
    tree_name: &str,
    name: &str,
    endpoint: &JetServiceEndpoint,
) -> Vec<u8> {
    let generation = endpoint.generation.to_string();
    let mut payload = Vec::new();
    for field in [
        "jet-service-directory-v2".as_bytes(),
        authority.as_bytes(),
        tree_name.as_bytes(),
        name.as_bytes(),
        endpoint.tree.as_bytes(),
        endpoint.worker.as_bytes(),
        generation.as_bytes(),
    ] {
        payload.extend_from_slice(&(field.len() as u64).to_be_bytes());
        payload.extend_from_slice(field);
    }
    payload
}

/// Sign a directory entry with the vetted Prelude HMAC-SHA-256 primitive and
/// the same provider-issued authority that authenticates endpoint routing.
/// Directory keys do not form a second table.
fn service_authority_sign_directory(
    authority: &str,
    tree_name: &str,
    name: &str,
    endpoint: &JetServiceEndpoint,
) -> Result<String, JetServiceError> {
    service_authority_require_issued(authority)?;
    service_authority_validate_endpoint(endpoint)?;
    if endpoint.authority != authority || endpoint.tree != tree_name {
        return Err(JetServiceError::Revoked(
            "service directory endpoint is outside its authority tree".to_string(),
        ));
    }
    let key = service_authority_signing_key(authority)?;
    let signature = jet_hmac_sha256(
        &key,
        &service_authority_directory_payload(authority, tree_name, name, endpoint),
    );
    Ok(service_authority_hex(&signature))
}

/// Validate a signed directory entry without re-encoding the policy in the
/// service engine. Invalid or rotated proofs are typed revocations.
fn service_authority_validate_directory(
    authority: &str,
    tree_name: &str,
    name: &str,
    endpoint: &JetServiceEndpoint,
    signature: &str,
) -> Result<(), JetServiceError> {
    service_authority_require_issued(authority)?;
    service_authority_validate_endpoint(endpoint)?;
    if endpoint.authority != authority || endpoint.tree != tree_name {
        return Err(JetServiceError::Revoked(
            "service directory endpoint is outside its authority tree".to_string(),
        ));
    }
    service_authority_require_registered_endpoint(endpoint)?;
    let supplied = service_authority_unhex(signature).ok_or_else(|| {
        JetServiceError::Revoked("service directory proof is malformed".to_string())
    })?;
    let key = service_authority_signing_key(authority)?;
    let expected = jet_hmac_sha256(
        &key,
        &service_authority_directory_payload(authority, tree_name, name, endpoint),
    );
    if supplied.len() != expected.len() || !jet_ct_eq(&expected, &supplied) {
        return Err(JetServiceError::Revoked(
            "service directory proof does not validate".to_string(),
        ));
    }
    Ok(())
}

fn service_authority_endpoint_unchecked(
    tree: String,
    worker: String,
    generation: i64,
    authority: String,
) -> Result<JetServiceEndpoint, JetServiceError> {
    let endpoint = JetServiceEndpoint {
        tree,
        worker,
        generation,
        authority,
        channel: None,
    };
    service_authority_validate_endpoint(&endpoint)?;
    Ok(endpoint)
}

pub fn jet_services_authority_endpoint(
    tree: String,
    worker: String,
    generation: i64,
    authority: String,
) -> Result<JetServiceEndpoint, JetServiceError> {
    let mut endpoint = service_authority_endpoint_unchecked(tree, worker, generation, authority)?;
    let key = service_endpoint_key(&endpoint.authority, &endpoint.worker, endpoint.generation);
    endpoint.channel = service_endpoint_registry()
        .lock()
        .ok()
        .and_then(|registry| registry.get(&key).and_then(|state| state.channel.clone()));
    Ok(endpoint)
}

/// Rebind an already-issued endpoint after a tree crosses a comptime or
/// interpreter boundary. A serialized endpoint never creates authority.
pub fn jet_services_authority_hydrate(
    endpoint: &JetServiceEndpoint,
    started: bool,
) -> Result<(), JetServiceError> {
    service_authority_require_registered_endpoint(endpoint)?;
    service_authority_register(endpoint, started)
}

fn service_authority_register(
    endpoint: &JetServiceEndpoint,
    started: bool,
) -> Result<(), JetServiceError> {
    service_authority_validate_endpoint(endpoint)?;
    let key = service_endpoint_key(&endpoint.authority, &endpoint.worker, endpoint.generation);
    let mut registry = service_endpoint_registry()
        .lock()
        .map_err(|_| service_authority_error("service endpoint registry lock is poisoned"))?;
    if !registry.contains_key(&key)
        && registry
            .values()
            .any(|state| state.authority == endpoint.authority && state.tree != endpoint.tree)
    {
        return Err(JetServiceError::Revoked(
            "service authority does not match its registered tree".to_string(),
        ));
    }
    if let Some(state) = registry.get_mut(&key) {
        if state.tree != endpoint.tree || state.worker != endpoint.worker {
            return Err(JetServiceError::Revoked(
                "service endpoint authority does not match its tree".to_string(),
            ));
        }
        if started && state.partitioned {
            return Err(JetServiceError::Partitioned(
                "service endpoint authority is partitioned".to_string(),
            ));
        }
        if state.generation != endpoint.generation {
            state.store = None;
            state.draining = false;
        }
        state.generation = endpoint.generation;
        state.started = started;
        if endpoint.channel.is_some() {
            state.channel = endpoint.channel.clone();
        }
        return Ok(());
    }
    registry.insert(
        key,
        ServiceAuthorityEndpointState {
            tree: endpoint.tree.clone(),
            worker: endpoint.worker.clone(),
            authority: endpoint.authority.clone(),
            generation: endpoint.generation,
            started,
            draining: false,
            partitioned: false,
            store: None,
            channel: endpoint.channel.clone(),
        },
    );
    Ok(())
}

/// Move one routing shard between generations while holding the authority
/// registry lock. A caller can therefore observe either the old endpoint or
/// the new endpoint, never a live old endpoint after the new one is published.
/// The old entry remains registered but stopped so stale endpoint values fail
/// closed instead of retaining send rights.
fn service_authority_rotate(
    old_endpoint: &JetServiceEndpoint,
    new_endpoint: &JetServiceEndpoint,
    started: bool,
    draining: bool,
) -> Result<(), JetServiceError> {
    service_authority_validate_endpoint(old_endpoint)?;
    service_authority_validate_endpoint(new_endpoint)?;
    if old_endpoint.tree != new_endpoint.tree
        || old_endpoint.worker != new_endpoint.worker
        || old_endpoint.authority != new_endpoint.authority
        || old_endpoint.generation == new_endpoint.generation
    {
        return Err(JetServiceError::Policy(
            "service authority rotation must stay within one worker and change generation"
                .to_string(),
        ));
    }
    let old_key = service_endpoint_key(
        &old_endpoint.authority,
        &old_endpoint.worker,
        old_endpoint.generation,
    );
    let new_key = service_endpoint_key(
        &new_endpoint.authority,
        &new_endpoint.worker,
        new_endpoint.generation,
    );
    let mut registry = service_endpoint_registry()
        .lock()
        .map_err(|_| service_authority_error("service endpoint registry lock is poisoned"))?;
    let old_state = registry.get(&old_key).cloned().ok_or_else(|| {
        JetServiceError::Partitioned("service endpoint authority is not registered".to_string())
    })?;
    if old_state.tree != old_endpoint.tree
        || old_state.worker != old_endpoint.worker
        || old_state.authority != old_endpoint.authority
        || old_state.generation != old_endpoint.generation
    {
        return Err(JetServiceError::Revoked(
            "service endpoint authority does not match its worker".to_string(),
        ));
    }

    let mut new_state =
        registry
            .get(&new_key)
            .cloned()
            .unwrap_or_else(|| ServiceAuthorityEndpointState {
                tree: new_endpoint.tree.clone(),
                worker: new_endpoint.worker.clone(),
                authority: new_endpoint.authority.clone(),
                generation: new_endpoint.generation,
                started: false,
                draining: false,
                partitioned: false,
                store: None,
                channel: None,
            });
    if new_state.tree != new_endpoint.tree
        || new_state.worker != new_endpoint.worker
        || new_state.authority != new_endpoint.authority
    {
        return Err(JetServiceError::Revoked(
            "new service endpoint authority does not match its worker".to_string(),
        ));
    }
    if started && new_state.partitioned {
        return Err(JetServiceError::Partitioned(
            "new service endpoint authority is partitioned".to_string(),
        ));
    }

    new_state.generation = new_endpoint.generation;
    new_state.started = started;
    new_state.draining = draining;
    new_state.partitioned = false;
    new_state.channel = new_endpoint
        .channel
        .clone()
        .or_else(|| old_state.channel.clone());

    let old_state = registry.get_mut(&old_key).ok_or_else(|| {
        service_authority_error("old service endpoint disappeared during rotation")
    })?;
    old_state.started = false;
    old_state.draining = false;
    registry.insert(new_key, new_state);
    Ok(())
}

/// Rebind a durable receipt's authority to the restarted worker that owns the
/// same tree/worker/generation. The receipt record is the proof that this
/// alias is in scope; the alias shares the source channel and store binding,
/// so it cannot create a second mailbox or rights table.
fn service_authority_adopt_alias(
    source: &JetServiceEndpoint,
    authority: String,
) -> Result<JetServiceEndpoint, JetServiceError> {
    jet_services_authority_validate(source)?;
    let alias = service_authority_endpoint_unchecked(
        source.tree.clone(),
        source.worker.clone(),
        source.generation,
        authority,
    )?;
    let source_key = service_endpoint_key(&source.authority, &source.worker, source.generation);
    let alias_key = service_endpoint_key(&alias.authority, &alias.worker, alias.generation);
    let mut registry = service_endpoint_registry()
        .lock()
        .map_err(|_| service_authority_error("service endpoint registry lock is poisoned"))?;
    let source_state = registry.get(&source_key).cloned().ok_or_else(|| {
        JetServiceError::Partitioned("service endpoint authority is not registered".to_string())
    })?;
    if source_state.tree != alias.tree || source_state.worker != alias.worker {
        return Err(JetServiceError::Revoked(
            "service authority alias does not match its worker".to_string(),
        ));
    }
    if source_state.partitioned || !source_state.started {
        return Err(JetServiceError::Partitioned(
            "service authority alias source is not active".to_string(),
        ));
    }
    if let Some(existing) = registry.get(&alias_key) {
        if existing.tree != alias.tree
            || existing.worker != alias.worker
            || existing.generation != alias.generation
        {
            return Err(JetServiceError::Revoked(
                "service authority alias is already bound to another worker".to_string(),
            ));
        }
    } else {
        let mut alias_state = source_state.clone();
        alias_state.authority = alias.authority.clone();
        registry.insert(alias_key, alias_state);
    }
    let mut hydrated = alias;
    hydrated.channel = registry
        .get(&service_endpoint_key(
            &hydrated.authority,
            &hydrated.worker,
            hydrated.generation,
        ))
        .and_then(|state| state.channel.clone());
    Ok(hydrated)
}

pub fn jet_services_authority_update(
    endpoint: &JetServiceEndpoint,
    started: bool,
) -> Result<(), JetServiceError> {
    service_authority_validate_endpoint(endpoint)?;
    let key = service_endpoint_key(&endpoint.authority, &endpoint.worker, endpoint.generation);
    let mut registry = service_endpoint_registry()
        .lock()
        .map_err(|_| service_authority_error("service endpoint registry lock is poisoned"))?;
    let state = registry.get_mut(&key).ok_or_else(|| {
        JetServiceError::Partitioned("service endpoint authority is not registered".to_string())
    })?;
    if state.tree != endpoint.tree || state.worker != endpoint.worker {
        return Err(JetServiceError::Revoked(
            "service endpoint authority does not match its tree".to_string(),
        ));
    }
    if started && state.partitioned {
        return Err(JetServiceError::Partitioned(
            "service endpoint authority is partitioned".to_string(),
        ));
    }
    if state.generation != endpoint.generation {
        state.store = None;
        state.draining = false;
    }
    state.generation = endpoint.generation;
    state.started = started;
    if endpoint.channel.is_some() {
        state.channel = endpoint.channel.clone();
    }
    Ok(())
}

/// Partition is an authority fact. Routing reads this bit before it reads a
/// mailbox, so an endpoint cannot bypass a tree's partition decision.
pub fn jet_services_authority_update_partitioned(
    endpoint: &JetServiceEndpoint,
    partitioned: bool,
) -> Result<(), JetServiceError> {
    service_authority_validate_endpoint(endpoint)?;
    let key = service_endpoint_key(&endpoint.authority, &endpoint.worker, endpoint.generation);
    let mut registry = service_endpoint_registry()
        .lock()
        .map_err(|_| service_authority_error("service endpoint registry lock is poisoned"))?;
    let state = registry.get_mut(&key).ok_or_else(|| {
        JetServiceError::Partitioned("service endpoint authority is not registered".to_string())
    })?;
    if state.tree != endpoint.tree || state.worker != endpoint.worker {
        return Err(JetServiceError::Revoked(
            "service endpoint authority does not match its tree".to_string(),
        ));
    }
    state.partitioned = partitioned;
    if partitioned {
        state.started = false;
    }
    if endpoint.channel.is_some() {
        state.channel = endpoint.channel.clone();
    }
    Ok(())
}

fn service_authority_channel(
    endpoint: &JetServiceEndpoint,
) -> Result<(bool, bool, std::sync::Arc<JetServiceChannel<String>>), JetServiceError> {
    jet_services_authority_validate(endpoint)?;
    let key = service_endpoint_key(&endpoint.authority, &endpoint.worker, endpoint.generation);
    let registry = service_endpoint_registry()
        .lock()
        .map_err(|_| service_authority_error("service endpoint registry lock is poisoned"))?;
    let state = registry.get(&key).ok_or_else(|| {
        JetServiceError::Partitioned("service endpoint authority is not registered".to_string())
    })?;
    if state.tree != endpoint.tree || state.worker != endpoint.worker {
        return Err(JetServiceError::Revoked(
            "service endpoint authority does not match its tree".to_string(),
        ));
    }
    if state.partitioned {
        return Err(JetServiceError::Partitioned(
            "service endpoint authority is partitioned".to_string(),
        ));
    }
    let channel = state
        .channel
        .clone()
        .or_else(|| endpoint.channel.clone())
        .ok_or_else(|| {
            JetServiceError::Partitioned("service endpoint mailbox is not connected".to_string())
        })?;
    Ok((state.started, state.draining, channel))
}

/// The rollout controller owns this bit. Endpoint sends read it through the
/// same authority registry, so an endpoint cannot race a tree-local drain by
/// bypassing `ServiceTree.send`.
pub fn jet_services_authority_update_draining(
    endpoint: &JetServiceEndpoint,
    draining: bool,
) -> Result<(), JetServiceError> {
    service_authority_validate_endpoint(endpoint)?;
    let key = service_endpoint_key(&endpoint.authority, &endpoint.worker, endpoint.generation);
    let mut registry = service_endpoint_registry()
        .lock()
        .map_err(|_| service_authority_error("service endpoint registry lock is poisoned"))?;
    let state = registry.get_mut(&key).ok_or_else(|| {
        JetServiceError::Partitioned("service endpoint authority is not registered".to_string())
    })?;
    if state.tree != endpoint.tree || state.worker != endpoint.worker {
        return Err(JetServiceError::Revoked(
            "service endpoint authority does not match its tree".to_string(),
        ));
    }
    state.draining = draining;
    if endpoint.channel.is_some() {
        state.channel = endpoint.channel.clone();
    }
    Ok(())
}

/// Enqueue through the authority gate while holding the registry lock. Drain,
/// partition, handoff, and endpoint send therefore have one linearization
/// point: a send either enters the mailbox before the gate changes, or it is
/// rejected after the gate changes. A separate read of `draining` followed by
/// a queue write would strand a message after an empty-drain observation.
fn service_authority_try_send(
    endpoint: &JetServiceEndpoint,
    message: String,
) -> Result<(), JetServiceError> {
    service_authority_validate_endpoint(endpoint)?;
    if message.len() > SERVICE_AUTH_MAX_MESSAGE || message.chars().any(char::is_control) {
        return Err(JetServiceError::Policy(
            "service message exceeds the 1 MiB limit".to_string(),
        ));
    }
    let key = service_endpoint_key(&endpoint.authority, &endpoint.worker, endpoint.generation);
    let registry = service_endpoint_registry()
        .lock()
        .map_err(|_| service_authority_error("service endpoint registry lock is poisoned"))?;
    let state = match registry.get(&key) {
        Some(state) => state,
        None if registry.values().any(|candidate| {
            candidate.tree == endpoint.tree
                && candidate.worker == endpoint.worker
                && candidate.generation == endpoint.generation
        }) =>
        {
            return Err(JetServiceError::Revoked(
                "service endpoint authority does not match the registered worker".to_string(),
            ));
        }
        None if registry.values().any(|candidate| {
            candidate.tree == endpoint.tree
                && candidate.worker == endpoint.worker
                && candidate.authority == endpoint.authority
                && candidate.generation > endpoint.generation
                && !candidate.partitioned
        }) =>
        {
            return Err(JetServiceError::Stale(format!(
                "service endpoint generation {} is no longer current",
                endpoint.generation
            )));
        }
        None => {
            return Err(JetServiceError::Partitioned(
                "service endpoint authority is not registered".to_string(),
            ));
        }
    };
    if state.tree != endpoint.tree || state.worker != endpoint.worker {
        return Err(JetServiceError::Revoked(
            "service endpoint authority does not match its tree".to_string(),
        ));
    }
    if state.partitioned {
        return Err(JetServiceError::Partitioned(
            "service endpoint authority is partitioned".to_string(),
        ));
    }
    if state.generation != endpoint.generation {
        return Err(JetServiceError::Stale(format!(
            "service endpoint generation {} is not current (current generation {})",
            endpoint.generation, state.generation
        )));
    }
    if !state.started {
        return Err(JetServiceError::NotStarted(format!(
            "service worker `{}` is not running",
            endpoint.worker
        )));
    }
    if state.draining {
        return Err(JetServiceError::NotStarted(
            "service endpoint is draining".to_string(),
        ));
    }
    let channel = state
        .channel
        .clone()
        .or_else(|| endpoint.channel.clone())
        .ok_or_else(|| {
            JetServiceError::Partitioned("service endpoint mailbox is not connected".to_string())
        })?;
    match channel.try_send(message) {
        Ok(()) => Ok(()),
        Err(JetServiceChannelError::Full(_)) => Err(JetServiceError::Full(
            "service endpoint mailbox is full".to_string(),
        )),
        Err(JetServiceChannelError::Closed) => Err(JetServiceError::NotStarted(
            "service endpoint mailbox is closed".to_string(),
        )),
        Err(JetServiceChannelError::Empty) => Err(JetServiceError::Policy(
            "service channel returned an invalid send result".to_string(),
        )),
    }
}

pub fn jet_services_endpoint_send(
    endpoint: &JetServiceEndpoint,
    message: String,
) -> Result<(), JetServiceError> {
    service_authority_try_send(endpoint, message)
}

pub fn jet_services_endpoint_receive(
    endpoint: &JetServiceEndpoint,
) -> Result<String, JetServiceError> {
    service_authority_validate_endpoint(endpoint)?;
    let (started, draining, channel) = service_authority_channel(endpoint)?;
    if !started && (!draining || channel.depth() == 0) {
        return Err(JetServiceError::NotStarted(
            "service worker is not running".to_string(),
        ));
    }
    match channel.try_recv() {
        Ok(message) => Ok(message),
        Err(JetServiceChannelError::Empty) => Err(JetServiceError::Ambiguous(
            "service endpoint mailbox is empty".to_string(),
        )),
        Err(JetServiceChannelError::Closed) => Err(JetServiceError::NotStarted(
            "service endpoint mailbox is closed".to_string(),
        )),
        Err(JetServiceChannelError::Full(_)) => Err(JetServiceError::Policy(
            "service channel returned an invalid receive result".to_string(),
        )),
    }
}

fn service_authority_current_endpoint(
    runtime: &JetServiceRuntime,
    entry: &ServiceAuthorityEntry,
) -> Result<JetServiceEndpoint, JetServiceError> {
    let store = service_authority_store_identity(&runtime.store);
    let (exact, newer_exact, candidates, exact_stopped, newer_generation) = {
        let registry = service_endpoint_registry()
            .lock()
            .map_err(|_| service_authority_error("service endpoint registry lock is poisoned"))?;
        let mut exact = None;
        let mut newer_exact = None;
        let mut candidates = Vec::new();
        let mut exact_stopped = false;
        let mut newer_generation = false;
        for state in registry.values() {
            if state.tree != entry.tree || state.worker != entry.worker {
                continue;
            }
            if state.partitioned {
                continue;
            }
            // A stopped replacement does not revoke a still-running source
            // generation. Only an active newer generation can move a durable
            // receipt during restart/reconcile.
            if state.generation > entry.generation && state.started {
                newer_generation = true;
            }
            let candidate = (
                state.authority.clone(),
                state.tree.clone(),
                state.worker.clone(),
                state.generation,
                state.store.as_ref().and_then(|(bound, generation)| {
                    (*generation == state.generation).then(|| bound.clone())
                }),
            );
            if state.authority == entry.authority {
                if state.generation == entry.generation && state.started {
                    exact = Some(candidate);
                } else if state.generation == entry.generation {
                    exact_stopped = true;
                } else if state.generation > entry.generation && state.started {
                    if newer_exact.as_ref().is_none_or(
                        |current: &(String, String, String, i64, Option<String>)| {
                            current.3 < state.generation
                        },
                    ) {
                        newer_exact = Some(candidate);
                    }
                }
            } else if state.generation == entry.generation && state.started {
                candidates.push(candidate);
            }
        }
        (
            exact,
            newer_exact,
            candidates,
            exact_stopped,
            newer_generation,
        )
    };
    let selected = if let Some(exact) = exact {
        exact
    } else if let Some(newer_exact) = newer_exact {
        newer_exact
    } else {
        let bound: Vec<_> = candidates
            .iter()
            .filter(|candidate| candidate.4.as_deref() == Some(store.as_str()))
            .collect();
        if bound.len() == 1 {
            bound[0].clone()
        } else if bound.len() > 1 {
            return Err(JetServiceError::Ambiguous(
                "multiple active service authorities are bound to this store".to_string(),
            ));
        } else if candidates.len() == 1 {
            candidates[0].clone()
        } else if candidates.len() > 1 {
            return Err(JetServiceError::Ambiguous(
                "service receipt matches multiple active provider authorities".to_string(),
            ));
        } else if exact_stopped {
            return Err(JetServiceError::NotStarted(format!(
                "service worker `{}` is not running",
                entry.worker
            )));
        } else if newer_generation {
            return Err(JetServiceError::Stale(format!(
                "service receipt generation {} is no longer current",
                entry.generation
            )));
        } else {
            return Err(JetServiceError::Partitioned(
                "service receipt provider authority is not registered".to_string(),
            ));
        }
    };
    let endpoint =
        service_authority_endpoint_unchecked(selected.1, selected.2, selected.3, selected.0)?;
    service_authority_bind_store(runtime, &endpoint)?;
    if endpoint.authority == entry.authority {
        Ok(endpoint)
    } else {
        let alias = service_authority_adopt_alias(&endpoint, entry.authority.clone())?;
        service_authority_bind_store(runtime, &alias)?;
        Ok(alias)
    }
}

pub fn jet_services_authority_validate(
    endpoint: &JetServiceEndpoint,
) -> Result<(), JetServiceError> {
    service_authority_validate_endpoint(endpoint)?;
    let key = service_endpoint_key(&endpoint.authority, &endpoint.worker, endpoint.generation);
    let registry = service_endpoint_registry()
        .lock()
        .map_err(|_| service_authority_error("service endpoint registry lock is poisoned"))?;
    let state = match registry.get(&key) {
        Some(state) => state,
        None if registry.values().any(|candidate| {
            candidate.tree == endpoint.tree
                && candidate.worker == endpoint.worker
                && candidate.generation == endpoint.generation
        }) =>
        {
            return Err(JetServiceError::Revoked(
                "service endpoint authority does not match the registered worker".to_string(),
            ));
        }
        None if registry.values().any(|candidate| {
            candidate.tree == endpoint.tree
                && candidate.worker == endpoint.worker
                && candidate.authority == endpoint.authority
                && candidate.generation > endpoint.generation
                && !candidate.partitioned
        }) =>
        {
            return Err(JetServiceError::Stale(format!(
                "service endpoint generation {} is no longer current",
                endpoint.generation
            )));
        }
        None => {
            return Err(JetServiceError::Partitioned(
                "service endpoint authority is not registered".to_string(),
            ));
        }
    };
    if state.tree != endpoint.tree {
        return Err(JetServiceError::Revoked(
            "service endpoint belongs to another tree".to_string(),
        ));
    }
    if state.worker != endpoint.worker {
        return Err(JetServiceError::Revoked(
            "service endpoint belongs to another worker".to_string(),
        ));
    }
    if state.partitioned {
        return Err(JetServiceError::Partitioned(
            "service endpoint authority is partitioned".to_string(),
        ));
    }
    if state.generation != endpoint.generation {
        return Err(JetServiceError::Stale(format!(
            "service endpoint generation {} is not current (current generation {})",
            endpoint.generation, state.generation
        )));
    }
    if !state.started && !state.draining {
        if registry.values().any(|candidate| {
            candidate.tree == endpoint.tree
                && candidate.worker == endpoint.worker
                && candidate.authority == endpoint.authority
                && candidate.generation > endpoint.generation
                && candidate.started
                && !candidate.partitioned
        }) {
            return Err(JetServiceError::Stale(format!(
                "service endpoint generation {} is no longer current",
                endpoint.generation
            )));
        }
        return Err(JetServiceError::NotStarted(format!(
            "service worker `{}` is not running",
            endpoint.worker
        )));
    }
    Ok(())
}

fn service_authority_bound_store(
    endpoint: &JetServiceEndpoint,
) -> Result<Option<String>, JetServiceError> {
    jet_services_authority_validate(endpoint)?;
    let key = service_endpoint_key(&endpoint.authority, &endpoint.worker, endpoint.generation);
    let registry = service_endpoint_registry()
        .lock()
        .map_err(|_| service_authority_error("service endpoint registry lock is poisoned"))?;
    let state = registry.get(&key).ok_or_else(|| {
        JetServiceError::Partitioned("service endpoint authority is not registered".to_string())
    })?;
    if state.tree != endpoint.tree || state.worker != endpoint.worker {
        return Err(JetServiceError::Revoked(
            "service endpoint authority does not match its tree".to_string(),
        ));
    }
    Ok(state.store.as_ref().and_then(|(store, generation)| {
        (*generation == endpoint.generation).then(|| store.clone())
    }))
}

fn service_authority_bind_store(
    runtime: &JetServiceRuntime,
    endpoint: &JetServiceEndpoint,
) -> Result<String, JetServiceError> {
    service_authority_validate_runtime(runtime)?;
    jet_services_authority_validate(endpoint)?;
    let store = service_authority_store_identity(&runtime.store);
    let key = service_endpoint_key(&endpoint.authority, &endpoint.worker, endpoint.generation);
    let mut registry = service_endpoint_registry()
        .lock()
        .map_err(|_| service_authority_error("service endpoint registry lock is poisoned"))?;
    let state = registry.get_mut(&key).ok_or_else(|| {
        JetServiceError::Partitioned("service endpoint authority is not registered".to_string())
    })?;
    if state.tree != endpoint.tree || state.worker != endpoint.worker {
        return Err(JetServiceError::Revoked(
            "service endpoint authority does not match its tree".to_string(),
        ));
    }
    match state.store.as_ref() {
        Some((bound, generation)) if *generation == endpoint.generation => {
            if bound != &store {
                return Err(JetServiceError::Revoked(
                    "service endpoint authority is bound to another store".to_string(),
                ));
            }
        }
        Some(_) => {
            state.store = None;
        }
        None => {}
    }
    if state.store.is_none() {
        state.store = Some((store.clone(), endpoint.generation));
    }
    Ok(store)
}

fn service_authority_remove_pending(store: &str, id: &str) -> Result<(), JetServiceError> {
    let mut pending = service_pending_registry()
        .lock()
        .map_err(|_| service_authority_error("service pending registry lock is poisoned"))?;
    let store_identity = service_authority_store_identity(store);
    for queue in pending.values_mut() {
        queue.retain(|(queued_id, _, queued_store)| {
            queued_id != id || service_authority_store_identity(queued_store) != store_identity
        });
    }
    Ok(())
}

fn service_authority_remove_pending_entry(
    runtime: &JetServiceRuntime,
    entry: &ServiceAuthorityEntry,
) -> Result<(), JetServiceError> {
    if entry.authority.is_empty() {
        return Ok(());
    }
    service_authority_remove_pending(&runtime.store, &entry.id)
}

fn service_authority_cleanup_expired(
    runtime: &JetServiceRuntime,
    entries: &[ServiceAuthorityEntry],
    now: i64,
) -> Result<bool, JetServiceError> {
    let mut cleaned = false;
    for entry in entries {
        if service_authority_entry_expired(entry, now) {
            service_authority_append_event(
                runtime,
                entry,
                JetDeliveryState::DeadLettered,
                entry.attempts,
                now,
            )?;
            service_authority_remove_pending_entry(runtime, entry)?;
            cleaned = true;
        }
    }
    Ok(cleaned)
}

pub fn jet_services_authority_enqueue(
    runtime: &JetServiceRuntime,
    endpoint: &JetServiceEndpoint,
    id: &str,
    message: &str,
) -> Result<(), JetServiceError> {
    service_authority_validate_runtime(runtime)?;
    jet_services_authority_validate(endpoint)?;
    let store = service_authority_bind_store(runtime, endpoint)?;
    service_authority_validate_text(id, "service id", SERVICE_AUTH_MAX_KEY, false)?;
    service_authority_validate_text(message, "service message", SERVICE_AUTH_MAX_MESSAGE, true)?;
    let key = service_pending_key(&store, endpoint);
    let mut pending = service_pending_registry()
        .lock()
        .map_err(|_| service_authority_error("service pending registry lock is poisoned"))?;
    let queue = pending.entry(key).or_default();
    if queue
        .iter()
        .any(|(queued_id, _, queued_store)| queued_id == id && queued_store == &store)
    {
        return Ok(());
    }
    if queue.len() >= SERVICE_AUTH_MAX_PENDING {
        return Err(JetServiceError::Full(
            "service authority pending delivery queue is full".to_string(),
        ));
    }
    queue.push((id.to_string(), message.to_string(), store));
    Ok(())
}

pub fn jet_services_authority_take_pending(
    endpoint: &JetServiceEndpoint,
    capacity: i64,
    skip_ids: &[String],
) -> Result<Vec<(String, String, String)>, JetServiceError> {
    jet_services_authority_validate(endpoint)?;
    if capacity <= 0 {
        return Ok(Vec::new());
    }
    let Some(store) = service_authority_bound_store(endpoint)? else {
        return Ok(Vec::new());
    };
    let runtime = JetServiceRuntime {
        store: store.clone(),
        retention_ms: 0,
    };
    let _guard = service_authority_lock()
        .lock()
        .map_err(|_| service_authority_error("service authority lock is poisoned"))?;
    let records = service_authority_read(&runtime)?;
    let mut entries = service_authority_entries(&runtime, &records)?;
    if service_authority_cleanup_expired(&runtime, &entries, service_authority_now())? {
        entries = service_authority_entries(&runtime, &service_authority_read(&runtime)?)?;
    }
    let key = service_pending_key(&store, endpoint);
    let mut pending = service_pending_registry()
        .lock()
        .map_err(|_| service_authority_error("service pending registry lock is poisoned"))?;
    let queue = pending.entry(key).or_default();
    let store_identity = service_authority_store_identity(&store);
    for entry in &entries {
        let queued = queue.iter().any(|(queued_id, _, queued_store)| {
            queued_id == &entry.id
                && service_authority_store_identity(queued_store) == store_identity
        });
        if entry.authority.is_empty()
            || entry.tree != endpoint.tree
            || entry.worker != endpoint.worker
            || !service_authority_entry_routes_to_endpoint(&runtime, entry, endpoint)
            || entry.dead
            || (entry.delivered_to_worker && entry.delivered)
            // A retained receipt is eligible only after retry explicitly
            // places it back in the bounded pending queue.
            || (entry.retained_until.is_some() && !queued)
            || service_authority_entry_expired(entry, service_authority_now())
            || skip_ids.iter().any(|id| id == &entry.id)
            // Never silently retry a receipt already handed to the worker. An
            // explicit retry appends R and resets delivered_to_worker.
            || (entry.delivered_to_worker && !entry.delivered)
        {
            continue;
        }
        if queued {
            continue;
        }
        if queue.len() >= SERVICE_AUTH_MAX_PENDING {
            return Err(JetServiceError::Full(
                "service authority pending delivery queue is full".to_string(),
            ));
        }
        queue.push((entry.id.clone(), entry.message.clone(), store.clone()));
    }
    let count = capacity as usize;
    let mut selected = Vec::new();
    let mut retained = Vec::with_capacity(queue.len());
    for entry in queue.drain(..) {
        if selected.len() < count && !skip_ids.iter().any(|id| id == &entry.0) {
            selected.push(entry);
        } else {
            retained.push(entry);
        }
    }
    *queue = retained;
    Ok(selected)
}

/// Put undelivered authority records back at the head of the queue after a
/// mailbox boundary rejects a batch.  Delivery is at-least-once: a record
/// already handed to the worker remains eligible for retry if its durable
/// delivery marker could not be written.
pub fn jet_services_authority_requeue_pending(
    endpoint: &JetServiceEndpoint,
    entries: Vec<(String, String, String)>,
) -> Result<(), JetServiceError> {
    jet_services_authority_validate(endpoint)?;
    if entries.is_empty() {
        return Ok(());
    }
    let store = service_authority_bound_store(endpoint)?.ok_or_else(|| {
        JetServiceError::Partitioned(
            "service endpoint has no bound authority store for pending delivery".to_string(),
        )
    })?;
    let runtime = JetServiceRuntime {
        store: store.clone(),
        retention_ms: 0,
    };
    let _authority_guard = service_authority_lock()
        .lock()
        .map_err(|_| service_authority_error("service authority lock is poisoned"))?;
    let mut authority_entries =
        service_authority_entries(&runtime, &service_authority_read(&runtime)?)?;
    if service_authority_cleanup_expired(&runtime, &authority_entries, service_authority_now())? {
        authority_entries =
            service_authority_entries(&runtime, &service_authority_read(&runtime)?)?;
    }
    let mut normalized_entries = Vec::with_capacity(entries.len());
    for (id, message, entry_store) in entries {
        service_authority_validate_text(id.as_str(), "service id", SERVICE_AUTH_MAX_KEY, false)?;
        service_authority_validate_text(
            message.as_str(),
            "service message",
            SERVICE_AUTH_MAX_MESSAGE,
            true,
        )?;
        service_authority_validate_text(
            entry_store.as_str(),
            "service store",
            SERVICE_AUTH_MAX_STORE,
            false,
        )?;
        if service_authority_store_identity(&entry_store) != store {
            return Err(JetServiceError::Revoked(
                "pending delivery belongs to another authority store".to_string(),
            ));
        }
        let authority_entry = authority_entries
            .iter()
            .find(|entry| entry.id == id)
            .ok_or_else(|| JetServiceError::Unknown(format!("service id `{id}` is unknown")))?;
        if authority_entry.authority.is_empty()
            || authority_entry.tree != endpoint.tree
            || authority_entry.worker != endpoint.worker
            || !service_authority_entry_routes_to_endpoint(&runtime, authority_entry, endpoint)
            || authority_entry.message != message
        {
            return Err(JetServiceError::Revoked(
                "pending delivery does not match its authority receipt".to_string(),
            ));
        }
        if authority_entry.dead {
            return Err(JetServiceError::Unavailable(
                "cannot requeue a dead-lettered service receipt".to_string(),
            ));
        }
        if service_authority_entry_expired(authority_entry, service_authority_now()) {
            return Err(JetServiceError::Expired(
                "service receipt retention expired during delivery rollback".to_string(),
            ));
        }
        normalized_entries.push((id, message, store.clone()));
    }
    let key = service_pending_key(&store, endpoint);
    let mut pending = service_pending_registry()
        .lock()
        .map_err(|_| service_authority_error("service pending registry lock is poisoned"))?;
    let queue = pending.entry(key).or_default();
    if queue.len().saturating_add(normalized_entries.len()) > SERVICE_AUTH_MAX_PENDING {
        return Err(JetServiceError::Full(
            "service authority pending delivery queue is full".to_string(),
        ));
    }
    let mut restored = Vec::with_capacity(normalized_entries.len() + queue.len());
    for entry in normalized_entries {
        if !queue.iter().any(|queued| queued == &entry)
            && !restored.iter().any(|queued| queued == &entry)
        {
            restored.push(entry);
        }
    }
    restored.append(queue);
    *queue = restored;
    drop(_authority_guard);
    Ok(())
}

/// Record that a pending receipt reached the worker mailbox. This is separate
/// from `commit`: a crash after this record and before application commit is
/// still retryable, while a crash before this record is rebuilt from `S`.
pub fn jet_services_authority_mark_delivered(store: &str, id: &str) -> Result<(), JetServiceError> {
    service_authority_validate_text(id, "service id", SERVICE_AUTH_MAX_KEY, false)?;
    let runtime = JetServiceRuntime {
        store: store.to_string(),
        retention_ms: 0,
    };
    service_authority_validate_runtime(&runtime)?;
    let _operation_lock = service_authority_operation_lock(&runtime, id)?;
    let _guard = service_authority_lock()
        .lock()
        .map_err(|_| service_authority_error("service authority lock is poisoned"))?;
    let entries = service_authority_entries(&runtime, &service_authority_read(&runtime)?)?;
    let entry = entries
        .iter()
        .find(|entry| entry.id.as_str() == id)
        .ok_or_else(|| JetServiceError::Unknown(format!("service id `{id}` is unknown")))?;
    if entry.dead {
        service_authority_remove_pending_entry(&runtime, entry)?;
        return Err(JetServiceError::Unavailable(
            "cannot deliver a dead-lettered service receipt".to_string(),
        ));
    }
    if entry.state == JetDeliveryState::Cancelled {
        service_authority_remove_pending_entry(&runtime, entry)?;
        return Err(JetServiceError::Unavailable(
            "cannot deliver a cancelled delivery".to_string(),
        ));
    }
    if !entry.delivered_to_worker {
        // Remove the in-memory reservation before the durable marker. If the
        // marker append fails, the log remains the recovery source and the
        // caller can put the whole suffix back without losing this receipt.
        service_authority_remove_pending_entry(&runtime, entry)?;
        service_authority_append_event(
            &runtime,
            entry,
            JetDeliveryState::Delivering,
            entry.attempts,
            service_authority_now(),
        )?;
    } else {
        service_authority_remove_pending_entry(&runtime, entry)?;
    }
    Ok(())
}

pub fn jet_services_authority_has_uncommitted(
    endpoint: &JetServiceEndpoint,
) -> Result<bool, JetServiceError> {
    service_authority_validate_endpoint(endpoint)?;
    let key = service_endpoint_key(&endpoint.authority, &endpoint.worker, endpoint.generation);
    let store = {
        let registry = service_endpoint_registry()
            .lock()
            .map_err(|_| service_authority_error("service endpoint registry lock is poisoned"))?;
        let state = registry.get(&key).ok_or_else(|| {
            JetServiceError::Partitioned("service endpoint authority is not registered".to_string())
        })?;
        if state.tree != endpoint.tree || state.worker != endpoint.worker {
            return Err(JetServiceError::Revoked(
                "service endpoint authority does not match its tree".to_string(),
            ));
        }
        state.store.as_ref().and_then(|(store, generation)| {
            (*generation == endpoint.generation).then(|| store.clone())
        })
    };
    let Some(store) = store else {
        return Ok(false);
    };
    let runtime = JetServiceRuntime {
        store,
        retention_ms: 0,
    };
    let entries = service_authority_entries(&runtime, &service_authority_read(&runtime)?)?;
    let now = service_authority_now();
    Ok(entries.iter().any(|entry| {
        entry.tree == endpoint.tree
            && entry.worker == endpoint.worker
            && service_authority_entry_routes_to_endpoint(&runtime, entry, endpoint)
            // A handoff must pin the shard for every live durable receipt,
            // including one still waiting in the authority queue. Rotating
            // only after `delivered_to_worker` strands a receipt between the
            // old pending queue and the new endpoint. Explicit retry remains
            // the only redelivery operation; this predicate only preserves
            // the receipt's current route.
            && !entry.delivered
            && !entry.dead
            && entry.retained_until.is_none()
            && !service_authority_entry_expired(entry, now)
    }))
}

fn service_authority_enqueue_entry(
    runtime: &JetServiceRuntime,
    entry: &ServiceAuthorityEntry,
) -> Result<(), JetServiceError> {
    let endpoint = service_authority_current_endpoint(runtime, entry)?;
    if endpoint.tree != entry.tree || endpoint.worker != entry.worker {
        return Err(JetServiceError::Revoked(
            "service receipt endpoint no longer names its worker".to_string(),
        ));
    }
    jet_services_authority_enqueue(runtime, &endpoint, &entry.id, &entry.message)
}

fn service_authority_entry_routes_to_endpoint(
    runtime: &JetServiceRuntime,
    entry: &ServiceAuthorityEntry,
    endpoint: &JetServiceEndpoint,
) -> bool {
    if entry.tree != endpoint.tree || entry.worker != endpoint.worker {
        return false;
    }
    if entry.authority == endpoint.authority && entry.generation == endpoint.generation {
        return true;
    }
    // A process restart issues a fresh provider authority for the same logical
    // shard. The durable receipt keeps its original authority, while the new
    // tree endpoint is the authenticated route. Compare the resolved logical
    // endpoint after authority validation; do not rewrite the receipt's
    // identity or create a retry side channel.
    service_authority_current_endpoint(runtime, entry).is_ok_and(|current| {
        current.tree == endpoint.tree
            && current.worker == endpoint.worker
            && current.generation == endpoint.generation
    })
}

fn service_authority_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn service_authority_unhex(value: &str) -> Option<Vec<u8>> {
    if value.len() % 2 != 0 {
        return None;
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    let mut chars = value.as_bytes().chunks_exact(2);
    for pair in &mut chars {
        let high = (pair[0] as char).to_digit(16)? as u8;
        let low = (pair[1] as char).to_digit(16)? as u8;
        bytes.push((high << 4) | low);
    }
    Some(bytes)
}

fn service_authority_field(value: &str) -> String {
    format!(
        "{}:{}",
        value.len(),
        service_authority_hex(value.as_bytes())
    )
}

fn service_authority_record(op: char, fields: &[String]) -> String {
    let mut record = op.to_string();
    for field in fields {
        record.push('|');
        record.push_str(&service_authority_field(field));
    }
    record.push('\n');
    record
}

fn service_authority_parse_record(line: &str) -> Result<(char, Vec<String>), JetServiceError> {
    let mut parts = line.split('|');
    let op = parts
        .next()
        .and_then(|value| value.chars().next())
        .ok_or_else(|| service_authority_error("service authority record has no operation"))?;
    let mut fields = Vec::new();
    for part in parts {
        let (length, encoded) = part
            .split_once(':')
            .ok_or_else(|| service_authority_error("service authority field is malformed"))?;
        let length = length
            .parse::<usize>()
            .map_err(|_| service_authority_error("service authority field length is malformed"))?;
        let bytes = service_authority_unhex(encoded)
            .ok_or_else(|| service_authority_error("service authority field is not hex"))?;
        if bytes.len() != length {
            return Err(service_authority_error(
                "service authority field length does not match",
            ));
        }
        let value = String::from_utf8(bytes)
            .map_err(|_| service_authority_error("service authority field is not UTF-8"))?;
        fields.push(value);
    }
    Ok((op, fields))
}

fn service_authority_read(
    runtime: &JetServiceRuntime,
) -> Result<Vec<(char, Vec<String>)>, JetServiceError> {
    service_authority_validate_store_path(&runtime.store)?;
    let _file_lock = service_authority_file_lock(&runtime.store, "store")?;
    let mut file = match OpenOptions::new().read(true).open(&runtime.store) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(service_authority_error(format!(
                "could not open service store: {error}"
            )))
        }
    };
    let size = file
        .metadata()
        .map_err(|error| {
            service_authority_error(format!("could not inspect service store: {error}"))
        })?
        .len();
    if size > SERVICE_AUTH_MAX_BYTES {
        return Err(JetServiceError::Policy(
            "service authority log exceeds its byte limit".to_string(),
        ));
    }
    let mut contents = String::new();
    file.read_to_string(&mut contents).map_err(|error| {
        service_authority_error(format!("could not read service store: {error}"))
    })?;
    if contents.lines().count() > SERVICE_AUTH_MAX_RECORDS {
        return Err(JetServiceError::Policy(
            "service authority log is full".to_string(),
        ));
    }
    // A process can die after appending bytes but before the newline and fsync.
    // Ignore only a syntactically incomplete tail. A complete record without
    // its newline is a truncated log record and fails closed instead of being
    // silently accepted as a valid history.
    let (complete, tail) = contents
        .rfind('\n')
        .map(|end| (&contents[..end], &contents[end + 1..]))
        .unwrap_or(("", contents.as_str()));
    if !tail.is_empty() && service_authority_parse_record(tail).is_ok() {
        return Err(service_authority_error(
            "service authority log ends with a truncated record",
        ));
    }
    complete
        .lines()
        .map(service_authority_parse_record)
        .collect()
}

fn service_authority_append(
    runtime: &JetServiceRuntime,
    op: char,
    fields: &[String],
) -> Result<(), JetServiceError> {
    service_authority_validate_store_path(&runtime.store)?;
    let _file_lock = service_authority_file_lock(&runtime.store, "store")?;
    let record = service_authority_record(op, fields);
    let mut options = OpenOptions::new();
    options.create(true).append(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&runtime.store).map_err(|error| {
        service_authority_error(format!("could not open service store: {error}"))
    })?;
    let current_size = file
        .metadata()
        .map_err(|error| {
            service_authority_error(format!("could not inspect service store: {error}"))
        })?
        .len();
    let record_size = u64::try_from(record.len())
        .map_err(|_| JetServiceError::Policy("service receipt record is too large".to_string()))?;
    if current_size
        .checked_add(record_size)
        .is_none_or(|size| size > SERVICE_AUTH_MAX_BYTES)
    {
        return Err(JetServiceError::Policy(
            "service authority log exceeds its byte limit".to_string(),
        ));
    }
    file.seek(SeekFrom::Start(0)).map_err(|error| {
        service_authority_error(format!("could not seek service store: {error}"))
    })?;
    let mut contents = String::new();
    file.read_to_string(&mut contents).map_err(|error| {
        service_authority_error(format!(
            "could not read service store before append: {error}"
        ))
    })?;
    let complete_end = contents.rfind('\n').map(|end| end + 1).unwrap_or(0);
    let tail = &contents[complete_end..];
    if !tail.is_empty() {
        if service_authority_parse_record(tail).is_ok() {
            return Err(service_authority_error(
                "service authority log ends with a truncated record",
            ));
        }
        file.set_len(complete_end as u64).map_err(|error| {
            service_authority_error(format!(
                "could not repair service authority log tail: {error}"
            ))
        })?;
        file.sync_all().map_err(|error| {
            service_authority_error(format!(
                "could not commit service authority log repair: {error}"
            ))
        })?;
    }
    file.seek(SeekFrom::End(0)).map_err(|error| {
        service_authority_error(format!("could not seek service store for append: {error}"))
    })?;
    file.write_all(record.as_bytes()).map_err(|error| {
        service_authority_error(format!("could not append service receipt: {error}"))
    })?;
    file.sync_all().map_err(|error| {
        service_authority_error(format!("could not commit service receipt: {error}"))
    })
}

fn service_authority_id(
    runtime: &JetServiceRuntime,
    endpoint: &JetServiceEndpoint,
    message: &str,
    key: &str,
) -> String {
    // Receipt IDs cross process boundaries and are persisted in the authority
    // log. Authority tokens and generations are signing/route facts, not the
    // logical operation identity: they change when a worker restarts or is
    // handed off. Use the shared Core SHA-256 primitive with length-framed
    // logical fields so the same operation keeps one identity across those
    // transitions and distinct endpoint/message/key tuples cannot alias.
    let mut input = Vec::new();
    let store = service_authority_store_identity(&runtime.store);
    for field in [
        store.as_bytes(),
        endpoint.tree.as_bytes(),
        endpoint.worker.as_bytes(),
        message.as_bytes(),
        key.as_bytes(),
    ] {
        input.extend_from_slice(&(field.len() as u64).to_be_bytes());
        input.extend_from_slice(field);
    }
    let digest = jet_sha256_raw(&input);
    format!(
        "svc-{}",
        digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

fn service_delivery_state_name(state: JetDeliveryState) -> &'static str {
    match state {
        JetDeliveryState::Pending => "Pending",
        JetDeliveryState::Accepted => "Accepted",
        JetDeliveryState::Delivering => "Delivering",
        JetDeliveryState::Delivered => "Delivered",
        JetDeliveryState::DeadLettered => "DeadLettered",
        JetDeliveryState::Cancelled => "Cancelled",
    }
}

fn service_delivery_state_parse(value: &str) -> Result<JetDeliveryState, JetServiceError> {
    match value {
        "Pending" => Ok(JetDeliveryState::Pending),
        "Accepted" => Ok(JetDeliveryState::Accepted),
        "Delivering" => Ok(JetDeliveryState::Delivering),
        "Delivered" => Ok(JetDeliveryState::Delivered),
        "DeadLettered" => Ok(JetDeliveryState::DeadLettered),
        "Cancelled" => Ok(JetDeliveryState::Cancelled),
        _ => Err(service_authority_error(
            "service delivery lifecycle state is malformed",
        )),
    }
}

fn service_delivery_transition_allowed(
    entry: &ServiceAuthorityEntry,
    state: JetDeliveryState,
    attempts: i64,
) -> Result<(), JetServiceError> {
    if attempts < entry.attempts {
        return Err(service_authority_error(
            "delivery event attempts move backwards",
        ));
    }
    let allowed = match (entry.state, state) {
        (JetDeliveryState::Pending, JetDeliveryState::Accepted)
        | (JetDeliveryState::Accepted, JetDeliveryState::Accepted)
        | (JetDeliveryState::Accepted, JetDeliveryState::Delivering)
        | (JetDeliveryState::Accepted, JetDeliveryState::DeadLettered)
        | (JetDeliveryState::Accepted, JetDeliveryState::Cancelled)
        | (JetDeliveryState::Delivering, JetDeliveryState::Accepted)
        | (JetDeliveryState::Delivering, JetDeliveryState::Delivered)
        | (JetDeliveryState::Delivering, JetDeliveryState::DeadLettered)
        | (JetDeliveryState::Delivering, JetDeliveryState::Cancelled)
        | (JetDeliveryState::Delivered, JetDeliveryState::DeadLettered) => true,
        (JetDeliveryState::Delivered, JetDeliveryState::Accepted) => {
            entry.retained_until.is_some()
        }
        _ => false,
    };
    if allowed {
        Ok(())
    } else {
        Err(service_authority_error(
            "delivery event contains an invalid lifecycle transition",
        ))
    }
}

fn service_delivery_generation_key(
    authority: &str,
    generation: i64,
) -> Result<[u8; 32], JetServiceError> {
    let authority_key = service_authority_signing_key(authority)?;
    let generation = generation.to_string();
    let mut payload = Vec::new();
    for field in [
        "jet-service-delivery-generation-v1".as_bytes(),
        generation.as_bytes(),
    ] {
        payload.extend_from_slice(&(field.len() as u64).to_be_bytes());
        payload.extend_from_slice(field);
    }
    Ok(jet_hmac_sha256(&authority_key, &payload))
}

fn service_delivery_event_signature(
    authority: &str,
    generation: i64,
    id: &str,
    sequence: i64,
    state: JetDeliveryState,
    attempts: i64,
    timestamp: i64,
) -> Result<String, JetServiceError> {
    let key = service_delivery_generation_key(authority, generation)?;
    let sequence = sequence.to_string();
    let attempts = attempts.to_string();
    let timestamp = timestamp.to_string();
    let state = service_delivery_state_name(state);
    let mut payload = Vec::new();
    for field in [
        "jet-service-delivery-event-v1".as_bytes(),
        id.as_bytes(),
        sequence.as_bytes(),
        state.as_bytes(),
        attempts.as_bytes(),
        timestamp.as_bytes(),
    ] {
        payload.extend_from_slice(&(field.len() as u64).to_be_bytes());
        payload.extend_from_slice(field);
    }
    Ok(service_authority_hex(&jet_hmac_sha256(&key, &payload)))
}

fn service_delivery_acceptance_signature(
    authority: &str,
    generation: i64,
    id: &str,
    key: &str,
    tree: &str,
    worker: &str,
    message: &str,
    created: i64,
    expires: i64,
) -> Result<String, JetServiceError> {
    let signing_key = service_delivery_generation_key(authority, generation)?;
    let generation = generation.to_string();
    let created = created.to_string();
    let expires = expires.to_string();
    let mut payload = Vec::new();
    for field in [
        "jet-service-delivery-acceptance-v2".as_bytes(),
        id.as_bytes(),
        key.as_bytes(),
        authority.as_bytes(),
        tree.as_bytes(),
        worker.as_bytes(),
        generation.as_bytes(),
        message.as_bytes(),
        created.as_bytes(),
        expires.as_bytes(),
    ] {
        payload.extend_from_slice(&(field.len() as u64).to_be_bytes());
        payload.extend_from_slice(field);
    }
    Ok(service_authority_hex(&jet_hmac_sha256(&signing_key, &payload)))
}

fn service_delivery_retention_signature(
    entry: &ServiceAuthorityEntry,
    retained_until: i64,
) -> Result<String, JetServiceError> {
    let signing_key = service_delivery_generation_key(&entry.authority, entry.generation)?;
    let retained_until = retained_until.to_string();
    let mut payload = Vec::new();
    for field in [
        "jet-service-delivery-retention-v1".as_bytes(),
        entry.id.as_bytes(),
        retained_until.as_bytes(),
    ] {
        payload.extend_from_slice(&(field.len() as u64).to_be_bytes());
        payload.extend_from_slice(field);
    }
    Ok(service_authority_hex(&jet_hmac_sha256(&signing_key, &payload)))
}

fn service_delivery_receipt_signature(
    entry: &ServiceAuthorityEntry,
    duplicate: bool,
) -> Result<String, JetServiceError> {
    let key = service_delivery_generation_key(&entry.authority, entry.generation)?;
    let state = service_delivery_state_name(entry.state);
    let attempts = entry.attempts.to_string();
    let expires = entry.expires.to_string();
    let retained_until = entry.retained_until.unwrap_or(-1).to_string();
    let generation = entry.generation.to_string();
    let duplicate = duplicate.to_string();
    let mut payload = Vec::new();
    for field in [
        "jet-service-delivery-receipt-v1".as_bytes(),
        entry.id.as_bytes(),
        entry.key.as_bytes(),
        duplicate.as_bytes(),
        entry.tree.as_bytes(),
        entry.worker.as_bytes(),
        entry.message.as_bytes(),
        state.as_bytes(),
        attempts.as_bytes(),
        expires.as_bytes(),
        retained_until.as_bytes(),
        generation.as_bytes(),
    ] {
        payload.extend_from_slice(&(field.len() as u64).to_be_bytes());
        payload.extend_from_slice(field);
    }
    Ok(service_authority_hex(&jet_hmac_sha256(&key, &payload)))
}

fn service_authority_delivery_for_entry(
    runtime: &JetServiceRuntime,
    entry: &ServiceAuthorityEntry,
    duplicate: bool,
) -> JetDelivery {
    JetDelivery {
        id: entry.id.clone(),
        store: service_authority_store_identity(&runtime.store),
        duplicate,
        authority: entry.authority.clone(),
        generation: entry.generation,
    }
}

fn service_authority_delivery_for_endpoint(
    runtime: &JetServiceRuntime,
    id: &str,
    endpoint: &JetServiceEndpoint,
    duplicate: bool,
) -> JetDelivery {
    JetDelivery {
        id: id.to_string(),
        store: service_authority_store_identity(&runtime.store),
        duplicate,
        authority: endpoint.authority.clone(),
        generation: endpoint.generation,
    }
}

fn service_authority_entry_state(entry: &ServiceAuthorityEntry) -> JetDeliveryState {
    entry.state
}

fn service_authority_append_event(
    runtime: &JetServiceRuntime,
    entry: &ServiceAuthorityEntry,
    state: JetDeliveryState,
    attempts: i64,
    timestamp: i64,
) -> Result<(), JetServiceError> {
    service_delivery_transition_allowed(entry, state, attempts)?;
    let sequence = entry.event_sequence.saturating_add(1);
    let signature = service_delivery_event_signature(
        &entry.authority,
        entry.generation,
        &entry.id,
        sequence,
        state,
        attempts,
        timestamp,
    )?;
    service_authority_append(
        runtime,
        'E',
        &[
            entry.id.clone(),
            sequence.to_string(),
            service_delivery_state_name(state).to_string(),
            attempts.to_string(),
            timestamp.to_string(),
            signature,
        ],
    )
}

fn service_authority_delivery_entry(
    delivery: &JetDelivery,
) -> Result<
    (
        JetServiceRuntime,
        ServiceAuthorityEntry,
        Vec<(char, Vec<String>)>,
    ),
    JetServiceError,
> {
    service_authority_validate_text(&delivery.id, "delivery id", SERVICE_AUTH_MAX_KEY, false)?;
    let runtime = JetServiceRuntime {
        store: delivery.store.clone(),
        retention_ms: 0,
    };
    service_authority_validate_runtime(&runtime)?;
    let _operation_lock = service_authority_operation_lock(&runtime, &delivery.id)?;
    let _guard = service_authority_lock()
        .lock()
        .map_err(|_| service_authority_error("service authority lock is poisoned"))?;
    let mut records = service_authority_read(&runtime)?;
    let mut entries = service_authority_entries(&runtime, &records)?;
    if service_authority_cleanup_expired(&runtime, &entries, service_authority_now())? {
        records = service_authority_read(&runtime)?;
        entries = service_authority_entries(&runtime, &records)?;
    }
    let entry = entries
        .into_iter()
        .find(|entry| entry.id == delivery.id)
        .ok_or_else(|| {
            JetServiceError::Unknown(format!("delivery `{}` is unknown", delivery.id))
        })?;
    if delivery.authority != entry.authority {
        return Err(JetServiceError::Revoked(
            "delivery handle authority does not match its receipt".to_string(),
        ));
    }
    if delivery.generation != entry.generation {
        return Err(JetServiceError::Stale(
            "delivery handle generation does not match its receipt".to_string(),
        ));
    }
    Ok((runtime, entry, records))
}

fn service_authority_validate_delivery_runtime(
    runtime: &JetServiceRuntime,
    delivery: &JetDelivery,
) -> Result<(), JetServiceError> {
    if service_authority_store_identity(&runtime.store) != delivery.store {
        return Err(JetServiceError::Revoked(
            "delivery handle belongs to another authority store".to_string(),
        ));
    }
    service_authority_delivery_entry(delivery).map(|_| ())
}

pub fn jet_services_delivery_status(
    delivery: &JetDelivery,
) -> Result<JetDeliveryState, JetServiceError> {
    let (_, entry, _) = service_authority_delivery_entry(delivery)?;
    Ok(service_authority_entry_state(&entry))
}

pub fn jet_services_delivery_wait(
    delivery: &JetDelivery,
) -> Result<JetDeliveryState, JetServiceError> {
    jet_services_delivery_status(delivery)
}

pub fn jet_services_delivery_receipt(
    delivery: &JetDelivery,
) -> Result<JetDeliveryReceipt, JetServiceError> {
    let (_, entry, _) = service_authority_delivery_entry(delivery)?;
    Ok(JetDeliveryReceipt {
        id: entry.id.clone(),
        state: service_authority_entry_state(&entry),
        attempts: entry.attempts,
        retention_until: entry.retained_until.unwrap_or(-1),
        deadline: entry.expires,
        idempotency_key: entry.key.clone(),
        duplicate: delivery.duplicate,
        authority: entry.authority.clone(),
        generation: entry.generation,
        signature: service_delivery_receipt_signature(&entry, delivery.duplicate)?,
    })
}

pub fn jet_services_delivery_events(
    delivery: &JetDelivery,
) -> Result<Vec<JetDeliveryEvent>, JetServiceError> {
    let (_, entry, records) = service_authority_delivery_entry(delivery)?;
    let mut events = Vec::new();
    for (op, fields) in records {
        match (op, fields.as_slice()) {
            (
                'S',
                [id, _key, authority, _tree, _worker, generation, _message, created, _expires, signature],
            ) if id == &delivery.id => {
                let generation = generation.parse::<i64>().map_err(|_| {
                    service_authority_error("delivery event generation is malformed")
                })?;
                let timestamp = created.parse::<i64>().map_err(|_| {
                    service_authority_error("delivery event timestamp is malformed")
                })?;
                events.push(JetDeliveryEvent {
                    sequence: 1,
                    state: JetDeliveryState::Accepted,
                    attempts: 0,
                    timestamp,
                    signature: signature.clone(),
                });
                let expected = service_delivery_acceptance_signature(
                    authority,
                    generation,
                    id,
                    _key,
                    _tree,
                    _worker,
                    _message,
                    timestamp,
                    _expires.parse::<i64>().map_err(|_| {
                        service_authority_error("delivery acceptance expiry is malformed")
                    })?,
                )?;
                let expected = service_authority_unhex(&expected).ok_or_else(|| {
                    service_authority_error("delivery acceptance signature is malformed")
                })?;
                let supplied = service_authority_unhex(signature).ok_or_else(|| {
                    service_authority_error("delivery acceptance signature is malformed")
                })?;
                if !jet_ct_eq(&expected, &supplied) {
                    return Err(service_authority_error(
                        "delivery acceptance history signature does not validate",
                    ));
                }
                if events.len() != 0 {
                    return Err(service_authority_error(
                        "delivery event sequence is not monotonic",
                    ));
                }
            }
            ('E', [id, sequence, state, attempts, timestamp, signature]) if id == &delivery.id => {
                let sequence = sequence
                    .parse::<i64>()
                    .map_err(|_| service_authority_error("delivery event sequence is malformed"))?;
                let state = service_delivery_state_parse(state)?;
                let attempts = attempts.parse::<i64>().map_err(|_| {
                    service_authority_error("delivery event attempts are malformed")
                })?;
                let timestamp = timestamp.parse::<i64>().map_err(|_| {
                    service_authority_error("delivery event timestamp is malformed")
                })?;
                let expected_sequence = events
                    .last()
                    .map(|event| event.sequence.saturating_add(1))
                    .unwrap_or(1);
                if sequence != expected_sequence {
                    return Err(service_authority_error(
                        "delivery event sequence is not monotonic",
                    ));
                }
                let expected = service_delivery_event_signature(
                    &entry.authority,
                    entry.generation,
                    id,
                    sequence,
                    state,
                    attempts,
                    timestamp,
                )?;
                let expected = service_authority_unhex(&expected).ok_or_else(|| {
                    service_authority_error("delivery event signature is malformed")
                })?;
                let supplied = service_authority_unhex(signature).ok_or_else(|| {
                    service_authority_error("delivery event signature is malformed")
                })?;
                if !jet_ct_eq(&expected, &supplied) {
                    return Err(service_authority_error(
                        "delivery event signature does not validate",
                    ));
                }
                events.push(JetDeliveryEvent {
                    sequence,
                    state,
                    attempts,
                    timestamp,
                    signature: signature.clone(),
                });
            }
            _ => {}
        }
    }
    if events.len() as i64 != entry.event_sequence
        || events.first().map(|event| event.sequence) != Some(1)
    {
        return Err(service_authority_error(
            "delivery event history is incomplete",
        ));
    }
    Ok(events)
}

pub fn jet_services_delivery_retry(delivery: &JetDelivery) -> Result<JetDelivery, JetServiceError> {
    service_authority_delivery_entry(delivery)?;
    let runtime = JetServiceRuntime {
        store: delivery.store.clone(),
        retention_ms: 0,
    };
    jet_services_runtime_retry(&runtime, delivery)
}

pub fn jet_services_delivery_cancel(
    delivery: &JetDelivery,
) -> Result<JetDelivery, JetServiceError> {
    service_authority_delivery_entry(delivery)?;
    let runtime = JetServiceRuntime {
        store: delivery.store.clone(),
        retention_ms: 0,
    };
    service_authority_validate_runtime(&runtime)?;
    let _operation_lock = service_authority_operation_lock(&runtime, &delivery.id)?;
    let _guard = service_authority_lock()
        .lock()
        .map_err(|_| service_authority_error("service authority lock is poisoned"))?;
    let entries = service_authority_entries(&runtime, &service_authority_read(&runtime)?)?;
    let entry = entries
        .iter()
        .find(|entry| entry.id == delivery.id)
        .ok_or_else(|| {
            JetServiceError::Unknown(format!("delivery `{}` is unknown", delivery.id))
        })?;
    if entry.state == JetDeliveryState::DeadLettered {
        return Err(JetServiceError::Unavailable(
            "dead-lettered delivery cannot be cancelled".to_string(),
        ));
    }
    if entry.state == JetDeliveryState::Delivered {
        return Err(JetServiceError::Policy(
            "delivered work cannot be cancelled".to_string(),
        ));
    }
    if entry.state != JetDeliveryState::Cancelled {
        service_authority_append_event(
            &runtime,
            entry,
            JetDeliveryState::Cancelled,
            entry.attempts,
            service_authority_now(),
        )?;
        service_authority_remove_pending_entry(&runtime, entry)?;
    }
    Ok(delivery.clone())
}

fn service_authority_entries(
    runtime: &JetServiceRuntime,
    records: &[(char, Vec<String>)],
) -> Result<Vec<ServiceAuthorityEntry>, JetServiceError> {
    let mut entries: Vec<ServiceAuthorityEntry> = Vec::new();
    for (op, fields) in records {
        match (*op, fields.as_slice()) {
            (
                'S',
                [id, key, authority, tree, worker, generation, message, created, expires, signature],
            ) => {
                let generation = generation
                    .parse::<i64>()
                    .map_err(|_| service_authority_error("service send generation is malformed"))?;
                let created = created
                    .parse::<i64>()
                    .map_err(|_| service_authority_error("service send timestamp is malformed"))?;
                let expires = expires
                    .parse::<i64>()
                    .map_err(|_| service_authority_error("service send expiry is malformed"))?;
                let endpoint = service_authority_endpoint_unchecked(
                    tree.clone(),
                    worker.clone(),
                    generation,
                    authority.clone(),
                )?;
                service_authority_validate_text(
                    key,
                    "service idempotency key",
                    SERVICE_AUTH_MAX_KEY,
                    false,
                )?;
                service_authority_validate_text(
                    message,
                    "service message",
                    SERVICE_AUTH_MAX_MESSAGE,
                    true,
                )?;
                let expected_id = service_authority_id(runtime, &endpoint, message, key);
                if id != &expected_id {
                    return Err(service_authority_error(
                        "service receipt identity does not match its authority fields",
                    ));
                }
                let expected_signature = service_delivery_acceptance_signature(
                    authority,
                    generation,
                    id,
                    key,
                    tree,
                    worker,
                    message,
                    created,
                    expires,
                )?;
                let supplied_signature = service_authority_unhex(signature).ok_or_else(|| {
                    service_authority_error("service acceptance signature is malformed")
                })?;
                let expected_bytes =
                    service_authority_unhex(&expected_signature).ok_or_else(|| {
                        service_authority_error("service acceptance signature is malformed")
                    })?;
                if !jet_ct_eq(&expected_bytes, &supplied_signature) {
                    return Err(service_authority_error(
                        "service acceptance signature does not validate",
                    ));
                }
                let entry = ServiceAuthorityEntry {
                    id: id.clone(),
                    key: key.clone(),
                    tree: tree.clone(),
                    worker: worker.clone(),
                    authority: authority.clone(),
                    generation,
                    message: message.clone(),
                    created,
                    expires,
                    retained_until: None,
                    attempts: 0,
                    state: JetDeliveryState::Accepted,
                    event_sequence: 1,
                    delivered_to_worker: false,
                    delivered: false,
                    dead: false,
                };
                if entries.iter().any(|previous| previous.id.as_str() == id.as_str()) {
                    return Err(service_authority_error(
                        "service authority log contains duplicate acceptance",
                    ));
                }
                if entries.iter().any(|previous| previous.key.as_str() == key.as_str()) {
                    return Err(service_authority_error(
                        "service authority log contains duplicate idempotency work",
                    ));
                }
                entries.push(entry);
            }
            ('K', [id, until, signature]) => {
                let until = until.parse::<i64>().map_err(|_| {
                    service_authority_error("service retention deadline is malformed")
                })?;
                let entry = entries
                    .iter_mut()
                    .find(|entry| entry.id.as_str() == id.as_str())
                    .ok_or_else(|| {
                        service_authority_error("service retention references an unknown id")
                    })?;
                let expected = service_delivery_retention_signature(entry, until)?;
                let expected = service_authority_unhex(&expected).ok_or_else(|| {
                    service_authority_error("service retention signature is malformed")
                })?;
                let supplied = service_authority_unhex(signature).ok_or_else(|| {
                    service_authority_error("service retention signature is malformed")
                })?;
                if !jet_ct_eq(&expected, &supplied) {
                    return Err(service_authority_error(
                        "service retention signature does not validate",
                    ));
                }
                entry.retained_until = Some(until);
            }
            ('E', [id, sequence, state, attempts, timestamp, signature]) => {
                let sequence = sequence
                    .parse::<i64>()
                    .map_err(|_| service_authority_error("delivery event sequence is malformed"))?;
                let attempts = attempts.parse::<i64>().map_err(|_| {
                    service_authority_error("delivery event attempts are malformed")
                })?;
                let timestamp = timestamp.parse::<i64>().map_err(|_| {
                    service_authority_error("delivery event timestamp is malformed")
                })?;
                let state = service_delivery_state_parse(state)?;
                let entry = entries
                    .iter_mut()
                    .find(|entry| entry.id.as_str() == id.as_str())
                    .ok_or_else(|| {
                        service_authority_error("delivery event references an unknown id")
                    })?;
                if sequence != entry.event_sequence.saturating_add(1) {
                    return Err(service_authority_error(
                        "delivery event sequence is not monotonic",
                    ));
                }
                service_delivery_transition_allowed(entry, state, attempts)?;
                let expected = service_delivery_event_signature(
                    &entry.authority,
                    entry.generation,
                    id,
                    sequence,
                    state,
                    attempts,
                    timestamp,
                )?;
                let expected = service_authority_unhex(&expected).ok_or_else(|| {
                    service_authority_error("delivery event signature is malformed")
                })?;
                let supplied = service_authority_unhex(signature).ok_or_else(|| {
                    service_authority_error("delivery event signature is malformed")
                })?;
                if !jet_ct_eq(&expected, &supplied) {
                    return Err(service_authority_error(
                        "delivery event signature does not validate",
                    ));
                }
                let _ = timestamp;
                entry.event_sequence = sequence;
                entry.attempts = attempts;
                entry.state = state;
                entry.delivered_to_worker =
                    state == JetDeliveryState::Delivering || state == JetDeliveryState::Delivered;
                entry.delivered = state == JetDeliveryState::Delivered;
                entry.dead = state == JetDeliveryState::DeadLettered;
            }
            _ => {
                return Err(service_authority_error(
                    "service authority record has an unknown shape",
                ))
            }
        }
    }
    Ok(entries)
}

pub fn jet_services_runtime(store: String, retention_ms: i64) -> JetServiceRuntime {
    JetServiceRuntime {
        store,
        retention_ms,
    }
}

pub fn jet_services_runtime_send(
    runtime: &JetServiceRuntime,
    endpoint: &JetServiceEndpoint,
    message: &String,
    key: &String,
) -> Result<JetDelivery, JetServiceError> {
    service_authority_validate_runtime(runtime)?;
    service_authority_validate_endpoint(endpoint)?;
    jet_services_authority_validate(endpoint)?;
    service_authority_bind_store(runtime, endpoint)?;
    service_authority_validate_text(key, "idempotency key", SERVICE_AUTH_MAX_KEY, false)?;
    service_authority_validate_text(message, "service message", SERVICE_AUTH_MAX_MESSAGE, true)?;
    let _operation_lock = service_authority_operation_lock(runtime, key)?;
    let _guard = service_authority_lock()
        .lock()
        .map_err(|_| service_authority_error("service authority lock is poisoned"))?;
    let records = service_authority_read(runtime)?;
    let mut entries = service_authority_entries(runtime, &records)?;
    let now = service_authority_now();
    if service_authority_cleanup_expired(runtime, &entries, now)? {
        entries = service_authority_entries(runtime, &service_authority_read(runtime)?)?;
    }
    if let Some(entry) = entries
        .iter()
        .find(|entry| entry.key.as_str() == key.as_str() && !entry.authority.is_empty())
    {
        // The provider authority token is a generation-scoped signing fact.
        // A restarted or handed-off provider gets a new token, but the same
        // logical tree/worker operation must still resolve to this record.
        // Conflicting route or message reuse is rejected before any work is
        // appended or enqueued.
        if entry.tree != endpoint.tree
            || entry.worker != endpoint.worker
            || entry.message.as_str() != message.as_str()
        {
            return Err(JetServiceError::Policy(
                "idempotency key was already used by another service authority or delivery"
                    .to_string(),
            ));
        }
        let duplicate_delivery = || service_authority_delivery_for_entry(runtime, entry, true);
        if entry.dead {
            return Ok(duplicate_delivery());
        }
        if entry.expires > now {
            if !entry.delivered && entry.state != JetDeliveryState::Cancelled {
                service_authority_enqueue_entry(runtime, entry)?;
            }
            return Ok(duplicate_delivery());
        }
        return Ok(duplicate_delivery());
    }
    let id = service_authority_id(runtime, endpoint, message, key);
    let expires = service_authority_retention_deadline(now, runtime.retention_ms);
    let signature = service_delivery_acceptance_signature(
        &endpoint.authority,
        endpoint.generation,
        &id,
        key,
        &endpoint.tree,
        &endpoint.worker,
        message,
        now,
        expires,
    )?;
    service_authority_append(
        runtime,
        'S',
        &[
            id.clone(),
            key.clone(),
            endpoint.authority.clone(),
            endpoint.tree.clone(),
            endpoint.worker.clone(),
            endpoint.generation.to_string(),
            message.clone(),
            now.to_string(),
            expires.to_string(),
            signature,
        ],
    )?;
    jet_services_authority_enqueue(runtime, endpoint, &id, message)?;
    Ok(service_authority_delivery_for_endpoint(
        runtime, &id, endpoint, false,
    ))
}

pub fn jet_services_runtime_retry(
    runtime: &JetServiceRuntime,
    delivery: &JetDelivery,
) -> Result<JetDelivery, JetServiceError> {
    service_authority_validate_runtime(runtime)?;
    service_authority_validate_delivery_runtime(runtime, delivery)?;
    let id = &delivery.id;
    service_authority_validate_text(id, "service id", SERVICE_AUTH_MAX_KEY, false)?;
    let _operation_lock = service_authority_operation_lock(runtime, id)?;
    let _guard = service_authority_lock()
        .lock()
        .map_err(|_| service_authority_error("service authority lock is poisoned"))?;
    let records = service_authority_read(runtime)?;
    let mut entries = service_authority_entries(runtime, &records)?;
    let now = service_authority_now();
    if service_authority_cleanup_expired(runtime, &entries, now)? {
        entries = service_authority_entries(runtime, &service_authority_read(runtime)?)?;
    }
    let entry = entries
        .iter()
        .find(|entry| entry.id.as_str() == id.as_str())
        .ok_or_else(|| JetServiceError::Unknown(format!("service id `{id}` is unknown")))?;
    if entry.authority.is_empty() {
        return Err(JetServiceError::Unavailable(
            "service receipt has no endpoint authority".to_string(),
        ));
    }
    if entry.dead {
        return Err(JetServiceError::Unavailable(
            "cannot retry a dead-lettered delivery".to_string(),
        ));
    }
    if entry.state == JetDeliveryState::Cancelled {
        return Err(JetServiceError::Policy(
            "cancelled delivery cannot be retried".to_string(),
        ));
    }
    if entry.delivered && entry.retained_until.is_none() {
        return Ok(service_authority_delivery_for_entry(runtime, entry, false));
    }
    // Retry is the one explicit operation that may reset a receipt already
    // handed to a worker but not committed. Without this R record, a retry
    // would enqueue a copy while the durable marker still said
    // `delivered_to_worker`, and the receive path could not distinguish it
    // from a silent duplicate.
    let attempts = entry.attempts.saturating_add(1);
    service_authority_append_event(runtime, entry, JetDeliveryState::Accepted, attempts, now)?;
    service_authority_enqueue_entry(runtime, entry)?;
    Ok(service_authority_delivery_for_entry(runtime, entry, false))
}

pub fn jet_services_runtime_dead_letter(
    runtime: &JetServiceRuntime,
    delivery: &JetDelivery,
) -> Result<JetDelivery, JetServiceError> {
    service_authority_validate_runtime(runtime)?;
    service_authority_validate_delivery_runtime(runtime, delivery)?;
    let id = &delivery.id;
    service_authority_validate_text(id, "service id", SERVICE_AUTH_MAX_KEY, false)?;
    let _operation_lock = service_authority_operation_lock(runtime, id)?;
    let _guard = service_authority_lock()
        .lock()
        .map_err(|_| service_authority_error("service authority lock is poisoned"))?;
    let mut entries = service_authority_entries(runtime, &service_authority_read(runtime)?)?;
    if service_authority_cleanup_expired(runtime, &entries, service_authority_now())? {
        entries = service_authority_entries(runtime, &service_authority_read(runtime)?)?;
    }
    let Some(entry) = entries
        .iter()
        .find(|entry| entry.id.as_str() == id.as_str())
    else {
        return Err(JetServiceError::Unknown(format!(
            "service id `{id}` is unknown"
        )));
    };
    if entry.dead {
        return Ok(service_authority_delivery_for_entry(runtime, entry, false));
    }
    service_authority_append_event(
        runtime,
        entry,
        JetDeliveryState::DeadLettered,
        entry.attempts,
        service_authority_now(),
    )?;
    service_authority_remove_pending_entry(runtime, entry)?;
    Ok(service_authority_delivery_for_entry(runtime, entry, false))
}

pub fn jet_services_runtime_retain(
    runtime: &JetServiceRuntime,
    delivery: &JetDelivery,
) -> Result<JetDelivery, JetServiceError> {
    service_authority_validate_runtime(runtime)?;
    service_authority_validate_delivery_runtime(runtime, delivery)?;
    let id = &delivery.id;
    service_authority_validate_text(id, "service id", SERVICE_AUTH_MAX_KEY, false)?;
    let _operation_lock = service_authority_operation_lock(runtime, id)?;
    let _guard = service_authority_lock()
        .lock()
        .map_err(|_| service_authority_error("service authority lock is poisoned"))?;
    let mut entries = service_authority_entries(runtime, &service_authority_read(runtime)?)?;
    if service_authority_cleanup_expired(runtime, &entries, service_authority_now())? {
        entries = service_authority_entries(runtime, &service_authority_read(runtime)?)?;
    }
    let Some(entry) = entries
        .iter()
        .find(|entry| entry.id.as_str() == id.as_str())
    else {
        return Err(JetServiceError::Unknown(format!(
            "service id `{id}` is unknown"
        )));
    };
    if entry.dead {
        return Ok(service_authority_delivery_for_entry(runtime, entry, false));
    }
    if entry.state == JetDeliveryState::Cancelled {
        return Err(JetServiceError::Policy(
            "cancelled delivery cannot be retained".to_string(),
        ));
    }
    let until = service_authority_retention_deadline(service_authority_now(), runtime.retention_ms);
    let signature = service_delivery_retention_signature(entry, until)?;
    service_authority_append(
        runtime,
        'K',
        &[id.clone(), until.to_string(), signature],
    )?;
    Ok(service_authority_delivery_for_entry(runtime, entry, false))
}

/// Durably acknowledge that a delivered receipt has completed its service
/// transaction. The acknowledgement is separate from mailbox enqueue so a
/// crash between durable send and worker delivery remains recoverable by
/// `retry`; an already acknowledged receipt is idempotent.
pub fn jet_services_runtime_commit(
    runtime: &JetServiceRuntime,
    delivery: &JetDelivery,
) -> Result<(), JetServiceError> {
    service_authority_validate_runtime(runtime)?;
    service_authority_validate_delivery_runtime(runtime, delivery)?;
    let id = &delivery.id;
    service_authority_validate_text(id, "service id", SERVICE_AUTH_MAX_KEY, false)?;
    let _operation_lock = service_authority_operation_lock(runtime, id)?;
    let _guard = service_authority_lock()
        .lock()
        .map_err(|_| service_authority_error("service authority lock is poisoned"))?;
    let mut entries = service_authority_entries(runtime, &service_authority_read(runtime)?)?;
    let now = service_authority_now();
    let target_expired = entries
        .iter()
        .find(|entry| entry.id.as_str() == id.as_str())
        .is_some_and(|entry| service_authority_entry_expired(entry, now));
    if service_authority_cleanup_expired(runtime, &entries, now)? {
        entries = service_authority_entries(runtime, &service_authority_read(runtime)?)?;
    }
    let entry = entries
        .iter()
        .find(|entry| entry.id.as_str() == id.as_str())
        .ok_or_else(|| JetServiceError::Unknown(format!("service id `{id}` is unknown")))?;
    if entry.dead {
        service_authority_remove_pending_entry(runtime, entry)?;
        if target_expired {
            return Err(JetServiceError::Expired(
                "service receipt retention expired".to_string(),
            ));
        }
        return Err(JetServiceError::Unavailable(
            "cannot commit a dead-lettered service receipt".to_string(),
        ));
    }
    if !entry.delivered_to_worker {
        return Err(JetServiceError::Unavailable(
            "service receipt has not reached its worker".to_string(),
        ));
    }
    if !entry.delivered {
        service_authority_append_event(
            runtime,
            entry,
            JetDeliveryState::Delivered,
            entry.attempts,
            service_authority_now(),
        )?;
    }
    service_authority_remove_pending_entry(runtime, entry)?;
    Ok(())
}
