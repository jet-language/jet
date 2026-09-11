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
thread_local! {
    /// The active job/service invocation carries the already-issued endpoint,
    /// not a second authority token. Job graph entry points install this scope
    /// before invoking checked work; queue dispatch reads it without scanning
    /// registries.
    static JET_SERVICE_EXECUTION_ENDPOINT: std::cell::RefCell<Option<JetServiceEndpoint>> =
        const { std::cell::RefCell::new(None) };
}

pub struct JetServiceExecutionScope {
    previous: Option<JetServiceEndpoint>,
}

impl Drop for JetServiceExecutionScope {
    fn drop(&mut self) {
        JET_SERVICE_EXECUTION_ENDPOINT.with(|slot| {
            slot.replace(self.previous.take());
        });
    }
}

/// Enter an execution scope using an endpoint issued by the existing service
/// authority.  Validation is performed on entry and again when read so
/// rotation, partition, stop, and revocation remain visible to queue callers.
pub fn jet_services_execution_scope(
    endpoint: &JetServiceEndpoint,
) -> Result<JetServiceExecutionScope, JetServiceError> {
    jet_services_authority_validate(endpoint)?;
    let previous = JET_SERVICE_EXECUTION_ENDPOINT.with(|slot| slot.replace(Some(endpoint.clone())));
    Ok(JetServiceExecutionScope { previous })
}

/// Read an installed endpoint without converting a genuinely empty execution
/// scope into the same `NotStarted` error used for a stopped worker.
fn jet_services_active_execution_endpoint_if_present(
) -> Result<Option<JetServiceEndpoint>, JetServiceError> {
    let endpoint = JET_SERVICE_EXECUTION_ENDPOINT.with(|slot| slot.borrow().clone());
    let Some(endpoint) = endpoint else {
        return Ok(None);
    };
    jet_services_authority_validate(&endpoint)?;
    Ok(Some(endpoint))
}

/// Return the currently checked endpoint for an in-process job invocation.
/// There is no ambient authority fallback: a job without an installed scope
/// receives a typed not-started error.
pub fn jet_services_active_execution_endpoint() -> Result<JetServiceEndpoint, JetServiceError> {
    jet_services_active_execution_endpoint_if_present()?.ok_or_else(|| {
        JetServiceError::NotStarted(
            "job queue requires an active issued service endpoint".to_string(),
        )
    })
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
    let authority_state = {
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
        if state.partitioned {
            JetDevtoolsTopologyEndpointAuthorityState::Revoked
        } else if state.started {
            JetDevtoolsTopologyEndpointAuthorityState::Verified
        } else {
            JetDevtoolsTopologyEndpointAuthorityState::Unverified
        }
    };
    jet_services_publish_endpoint_fact(endpoint, authority_state);
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
    let authority_state = {
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
        if state.partitioned {
            JetDevtoolsTopologyEndpointAuthorityState::Revoked
        } else if state.started {
            JetDevtoolsTopologyEndpointAuthorityState::Verified
        } else {
            JetDevtoolsTopologyEndpointAuthorityState::Unverified
        }
    };
    jet_services_publish_endpoint_fact(endpoint, authority_state);
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
    let authority_state = {
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
        if state.partitioned {
            JetDevtoolsTopologyEndpointAuthorityState::Revoked
        } else if state.started {
            JetDevtoolsTopologyEndpointAuthorityState::Verified
        } else {
            JetDevtoolsTopologyEndpointAuthorityState::Unverified
        }
    };
    jet_services_publish_endpoint_fact(endpoint, authority_state);
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
            || entry.state == JetDeliveryState::Cancelled
            || entry.dead
            || !service_authority_entry_routes_to_endpoint(&runtime, entry, endpoint)
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
        if authority_entry.state == JetDeliveryState::Cancelled {
            return Err(JetServiceError::Unavailable(
                "cannot requeue a cancelled service receipt".to_string(),
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
            && entry.state != JetDeliveryState::Cancelled
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
                if !events.is_empty() {
                    return Err(service_authority_error(
                        "delivery event sequence is not monotonic",
                    ));
                }
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
// D-DX-QUEUE1=A: the durable job queue is a database-backed state machine.
// This fragment owns the queue contract and transition rules.  A tier supplies
// only a `JetJobQueueStore` adapter; the default adapter is installed by the
// existing SQLite bridge, while alternate providers are explicit.
const JET_JOB_QUEUE_MAX_NAME: usize = 256;
const JET_JOB_QUEUE_MAX_TYPE: usize = 256;
const JET_JOB_QUEUE_MAX_KEY: usize = 1024;
const JET_JOB_QUEUE_MAX_REQUEST: usize = 1024;
const JET_JOB_QUEUE_MAX_PAYLOAD: usize = 4 * 1024 * 1024;
const JET_JOB_QUEUE_MAX_REASON: usize = 1024;
const JET_JOB_QUEUE_MAX_DETAIL: usize = 16 * 1024;
const JET_JOB_QUEUE_MAX_BULK: usize = 1024;
const JET_JOB_QUEUE_MAX_INSPECT: usize = 1024;
const JET_JOB_QUEUE_MAX_ATTEMPTS: u32 = 64;
const JET_JOB_QUEUE_MAX_LEASE_MS: i64 = 24 * 60 * 60 * 1000;
const JET_JOB_QUEUE_MAX_DELAY_MS: i64 = 365 * 24 * 60 * 60 * 1000;

/// Values a queue provider binds to SQLite.  Queue SQL is private to this
/// Prelude; providers never concatenate payloads into statements.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JetJobQueueValue {
    Null,
    Int(i64),
    Text(String),
    Blob(Vec<u8>),
}

/// One row returned by a queue provider.  A vector keeps this carrier usable
/// by the AOT, JIT, and interpreter adapters without exposing a backend map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetJobQueueRow {
    pub values: Vec<(String, JetJobQueueValue)>,
}

impl JetJobQueueRow {
    pub fn new(values: Vec<(String, JetJobQueueValue)>) -> Self {
        Self { values }
    }

    pub fn get(&self, name: &str) -> Option<&JetJobQueueValue> {
        self.values
            .iter()
            .find(|(column, _)| column == name)
            .map(|(_, value)| value)
    }
}

/// The database transaction seam.  It is deliberately smaller than the
/// general DB surface: a provider only marshals typed bound values and rows;
/// enqueue, lease, retry, and authority policy remain here.
pub trait JetJobQueueStore: Send {
    fn begin(&mut self) -> Result<(), JetServiceError>;
    fn commit(&mut self) -> Result<(), JetServiceError>;
    fn rollback(&mut self);
    fn execute(
        &mut self,
        sql: &str,
        params: &[JetJobQueueValue],
    ) -> Result<i64, JetServiceError>;
    fn query(
        &mut self,
        sql: &str,
        params: &[JetJobQueueValue],
    ) -> Result<Vec<JetJobQueueRow>, JetServiceError>;
}

/// At-least-once is explicit.  The queue never claims exactly-once delivery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetJobQueueDeliveryPolicy {
    AtLeastOnce,
}

impl JetJobQueueDeliveryPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AtLeastOnce => "at_least_once",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetJobQueueState {
    Queued,
    Running,
    Retrying,
    Completed,
    Failed,
    DeadLettered,
    Cancelled,
}

impl JetJobQueueState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Retrying => "retrying",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::DeadLettered => "dead_lettered",
            Self::Cancelled => "cancelled",
        }
    }

    fn parse(value: &str) -> Result<Self, JetServiceError> {
        match value {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "retrying" => Ok(Self::Retrying),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "dead_lettered" => Ok(Self::DeadLettered),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(service_authority_error(format!(
                "queue state `{value}` is unknown"
            ))),
        }
    }
}


impl JetJobPayload {
    pub fn new(
        type_id: impl Into<String>,
        bytes: Vec<u8>,
        publish: bool,
    ) -> Result<Self, JetServiceError> {
        let payload = Self {
            type_id: type_id.into(),
            bytes,
            publish,
        };
        jet_job_queue_validate_payload(&payload)?;
        Ok(payload)
    }
}


impl JetJobResult {
    pub fn new(
        type_id: impl Into<String>,
        bytes: Vec<u8>,
        publish: bool,
    ) -> Result<Self, JetServiceError> {
        let result = Self {
            type_id: type_id.into(),
            bytes,
            publish,
        };
        jet_job_queue_validate_result(&result)?;
        Ok(result)
    }
}


impl JetJobError {
    pub fn new(
        type_id: impl Into<String>,
        reason: impl Into<String>,
        detail: Option<String>,
    ) -> Result<Self, JetServiceError> {
        let error = Self {
            type_id: type_id.into(),
            reason: reason.into(),
            detail,
        };
        jet_job_queue_validate_error(&error)?;
        Ok(error)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetJobEnqueue {
    pub job_type: String,
    pub payload: JetJobPayload,
    pub idempotency_key: Option<String>,
    pub request_id: Option<String>,
    /// Relative delay in milliseconds.  Zero means immediately due.
    pub delay_ms: i64,
}

impl JetJobEnqueue {
    pub fn new(job_type: impl Into<String>, payload: JetJobPayload) -> Self {
        Self {
            job_type: job_type.into(),
            payload,
            idempotency_key: None,
            request_id: None,
            delay_ms: 0,
        }
    }

    pub fn with_key(mut self, key: impl Into<String>) -> Self {
        self.idempotency_key = Some(key.into());
        self
    }

    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }

    pub fn delayed(mut self, delay_ms: i64) -> Self {
        self.delay_ms = delay_ms;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetJobQueuePolicy {
    pub delivery: JetJobQueueDeliveryPolicy,
    pub max_attempts: u32,
    pub lease_ms: i64,
    pub retry_delay_ms: i64,
    pub max_retry_delay_ms: i64,
    pub capacity: usize,
    pub retention_ms: i64,
    pub publish_payload: bool,
}

impl Default for JetJobQueuePolicy {
    fn default() -> Self {
        Self {
            delivery: JetJobQueueDeliveryPolicy::AtLeastOnce,
            max_attempts: 3,
            lease_ms: 30_000,
            retry_delay_ms: 1_000,
            max_retry_delay_ms: 60_000,
            capacity: 1_024,
            retention_ms: 7 * 24 * 60 * 60 * 1_000,
            publish_payload: false,
        }
    }
}

impl JetJobQueuePolicy {
    pub fn validate(&self) -> Result<(), JetServiceError> {
        if self.delivery != JetJobQueueDeliveryPolicy::AtLeastOnce {
            return Err(JetServiceError::Policy(
                "queue delivery policy must be explicit at-least-once".to_string(),
            ));
        }
        if self.max_attempts == 0 || self.max_attempts > JET_JOB_QUEUE_MAX_ATTEMPTS {
            return Err(JetServiceError::Policy(format!(
                "queue max attempts must be between 1 and {JET_JOB_QUEUE_MAX_ATTEMPTS}"
            )));
        }
        if self.lease_ms <= 0 || self.lease_ms > JET_JOB_QUEUE_MAX_LEASE_MS {
            return Err(JetServiceError::Policy(
                "queue lease must be positive and bounded".to_string(),
            ));
        }
        if self.retry_delay_ms < 0
            || self.max_retry_delay_ms < self.retry_delay_ms
            || self.max_retry_delay_ms > JET_JOB_QUEUE_MAX_DELAY_MS
        {
            return Err(JetServiceError::Policy(
                "queue retry delays are outside the supported range".to_string(),
            ));
        }
        if self.capacity == 0 || self.capacity > JET_JOB_QUEUE_MAX_INSPECT {
            return Err(JetServiceError::Policy(
                "queue capacity must be positive and bounded".to_string(),
            ));
        }
        if self.retention_ms < 0 || self.retention_ms > JET_JOB_QUEUE_MAX_DELAY_MS {
            return Err(JetServiceError::Policy(
                "queue retention is outside the supported range".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetJobQueueReceipt {
    pub id: String,
    pub authority: String,
    pub queue: String,
    pub job_type: String,
    pub state: JetJobQueueState,
    pub sequence: i64,
    pub attempts: u32,
    pub due_at_ms: i64,
    pub accepted_at_ms: i64,
    pub request_id: Option<String>,
    pub idempotency_key: Option<String>,
    pub lease_until_ms: Option<i64>,
    pub duration_ms: Option<i64>,
    pub error_reason: Option<String>,
    pub delivery: JetJobQueueDeliveryPolicy,
    pub duplicate: bool,
    pub signature: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetJobQueueClaim {
    pub receipt: JetJobQueueReceipt,
    pub payload: JetJobPayload,
    pub worker: String,
    pub lease_token: String,
    pub lease_until_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetJobQueueEvent {
    pub sequence: i64,
    pub state: JetJobQueueState,
    pub attempts: u32,
    pub timestamp_ms: i64,
    pub reason: Option<String>,
    pub duration_ms: Option<i64>,
    pub worker: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetJobQueueRecord {
    pub receipt: JetJobQueueReceipt,
    pub payload: Option<JetJobPayload>,
    pub result: Option<JetJobResult>,
    pub error: Option<JetJobError>,
    pub started_at_ms: Option<i64>,
    pub finished_at_ms: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetJobQueueStatus {
    pub queue: String,
    pub authority: String,
    pub queued: u64,
    pub running: u64,
    pub retrying: u64,
    pub completed: u64,
    pub failed: u64,
    pub dead_lettered: u64,
    pub cancelled: u64,
    pub depth: u64,
    pub wait_ms: u64,
    pub throughput: u64,
    pub capacity: usize,
    pub paused: bool,
    pub freshness_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetJobQueueWorker {
    pub id: String,
    pub concurrency: usize,
    pub heartbeat_ms: i64,
}

impl JetJobQueueWorker {
    pub fn new(id: impl Into<String>, concurrency: usize, heartbeat_ms: i64) -> Result<Self, JetServiceError> {
        let worker = Self {
            id: id.into(),
            concurrency,
            heartbeat_ms,
        };
        service_authority_validate_text(&worker.id, "queue worker", JET_JOB_QUEUE_MAX_NAME, false)?;
        if worker.concurrency == 0 || worker.concurrency > JET_JOB_QUEUE_MAX_INSPECT {
            return Err(JetServiceError::Policy(
                "queue worker concurrency must be positive and bounded".to_string(),
            ));
        }
        if worker.heartbeat_ms <= 0 || worker.heartbeat_ms > JET_JOB_QUEUE_MAX_LEASE_MS {
            return Err(JetServiceError::Policy(
                "queue worker heartbeat must be positive and bounded".to_string(),
            ));
        }
        Ok(worker)
    }

    pub fn poll<'a>(
        &self,
        queue: &mut JetJobQueue<'a>,
    ) -> Result<Vec<JetJobQueueClaim>, JetServiceError> {
        queue.claim(&self.id, self.concurrency)
    }
}

#[derive(Clone, Debug)]
struct JetJobQueueStoredRecord {
    id: String,
    authority: String,
    queue: String,
    job_type: String,
    payload_type: String,
    payload: Vec<u8>,
    payload_public: bool,
    idempotency_key: Option<String>,
    request_id: Option<String>,
    sequence: i64,
    state: JetJobQueueState,
    due_at_ms: i64,
    accepted_at_ms: i64,
    started_at_ms: Option<i64>,
    finished_at_ms: Option<i64>,
    attempts: u32,
    lease_owner: Option<String>,
    lease_token: Option<String>,
    lease_until_ms: Option<i64>,
    result_type: Option<String>,
    result: Option<Vec<u8>>,
    result_public: bool,
    error_type: Option<String>,
    error_reason: Option<String>,
    error_detail: Option<String>,
    retry_at_ms: Option<i64>,
    updated_at_ms: i64,
    signature: String,
    event_sequence: i64,
}
const JET_JOB_QUEUE_DEVTOOLS_SOURCE: &str = "job-runtime";

#[derive(Clone, Debug)]
struct JetJobQueueDevtoolsTransition {
    event: &'static str,
    authority: String,
    queue: String,
    job_type: String,
    job_id: String,
    sequence: i64,
    state: JetJobQueueState,
    attempts: u32,
    timestamp_ms: i64,
    reason: Option<String>,
    duration_ms: Option<i64>,
    worker: Option<String>,
    request_id: Option<String>,
}

impl JetJobQueueDevtoolsTransition {
    fn from_record(
        record: &JetJobQueueStoredRecord,
        state: JetJobQueueState,
        attempts: u32,
        timestamp_ms: i64,
        reason: Option<&str>,
        duration_ms: Option<i64>,
        worker: Option<&str>,
    ) -> Self {
        let event = match state {
            JetJobQueueState::Queued => "enqueue",
            JetJobQueueState::Running => "start",
            JetJobQueueState::Retrying => "retry",
            JetJobQueueState::Completed => "complete",
            JetJobQueueState::Failed
            | JetJobQueueState::DeadLettered
            | JetJobQueueState::Cancelled => "fail",
        };
        Self {
            event,
            authority: record.authority.clone(),
            queue: record.queue.clone(),
            job_type: record.job_type.clone(),
            job_id: record.id.clone(),
            sequence: record.event_sequence.saturating_add(1),
            state,
            attempts,
            timestamp_ms,
            reason: reason.map(str::to_string),
            duration_ms,
            worker: worker.map(str::to_string),
            request_id: record.request_id.clone(),
        }
    }

    fn with_event(mut self, event: &'static str) -> Self {
        self.event = event;
        self
    }
}

fn jet_job_queue_panel_state(state: JetJobQueueState) -> &'static str {
    match state {
        JetJobQueueState::Queued => "enqueued",
        JetJobQueueState::Running => "started",
        JetJobQueueState::Retrying => "retrying",
        JetJobQueueState::Completed => "completed",
        JetJobQueueState::Failed
        | JetJobQueueState::DeadLettered
        | JetJobQueueState::Cancelled => "failed",
    }
}

fn jet_job_queue_panel_failure(
    state: JetJobQueueState,
    reason: Option<&str>,
) -> Option<&'static str> {
    match state {
        JetJobQueueState::Cancelled => Some("cancelled"),
        JetJobQueueState::Failed | JetJobQueueState::DeadLettered => {
            if reason.map_or(false, |value| value.starts_with("lease_expired")) {
                Some("worker_lost")
            } else {
                Some("error")
            }
        }
        JetJobQueueState::Queued
        | JetJobQueueState::Running
        | JetJobQueueState::Retrying
        | JetJobQueueState::Completed => None,
    }
}

fn jet_job_queue_json_text(value: &str) -> String {
    let mut output = String::with_capacity(value.len().saturating_add(2));
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0c}' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character.is_control() => {
                output.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => output.push(character),
        }
    }
    output.push('"');
    output
}

fn jet_job_queue_json_optional_text(value: Option<&str>) -> String {
    value
        .map(jet_job_queue_json_text)
        .unwrap_or_else(|| "null".to_string())
}

fn jet_job_queue_event_time(timestamp_ms: i64) -> u64 {
    timestamp_ms.max(0) as u64
}

fn jet_job_queue_publish_transition(observation: &JetJobQueueDevtoolsTransition) {
    let duration = observation
        .duration_ms
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_string());
    let worker = jet_job_queue_json_optional_text(observation.worker.as_deref());
    let request_id = jet_job_queue_json_optional_text(observation.request_id.as_deref());
    let failure = jet_job_queue_json_optional_text(jet_job_queue_panel_failure(
        observation.state,
        observation.reason.as_deref(),
    ));
    let reason = jet_job_queue_json_optional_text(observation.reason.as_deref());
    let fields = format!(
        "{{\"event\":{},\"state\":{},\"job_id\":{},\"name\":{},\"queue\":{},\"attempts\":{},\"duration_ms\":{},\"worker\":{},\"request_id\":{},\"failure\":{},\"reason\":{},\"sequence\":{},\"authority\":{},\"labels\":{{}}}}",
        jet_job_queue_json_text(observation.event),
        jet_job_queue_json_text(jet_job_queue_panel_state(observation.state)),
        jet_job_queue_json_text(&observation.job_id),
        jet_job_queue_json_text(&observation.job_type),
        jet_job_queue_json_text(&observation.queue),
        observation.attempts,
        duration,
        worker,
        request_id,
        failure,
        reason,
        observation.sequence,
        jet_job_queue_json_text(&observation.authority),
    );
    if let Ok(event) = JetDevtoolsEvent::from_parts(
        jet_job_queue_event_time(observation.timestamp_ms),
        JET_JOB_QUEUE_DEVTOOLS_SOURCE,
        "Job",
        observation.job_id.clone(),
        fields,
    ) {
        jet_devtools_publish_event(event);
    }
}

fn jet_job_queue_publish_transitions(
    observations: impl IntoIterator<Item = JetJobQueueDevtoolsTransition>,
) {
    for observation in observations {
        jet_job_queue_publish_transition(&observation);
    }
}

fn jet_job_queue_publish_status(status: &JetJobQueueStatus) {
    let entity = format!("{}:{}", status.authority, status.queue);
    let fields = format!(
        "{{\"event\":\"status\",\"state\":\"status\",\"job_id\":{},\"name\":{},\"queue\":{},\"attempts\":0,\"duration_ms\":null,\"worker\":null,\"request_id\":null,\"failure\":null,\"reason\":null,\"sequence\":0,\"authority\":{},\"queued\":{},\"running\":{},\"retrying\":{},\"completed\":{},\"failed\":{},\"dead_lettered\":{},\"cancelled\":{},\"depth\":{},\"wait_ms\":{},\"throughput\":{},\"capacity\":{},\"paused\":{},\"freshness_ms\":{},\"labels\":{{}}}}",
        jet_job_queue_json_text(&entity),
        jet_job_queue_json_text(&status.queue),
        jet_job_queue_json_text(&status.queue),
        jet_job_queue_json_text(&status.authority),
        status.queued,
        status.running,
        status.retrying,
        status.completed,
        status.failed,
        status.dead_lettered,
        status.cancelled,
        status.depth,
        status.wait_ms,
        status.throughput,
        status.capacity,
        status.paused,
        status.freshness_ms,
    );
    if let Ok(event) = JetDevtoolsEvent::from_parts(
        jet_job_queue_event_time(service_authority_now()),
        JET_JOB_QUEUE_DEVTOOLS_SOURCE,
        "Job",
        entity,
        fields,
    ) {
        jet_devtools_publish_event(event);
    }
}


const JET_JOB_QUEUE_SELECT: &str = "SELECT id, authority, queue, job_type, payload_type, payload, payload_public, idempotency_key, request_id, sequence, state, due_at_ms, accepted_at_ms, started_at_ms, finished_at_ms, attempts, lease_owner, lease_token, lease_until_ms, result_type, result, result_public, error_type, error_reason, error_detail, retry_at_ms, updated_at_ms, signature, event_sequence FROM jet_job_queue_jobs";
const JET_JOB_QUEUE_CREATE_META: &str = "CREATE TABLE IF NOT EXISTS jet_job_queue_meta (authority TEXT NOT NULL, queue TEXT NOT NULL, capacity INTEGER NOT NULL, paused INTEGER NOT NULL DEFAULT 0, updated_at_ms INTEGER NOT NULL, PRIMARY KEY (authority, queue))";
const JET_JOB_QUEUE_CREATE_JOBS: &str = "CREATE TABLE IF NOT EXISTS jet_job_queue_jobs (id TEXT PRIMARY KEY, authority TEXT NOT NULL, queue TEXT NOT NULL, job_type TEXT NOT NULL, payload_type TEXT NOT NULL, payload BLOB NOT NULL, payload_public INTEGER NOT NULL, idempotency_key TEXT, request_id TEXT, sequence INTEGER NOT NULL, state TEXT NOT NULL, due_at_ms INTEGER NOT NULL, accepted_at_ms INTEGER NOT NULL, started_at_ms INTEGER, finished_at_ms INTEGER, attempts INTEGER NOT NULL, lease_owner TEXT, lease_token TEXT, lease_until_ms INTEGER, result_type TEXT, result BLOB, result_public INTEGER NOT NULL DEFAULT 0, error_type TEXT, error_reason TEXT, error_detail TEXT, retry_at_ms INTEGER, updated_at_ms INTEGER NOT NULL, signature TEXT NOT NULL, event_sequence INTEGER NOT NULL, UNIQUE(authority, queue, idempotency_key))";
const JET_JOB_QUEUE_CREATE_EVENTS: &str = "CREATE TABLE IF NOT EXISTS jet_job_queue_events (authority TEXT NOT NULL, queue TEXT NOT NULL, job_id TEXT NOT NULL, event_sequence INTEGER NOT NULL, state TEXT NOT NULL, attempts INTEGER NOT NULL, timestamp_ms INTEGER NOT NULL, reason TEXT, duration_ms INTEGER, worker TEXT, PRIMARY KEY (authority, queue, job_id, event_sequence))";
const JET_JOB_QUEUE_CREATE_DUE_INDEX: &str = "CREATE INDEX IF NOT EXISTS jet_job_queue_due ON jet_job_queue_jobs (authority, queue, state, due_at_ms, sequence)";
const JET_JOB_QUEUE_CREATE_EVENT_INDEX: &str = "CREATE INDEX IF NOT EXISTS jet_job_queue_events_time ON jet_job_queue_events (authority, queue, state, timestamp_ms)";

pub type JetJobQueueStoreFactory =
    fn(&str, &str) -> Result<Box<dyn JetJobQueueStore>, JetServiceError>;

static JET_JOB_QUEUE_STORE_FACTORY: std::sync::LazyLock<
    std::sync::Mutex<Option<JetJobQueueStoreFactory>>,
> = std::sync::LazyLock::new(std::sync::Mutex::default);

fn jet_job_queue_store_factory(
) -> &'static std::sync::Mutex<Option<JetJobQueueStoreFactory>> {
    &JET_JOB_QUEUE_STORE_FACTORY
}

/// A provider owns only opening/marshalling a durable database connection.
/// Queue transitions never dispatch through this trait.
pub trait JetJobQueueStoreProvider: Send + Sync {
    fn open(
        &self,
        path: &str,
        authority: &str,
    ) -> Result<Box<dyn JetJobQueueStore>, JetServiceError>;
}

static JET_JOB_QUEUE_STORE_PROVIDER: std::sync::LazyLock<
    std::sync::Mutex<Option<Box<dyn JetJobQueueStoreProvider>>>,
> = std::sync::LazyLock::new(std::sync::Mutex::default);

pub fn jet_job_queue_install_store_provider(
    provider: Option<Box<dyn JetJobQueueStoreProvider>>,
) -> bool {
    let Ok(mut current) = JET_JOB_QUEUE_STORE_PROVIDER.lock() else {
        return false;
    };
    *current = provider;
    true
}

/// Install the built-in provider only when no explicit provider or factory
/// has already been selected.  A configured provider remains authoritative;
/// the default bridge must not silently replace it during first use.
pub fn jet_job_queue_install_store_provider_if_absent(
    provider: Box<dyn JetJobQueueStoreProvider>,
) -> bool {
    let Ok(mut current) = JET_JOB_QUEUE_STORE_PROVIDER.lock() else {
        return false;
    };
    if current.is_some() {
        return true;
    }
    let Ok(factory) = jet_job_queue_store_factory().lock() else {
        return false;
    };
    if factory.is_some() {
        return true;
    }
    *current = Some(provider);
    true
}

/// Install the explicitly selected database provider.  Passing `None` removes
/// the provider; no in-memory or log fallback is installed implicitly.
pub fn jet_job_queue_install_store_factory(
    factory: Option<JetJobQueueStoreFactory>,
) -> Option<JetJobQueueStoreFactory> {
    let mut current = jet_job_queue_store_factory()
        .lock()
        .map_err(|_| ())
        .ok()?;
    Some(std::mem::replace(&mut *current, factory)).flatten()
}

pub fn jet_job_queue_default_path() -> String {
    std::env::var("JET_JOB_QUEUE_DB")
        .ok()
        .filter(|path| !path.is_empty())
        .unwrap_or_else(|| ".jet/state/jobs.db".to_string())
}

fn jet_job_queue_validate_name(value: &str, label: &str) -> Result<(), JetServiceError> {
    service_authority_validate_text(value, label, JET_JOB_QUEUE_MAX_NAME, false)
}

fn jet_job_queue_validate_payload(payload: &JetJobPayload) -> Result<(), JetServiceError> {
    service_authority_validate_text(
        &payload.type_id,
        "job payload type",
        JET_JOB_QUEUE_MAX_TYPE,
        false,
    )?;
    if payload.bytes.len() > JET_JOB_QUEUE_MAX_PAYLOAD {
        return Err(JetServiceError::Policy(
            "job payload exceeds the bounded queue payload limit".to_string(),
        ));
    }
    Ok(())
}

fn jet_job_queue_validate_result(result: &JetJobResult) -> Result<(), JetServiceError> {
    service_authority_validate_text(
        &result.type_id,
        "job result type",
        JET_JOB_QUEUE_MAX_TYPE,
        false,
    )?;
    if result.bytes.len() > JET_JOB_QUEUE_MAX_PAYLOAD {
        return Err(JetServiceError::Policy(
            "job result exceeds the bounded queue payload limit".to_string(),
        ));
    }
    Ok(())
}

fn jet_job_queue_validate_error(error: &JetJobError) -> Result<(), JetServiceError> {
    service_authority_validate_text(
        &error.type_id,
        "job error type",
        JET_JOB_QUEUE_MAX_TYPE,
        false,
    )?;
    service_authority_validate_text(
        &error.reason,
        "job error reason",
        JET_JOB_QUEUE_MAX_REASON,
        false,
    )?;
    if let Some(detail) = &error.detail {
        service_authority_validate_text(detail, "job error detail", JET_JOB_QUEUE_MAX_DETAIL, true)?;
    }
    Ok(())
}

fn jet_job_queue_validate_enqueue(
    request: &JetJobEnqueue,
) -> Result<(), JetServiceError> {
    service_authority_validate_text(
        &request.job_type,
        "job type",
        JET_JOB_QUEUE_MAX_TYPE,
        false,
    )?;
    jet_job_queue_validate_payload(&request.payload)?;
    if request.delay_ms < 0 || request.delay_ms > JET_JOB_QUEUE_MAX_DELAY_MS {
        return Err(JetServiceError::Policy(
            "job delay must be non-negative and bounded".to_string(),
        ));
    }
    if let Some(key) = &request.idempotency_key {
        service_authority_validate_text(key, "job idempotency key", JET_JOB_QUEUE_MAX_KEY, false)?;
    }
    if let Some(request_id) = &request.request_id {
        service_authority_validate_text(
            request_id,
            "job request id",
            JET_JOB_QUEUE_MAX_REQUEST,
            false,
        )?;
    }
    Ok(())
}

fn jet_job_queue_now_ms() -> i64 {
    service_authority_now()
}

fn jet_job_queue_value_text(value: &JetJobQueueValue, label: &str) -> Result<String, JetServiceError> {
    match value {
        JetJobQueueValue::Text(value) => Ok(value.clone()),
        _ => Err(service_authority_error(format!(
            "queue column `{label}` is not text"
        ))),
    }
}

fn jet_job_queue_value_i64(value: &JetJobQueueValue, label: &str) -> Result<i64, JetServiceError> {
    match value {
        JetJobQueueValue::Int(value) => Ok(*value),
        _ => Err(service_authority_error(format!(
            "queue column `{label}` is not an integer"
        ))),
    }
}

fn jet_job_queue_value_blob(value: &JetJobQueueValue, label: &str) -> Result<Vec<u8>, JetServiceError> {
    match value {
        JetJobQueueValue::Blob(value) => Ok(value.clone()),
        _ => Err(service_authority_error(format!(
            "queue column `{label}` is not a blob"
        ))),
    }
}

fn jet_job_queue_row_value<'a>(
    row: &'a JetJobQueueRow,
    name: &str,
) -> Result<&'a JetJobQueueValue, JetServiceError> {
    row.get(name)
        .ok_or_else(|| service_authority_error(format!("queue row has no `{name}` column")))
}

fn jet_job_queue_row_text(row: &JetJobQueueRow, name: &str) -> Result<String, JetServiceError> {
    jet_job_queue_value_text(jet_job_queue_row_value(row, name)?, name)
}

fn jet_job_queue_row_i64(row: &JetJobQueueRow, name: &str) -> Result<i64, JetServiceError> {
    jet_job_queue_value_i64(jet_job_queue_row_value(row, name)?, name)
}

fn jet_job_queue_row_blob(row: &JetJobQueueRow, name: &str) -> Result<Vec<u8>, JetServiceError> {
    jet_job_queue_value_blob(jet_job_queue_row_value(row, name)?, name)
}

fn jet_job_queue_row_optional_text(
    row: &JetJobQueueRow,
    name: &str,
) -> Result<Option<String>, JetServiceError> {
    match jet_job_queue_row_value(row, name)? {
        JetJobQueueValue::Null => Ok(None),
        JetJobQueueValue::Text(value) => Ok(Some(value.clone())),
        _ => Err(service_authority_error(format!(
            "queue column `{name}` is neither text nor null"
        ))),
    }
}

fn jet_job_queue_row_optional_i64(
    row: &JetJobQueueRow,
    name: &str,
) -> Result<Option<i64>, JetServiceError> {
    match jet_job_queue_row_value(row, name)? {
        JetJobQueueValue::Null => Ok(None),
        JetJobQueueValue::Int(value) => Ok(Some(*value)),
        _ => Err(service_authority_error(format!(
            "queue column `{name}` is neither integer nor null"
        ))),
    }
}

fn jet_job_queue_row_bool(row: &JetJobQueueRow, name: &str) -> Result<bool, JetServiceError> {
    match jet_job_queue_row_i64(row, name)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(service_authority_error(format!(
            "queue column `{name}` is not a boolean integer"
        ))),
    }
}

fn jet_job_queue_row_optional_blob(
    row: &JetJobQueueRow,
    name: &str,
) -> Result<Option<Vec<u8>>, JetServiceError> {
    match jet_job_queue_row_value(row, name)? {
        JetJobQueueValue::Null => Ok(None),
        JetJobQueueValue::Blob(value) => Ok(Some(value.clone())),
        _ => Err(service_authority_error(format!(
            "queue column `{name}` is neither blob nor null"
        ))),
    }
}

fn jet_job_queue_u32(value: i64, label: &str) -> Result<u32, JetServiceError> {
    u32::try_from(value).map_err(|_| {
        service_authority_error(format!("queue {label} is outside the supported range"))
    })
}

fn jet_job_queue_usize(value: i64, label: &str) -> Result<usize, JetServiceError> {
    usize::try_from(value).map_err(|_| {
        service_authority_error(format!("queue {label} is outside the supported range"))
    })
}

fn jet_job_queue_state_from_row(
    row: &JetJobQueueRow,
) -> Result<JetJobQueueState, JetServiceError> {
    JetJobQueueState::parse(&jet_job_queue_row_text(row, "state")?)
}

fn jet_job_queue_stored_record(
    row: &JetJobQueueRow,
) -> Result<JetJobQueueStoredRecord, JetServiceError> {
    Ok(JetJobQueueStoredRecord {
        id: jet_job_queue_row_text(row, "id")?,
        authority: jet_job_queue_row_text(row, "authority")?,
        queue: jet_job_queue_row_text(row, "queue")?,
        job_type: jet_job_queue_row_text(row, "job_type")?,
        payload_type: jet_job_queue_row_text(row, "payload_type")?,
        payload: jet_job_queue_row_blob(row, "payload")?,
        payload_public: jet_job_queue_row_bool(row, "payload_public")?,
        idempotency_key: jet_job_queue_row_optional_text(row, "idempotency_key")?,
        request_id: jet_job_queue_row_optional_text(row, "request_id")?,
        sequence: jet_job_queue_row_i64(row, "sequence")?,
        state: jet_job_queue_state_from_row(row)?,
        due_at_ms: jet_job_queue_row_i64(row, "due_at_ms")?,
        accepted_at_ms: jet_job_queue_row_i64(row, "accepted_at_ms")?,
        started_at_ms: jet_job_queue_row_optional_i64(row, "started_at_ms")?,
        finished_at_ms: jet_job_queue_row_optional_i64(row, "finished_at_ms")?,
        attempts: jet_job_queue_u32(jet_job_queue_row_i64(row, "attempts")?, "attempts")?,
        lease_owner: jet_job_queue_row_optional_text(row, "lease_owner")?,
        lease_token: jet_job_queue_row_optional_text(row, "lease_token")?,
        lease_until_ms: jet_job_queue_row_optional_i64(row, "lease_until_ms")?,
        result_type: jet_job_queue_row_optional_text(row, "result_type")?,
        result: jet_job_queue_row_optional_blob(row, "result")?,
        result_public: jet_job_queue_row_bool(row, "result_public")?,
        error_type: jet_job_queue_row_optional_text(row, "error_type")?,
        error_reason: jet_job_queue_row_optional_text(row, "error_reason")?,
        error_detail: jet_job_queue_row_optional_text(row, "error_detail")?,
        retry_at_ms: jet_job_queue_row_optional_i64(row, "retry_at_ms")?,
        updated_at_ms: jet_job_queue_row_i64(row, "updated_at_ms")?,
        signature: jet_job_queue_row_text(row, "signature")?,
        event_sequence: jet_job_queue_row_i64(row, "event_sequence")?,
    })
}

fn jet_job_queue_framed(input: &mut Vec<u8>, field: &[u8]) {
    input.extend_from_slice(&(field.len() as u64).to_be_bytes());
    input.extend_from_slice(field);
}

fn jet_job_queue_record_signature(
    authority: &str,
    id: &str,
    queue: &str,
    job_type: &str,
    sequence: i64,
    due_at_ms: i64,
    payload: &[u8],
) -> Result<String, JetServiceError> {
    let key = service_authority_signing_key(authority)?;
    let payload_digest = jet_sha256_raw(payload);
    let mut input = Vec::with_capacity(128);
    jet_job_queue_framed(&mut input, b"jet-job-queue-v1");
    jet_job_queue_framed(&mut input, authority.as_bytes());
    jet_job_queue_framed(&mut input, id.as_bytes());
    jet_job_queue_framed(&mut input, queue.as_bytes());
    jet_job_queue_framed(&mut input, job_type.as_bytes());
    jet_job_queue_framed(&mut input, sequence.to_string().as_bytes());
    jet_job_queue_framed(&mut input, due_at_ms.to_string().as_bytes());
    jet_job_queue_framed(&mut input, &payload_digest);
    Ok(service_authority_hex(&jet_hmac_sha256(&key, &input)))
}

fn jet_job_queue_id(
    authority: &str,
    queue: &str,
    sequence: i64,
    job_type: &str,
    payload: &[u8],
) -> String {
    let payload_digest = jet_sha256_raw(payload);
    let mut input = Vec::with_capacity(128);
    jet_job_queue_framed(&mut input, b"jet-job-id-v1");
    jet_job_queue_framed(&mut input, authority.as_bytes());
    jet_job_queue_framed(&mut input, queue.as_bytes());
    jet_job_queue_framed(&mut input, sequence.to_string().as_bytes());
    jet_job_queue_framed(&mut input, job_type.as_bytes());
    jet_job_queue_framed(&mut input, &payload_digest);
    format!("jq-{}", service_authority_hex(&jet_sha256_raw(&input)))
}

fn jet_job_queue_make_receipt(
    record: &JetJobQueueStoredRecord,
    delivery: JetJobQueueDeliveryPolicy,
    duplicate: bool,
) -> JetJobQueueReceipt {
    JetJobQueueReceipt {
        id: record.id.clone(),
        authority: record.authority.clone(),
        queue: record.queue.clone(),
        job_type: record.job_type.clone(),
        state: record.state,
        sequence: record.sequence,
        attempts: record.attempts,
        due_at_ms: record.due_at_ms,
        accepted_at_ms: record.accepted_at_ms,
        request_id: record.request_id.clone(),
        idempotency_key: record.idempotency_key.clone(),
        lease_until_ms: record.lease_until_ms,
        duration_ms: record
            .started_at_ms
            .zip(record.finished_at_ms)
            .map(|(started, finished)| finished.saturating_sub(started)),
        error_reason: record.error_reason.clone(),
        delivery,
        duplicate,
        signature: record.signature.clone(),
}
}

fn jet_job_queue_record(
    record: &JetJobQueueStoredRecord,
    delivery: JetJobQueueDeliveryPolicy,
    include_payload: bool,
) -> JetJobQueueRecord {
    let payload = include_payload && record.payload_public;
    let payload = payload.then(|| JetJobPayload {
        type_id: record.payload_type.clone(),
        bytes: record.payload.clone(),
        publish: true,
    });
    let result = if include_payload && record.result_public {
        record.result.as_ref().zip(record.result_type.as_ref()).map(|(bytes, type_id)| {
            JetJobResult {
                type_id: type_id.clone(),
                bytes: bytes.clone(),
                publish: true,
            }
        })
    } else {
        None
    };
    let error = record
        .error_type
        .as_ref()
        .zip(record.error_reason.as_ref())
        .map(|(type_id, reason)| JetJobError {
            type_id: type_id.clone(),
            reason: reason.clone(),
            detail: record.error_detail.clone(),
        });
    JetJobQueueRecord {
        receipt: jet_job_queue_make_receipt(record, delivery, false),
        payload,
        result,
        error,
        started_at_ms: record.started_at_ms,
        finished_at_ms: record.finished_at_ms,
    }
}

fn jet_job_queue_backoff(policy: &JetJobQueuePolicy, attempts: u32) -> i64 {
    let exponent = attempts.saturating_sub(1).min(20);
    let multiplier = 1_i64.checked_shl(exponent).unwrap_or(i64::MAX);
    policy
        .retry_delay_ms
        .saturating_mul(multiplier)
        .min(policy.max_retry_delay_ms)
}

fn jet_job_queue_token(id: &str, worker: &str) -> Result<String, JetServiceError> {
    let mut entropy = jet_crypto_entropy_bytes(24).map_err(|_| {
        JetServiceError::Policy("queue lease token could not obtain cryptographic entropy".to_string())
    })?;
    let mut input = Vec::with_capacity(64);
    jet_job_queue_framed(&mut input, id.as_bytes());
    jet_job_queue_framed(&mut input, worker.as_bytes());
    jet_job_queue_framed(&mut input, &jet_job_queue_now_ms().to_string().into_bytes());
    input.append(&mut entropy);
    Ok(format!("lease-{}", service_authority_hex(&jet_sha256_raw(&input))))
}

fn jet_job_queue_sql_error(operation: &str, detail: JetServiceError) -> JetServiceError {
    match detail {
        JetServiceError::Unavailable(message) => {
            JetServiceError::Unavailable(format!("queue {operation}: {message}"))
        }
        JetServiceError::Policy(message) => {
            JetServiceError::Policy(format!("queue {operation}: {message}"))
        }
        JetServiceError::Revoked(message) => {
            JetServiceError::Revoked(format!("queue {operation}: {message}"))
        }
        JetServiceError::Stale(message) => {
            JetServiceError::Stale(format!("queue {operation}: {message}"))
        }
        other => other,
    }
}

pub struct JetJobQueue<'a> {
    store: Box<dyn JetJobQueueStore + 'a>,
    authority: String,
    name: String,
    policy: JetJobQueuePolicy,
    endpoint: JetServiceEndpoint,
}

impl<'a> JetJobQueue<'a> {
    fn with_store_validated(
        store: Box<dyn JetJobQueueStore + 'a>,
        name: String,
        policy: JetJobQueuePolicy,
        endpoint: JetServiceEndpoint,
    ) -> Result<Self, JetServiceError> {
        jet_job_queue_validate_name(&name, "job queue")?;
        policy.validate()?;
        let mut queue = Self {
            store,
            authority: endpoint.authority.clone(),
            name,
            policy,
            endpoint,
        };
        queue.initialize()?;
        Ok(queue)
    }

    /// Construct a queue around an explicit provider and an already-issued
    /// service endpoint.  The provider is the only replaceable layer; all
    /// state transitions below remain shared.
    pub fn with_store(
        store: Box<dyn JetJobQueueStore + 'a>,
        endpoint: &JetServiceEndpoint,
        name: String,
        policy: JetJobQueuePolicy,
    ) -> Result<Self, JetServiceError> {
        jet_services_authority_validate(endpoint)?;
        Self::with_store_validated(store, name, policy, endpoint.clone())
    }

    pub fn open_default(
        endpoint: &JetServiceEndpoint,
        name: String,
        policy: JetJobQueuePolicy,
    ) -> Result<JetJobQueue<'static>, JetServiceError> {
        jet_services_authority_validate(endpoint)?;
        Self::open_default_validated(endpoint.clone(), name, policy)
    }

    fn open_default_validated(
        endpoint: JetServiceEndpoint,
        name: String,
        policy: JetJobQueuePolicy,
    ) -> Result<JetJobQueue<'static>, JetServiceError> {
        let path = jet_job_queue_default_path();
        let authority = endpoint.authority.clone();
        let provider_store = JET_JOB_QUEUE_STORE_PROVIDER
            .lock()
            .map_err(|_| service_authority_error("queue provider registry is poisoned"))?
            .as_ref()
            .map(|provider| provider.open(&path, &authority))
            .transpose()?;
        if let Some(store) = provider_store {
            return JetJobQueue::<'static>::with_store_validated(
                store,
                name,
                policy,
                endpoint.clone(),
            );
        }
        let factory = jet_job_queue_store_factory()
            .lock()
            .map_err(|_| service_authority_error("queue provider registry is poisoned"))?
            .ok_or_else(|| {
                JetServiceError::Unavailable(
                    "no SQLite queue provider is installed; select a provider explicitly"
                        .to_string(),
                )
            })?;
        let store = factory(&path, &authority)?;
        JetJobQueue::<'static>::with_store_validated(store, name, policy, endpoint)
    }

    pub fn authority(&self) -> &str {
        &self.authority
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn policy(&self) -> &JetJobQueuePolicy {
        &self.policy
    }
    fn validate_endpoint(&self) -> Result<(), JetServiceError> {
        jet_services_authority_validate(&self.endpoint)
    }

    fn initialize(&mut self) -> Result<(), JetServiceError> {
        self.transaction(|queue, store| {
            for statement in [
                JET_JOB_QUEUE_CREATE_META,
                JET_JOB_QUEUE_CREATE_JOBS,
                JET_JOB_QUEUE_CREATE_EVENTS,
                JET_JOB_QUEUE_CREATE_DUE_INDEX,
                JET_JOB_QUEUE_CREATE_EVENT_INDEX,
            ] {
                store
                    .execute(statement, &[])
                    .map_err(|error| jet_job_queue_sql_error("schema", error))?;
            }
            let now = jet_job_queue_now_ms();
            store
                .execute(
                    "INSERT OR IGNORE INTO jet_job_queue_meta (authority, queue, capacity, paused, updated_at_ms) VALUES (?, ?, ?, 0, ?)",
                    &[
                        JetJobQueueValue::Text(queue.authority.clone()),
                        JetJobQueueValue::Text(queue.name.clone()),
                        JetJobQueueValue::Int(queue.policy.capacity as i64),
                        JetJobQueueValue::Int(now),
                    ],
                )
                .map_err(|error| jet_job_queue_sql_error("metadata", error))?;
            store
                .execute(
                    "UPDATE jet_job_queue_meta SET capacity = ?, updated_at_ms = ? WHERE authority = ? AND queue = ?",
                    &[
                        JetJobQueueValue::Int(queue.policy.capacity as i64),
                        JetJobQueueValue::Int(now),
                        JetJobQueueValue::Text(queue.authority.clone()),
                        JetJobQueueValue::Text(queue.name.clone()),
                    ],
                )
                .map_err(|error| jet_job_queue_sql_error("metadata", error))?;
            Ok(())
        })?;
        let _ = self.status()?;
        Ok(())
    }

    fn transaction<T>(
        &mut self,
        operation: impl FnOnce(&mut Self, &mut dyn JetJobQueueStore) -> Result<T, JetServiceError>,
    ) -> Result<T, JetServiceError> {
        self.validate_endpoint()?;
        let mut store = std::mem::replace(
            &mut self.store,
            Box::new(JetJobQueueNoopStore),
        );
        if let Err(error) = store.begin() {
            self.store = store;
            return Err(jet_job_queue_sql_error("begin", error));
        }
        let result = operation(self, store.as_mut());
        let result = match result {
            Ok(value) => match store.commit() {
                Ok(()) => Ok(value),
                Err(error) => {
                    let _ = store.rollback();
                    Err(jet_job_queue_sql_error("commit", error))
                }
            },
            Err(error) => {
                store.rollback();
                Err(error)
            }
        };
        self.store = store;
        result
    }

    fn queue_params(&self) -> [JetJobQueueValue; 2] {
        [
            JetJobQueueValue::Text(self.authority.clone()),
            JetJobQueueValue::Text(self.name.clone()),
        ]
    }

    fn next_sequence(
        &self,
        store: &mut dyn JetJobQueueStore,
    ) -> Result<i64, JetServiceError> {
        let rows = store
            .query(
                "SELECT COALESCE(MAX(sequence), 0) AS sequence FROM jet_job_queue_jobs WHERE authority = ? AND queue = ?",
                &self.queue_params(),
            )
            .map_err(|error| jet_job_queue_sql_error("sequence", error))?;
        let next = rows
            .first()
            .map(|row| jet_job_queue_row_i64(row, "sequence"))
            .transpose()?
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| JetServiceError::Full("queue sequence exhausted".to_string()))?;
        Ok(next)
    }

    fn find_by_key(
        &self,
        store: &mut dyn JetJobQueueStore,
        key: &str,
    ) -> Result<Option<JetJobQueueStoredRecord>, JetServiceError> {
        let rows = store
            .query(
                &format!(
                    "{JET_JOB_QUEUE_SELECT} WHERE authority = ? AND queue = ? AND idempotency_key = ? LIMIT 1"
                ),
                &[
                    JetJobQueueValue::Text(self.authority.clone()),
                    JetJobQueueValue::Text(self.name.clone()),
                    JetJobQueueValue::Text(key.to_string()),
                ],
            )
            .map_err(|error| jet_job_queue_sql_error("idempotency lookup", error))?;
        rows.first().map(jet_job_queue_stored_record).transpose()
    }

    fn find_by_id(
        &self,
        store: &mut dyn JetJobQueueStore,
        id: &str,
    ) -> Result<JetJobQueueStoredRecord, JetServiceError> {
        let rows = store
            .query(
                &format!("{JET_JOB_QUEUE_SELECT} WHERE authority = ? AND queue = ? AND id = ? LIMIT 1"),
                &[
                    JetJobQueueValue::Text(self.authority.clone()),
                    JetJobQueueValue::Text(self.name.clone()),
                    JetJobQueueValue::Text(id.to_string()),
                ],
            )
            .map_err(|error| jet_job_queue_sql_error("receipt lookup", error))?;
        rows.first()
            .ok_or_else(|| JetServiceError::Unknown(format!("queue job `{id}` is unknown")))
            .and_then(jet_job_queue_stored_record)
    }

    fn insert_event(
        &self,
        store: &mut dyn JetJobQueueStore,
        record: &JetJobQueueStoredRecord,
        state: JetJobQueueState,
        attempts: u32,
        timestamp_ms: i64,
        reason: Option<&str>,
        duration_ms: Option<i64>,
        worker: Option<&str>,
    ) -> Result<(), JetServiceError> {
        store
            .execute(
                "INSERT INTO jet_job_queue_events (authority, queue, job_id, event_sequence, state, attempts, timestamp_ms, reason, duration_ms, worker) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                &[
                    JetJobQueueValue::Text(self.authority.clone()),
                    JetJobQueueValue::Text(self.name.clone()),
                    JetJobQueueValue::Text(record.id.clone()),
                    JetJobQueueValue::Int(record.event_sequence.saturating_add(1)),
                    JetJobQueueValue::Text(state.as_str().to_string()),
                    JetJobQueueValue::Int(i64::from(attempts)),
                    JetJobQueueValue::Int(timestamp_ms),
                    reason.map_or(JetJobQueueValue::Null, |value| {
                        JetJobQueueValue::Text(value.to_string())
                    }),
                    duration_ms.map_or(JetJobQueueValue::Null, JetJobQueueValue::Int),
                    worker.map_or(JetJobQueueValue::Null, |value| {
                        JetJobQueueValue::Text(value.to_string())
                    }),
                ],
            )
            .map_err(|error| jet_job_queue_sql_error("event", error))?;
        Ok(())
    }
}

impl<'a> JetJobQueue<'a> {
    pub fn enqueue(&mut self, request: JetJobEnqueue) -> Result<JetJobQueueReceipt, JetServiceError> {
        let mut receipts = self.enqueue_bulk(vec![request])?;
        receipts
            .pop()
            .ok_or_else(|| service_authority_error("queue enqueue returned no receipt"))
    }

    pub fn enqueue_delayed(
        &mut self,
        job_type: impl Into<String>,
        payload: JetJobPayload,
        delay_ms: i64,
        idempotency_key: Option<String>,
        request_id: Option<String>,
    ) -> Result<JetJobQueueReceipt, JetServiceError> {
        self.enqueue(
            JetJobEnqueue {
                job_type: job_type.into(),
                payload,
                idempotency_key,
                request_id,
                delay_ms,
            },
        )
    }

    pub fn enqueue_bulk(
        &mut self,
        requests: Vec<JetJobEnqueue>,
    ) -> Result<Vec<JetJobQueueReceipt>, JetServiceError> {
        if requests.is_empty() {
            return Err(JetServiceError::Policy(
                "queue bulk enqueue requires at least one job".to_string(),
            ));
        }
        if requests.len() > JET_JOB_QUEUE_MAX_BULK {
            return Err(JetServiceError::Full(format!(
                "queue bulk enqueue is limited to {JET_JOB_QUEUE_MAX_BULK} jobs"
            )));
        }
        for request in &requests {
            jet_job_queue_validate_enqueue(request)?;
            if request.payload.type_id != request.job_type {
                return Err(JetServiceError::Policy(
                    "job payload type must match the declared job type".to_string(),
                ));
            }
        }
        let now = jet_job_queue_now_ms();
        self.transaction(|queue, store| {
            let mut next_sequence = queue.next_sequence(store)?;
            let mut receipts = Vec::with_capacity(requests.len());
            let mut observations = Vec::with_capacity(requests.len());
            for request in requests {
                if let Some(key) = request.idempotency_key.as_deref() {
                    if let Some(existing) = queue.find_by_key(store, key)? {
                        if existing.job_type != request.job_type
                            || existing.payload_type != request.payload.type_id
                            || existing.payload != request.payload.bytes
                        {
                            return Err(JetServiceError::Policy(
                                "job idempotency key was already used by another job".to_string(),
                            ));
                        }
                        receipts.push(jet_job_queue_make_receipt(
                            &existing,
                            queue.policy.delivery,
                            true,
                        ));
                        continue;
                    }
                }
                let depth_rows = store
                    .query(
                        "SELECT COUNT(*) AS depth FROM jet_job_queue_jobs WHERE authority = ? AND queue = ? AND state IN ('queued', 'running', 'retrying')",
                        &queue.queue_params(),
                    )
                    .map_err(|error| jet_job_queue_sql_error("capacity", error))?;
                let depth = depth_rows
                    .first()
                    .map(|row| jet_job_queue_u64(jet_job_queue_row_i64(row, "depth")?, "depth"))
                    .transpose()?
                    .unwrap_or(0);
                if depth >= queue.policy.capacity as u64 {
                    return Err(JetServiceError::Full(
                        "queue capacity has been reached".to_string(),
                    ));
                }
                let due_at_ms = now
                    .checked_add(request.delay_ms)
                    .ok_or_else(|| JetServiceError::Policy("job due time overflowed".to_string()))?;
                let sequence = next_sequence;
                next_sequence = next_sequence
                    .checked_add(1)
                    .ok_or_else(|| JetServiceError::Full("queue sequence exhausted".to_string()))?;
                let id = jet_job_queue_id(
                    &queue.authority,
                    &queue.name,
                    sequence,
                    &request.job_type,
                    &request.payload.bytes,
                );
                let signature = jet_job_queue_record_signature(
                    &queue.authority,
                    &id,
                    &queue.name,
                    &request.job_type,
                    sequence,
                    due_at_ms,
                    &request.payload.bytes,
                )?;
                let payload_public = request.payload.publish && queue.policy.publish_payload;
                let record = JetJobQueueStoredRecord {
                    id: id.clone(),
                    authority: queue.authority.clone(),
                    queue: queue.name.clone(),
                    job_type: request.job_type.clone(),
                    payload_type: request.payload.type_id.clone(),
                    payload: request.payload.bytes.clone(),
                    payload_public,
                    idempotency_key: request.idempotency_key.clone(),
                    request_id: request.request_id.clone(),
                    sequence,
                    state: JetJobQueueState::Queued,
                    due_at_ms,
                    accepted_at_ms: now,
                    started_at_ms: None,
                    finished_at_ms: None,
                    attempts: 0,
                    lease_owner: None,
                    lease_token: None,
                    lease_until_ms: None,
                    result_type: None,
                    result: None,
                    result_public: false,
                    error_type: None,
                    error_reason: None,
                    error_detail: None,
                    retry_at_ms: None,
                    updated_at_ms: now,
                    signature,
                    event_sequence: 0,
                };
                store
                    .execute(
                        "INSERT INTO jet_job_queue_jobs (id, authority, queue, job_type, payload_type, payload, payload_public, idempotency_key, request_id, sequence, state, due_at_ms, accepted_at_ms, started_at_ms, finished_at_ms, attempts, lease_owner, lease_token, lease_until_ms, result_type, result, result_public, error_type, error_reason, error_detail, retry_at_ms, updated_at_ms, signature, event_sequence) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                        &[
                            JetJobQueueValue::Text(record.id.clone()),
                            JetJobQueueValue::Text(record.authority.clone()),
                            JetJobQueueValue::Text(record.queue.clone()),
                            JetJobQueueValue::Text(record.job_type.clone()),
                            JetJobQueueValue::Text(record.payload_type.clone()),
                            JetJobQueueValue::Blob(record.payload.clone()),
                            JetJobQueueValue::Int(if record.payload_public { 1 } else { 0 }),
                            record.idempotency_key.clone().map_or(JetJobQueueValue::Null, JetJobQueueValue::Text),
                            record.request_id.clone().map_or(JetJobQueueValue::Null, JetJobQueueValue::Text),
                            JetJobQueueValue::Int(record.sequence),
                            JetJobQueueValue::Text(record.state.as_str().to_string()),
                            JetJobQueueValue::Int(record.due_at_ms),
                            JetJobQueueValue::Int(record.accepted_at_ms),
                            JetJobQueueValue::Null,
                            JetJobQueueValue::Null,
                            JetJobQueueValue::Int(i64::from(record.attempts)),
                            JetJobQueueValue::Null,
                            JetJobQueueValue::Null,
                            JetJobQueueValue::Null,
                            JetJobQueueValue::Null,
                            JetJobQueueValue::Null,
                            JetJobQueueValue::Int(0),
                            JetJobQueueValue::Null,
                            JetJobQueueValue::Null,
                            JetJobQueueValue::Null,
                            JetJobQueueValue::Null,
                            JetJobQueueValue::Int(record.updated_at_ms),
                            JetJobQueueValue::Text(record.signature.clone()),
                            JetJobQueueValue::Int(1),
                        ],
                    )
                    .map_err(|error| jet_job_queue_sql_error("enqueue", error))?;
                queue.insert_event(
                    store,
                    &record,
                    JetJobQueueState::Queued,
                    0,
                    now,
                    Some("accepted"),
                    None,
                    None,
                )?;
                observations.push(JetJobQueueDevtoolsTransition::from_record(
                    &record,
                    JetJobQueueState::Queued,
                    0,
                    now,
                    Some("accepted"),
                    None,
                    None,
                ));
                receipts.push(jet_job_queue_make_receipt(
                    &record,
                    queue.policy.delivery,
                    false,
                ));
            }
            Ok((receipts, observations))
        })
        .map(|(receipts, observations)| {
            jet_job_queue_publish_transitions(observations);
            receipts
        })
    }

    pub fn receipt(&mut self, id: &str) -> Result<JetJobQueueReceipt, JetServiceError> {
        self.validate_endpoint()?;
        service_authority_validate_text(id, "queue job id", JET_JOB_QUEUE_MAX_KEY, false)?;
        let record =
            jet_job_queue_find_stored(self.store.as_mut(), &self.authority, &self.name, id)?;
        Ok(jet_job_queue_make_receipt(&record, self.policy.delivery, false))
    }

    pub fn inspect(
        &mut self,
        limit: usize,
        include_payload: bool,
    ) -> Result<Vec<JetJobQueueRecord>, JetServiceError> {
        self.validate_endpoint()?;
        if limit == 0 || limit > JET_JOB_QUEUE_MAX_INSPECT {
            return Err(JetServiceError::Policy(
                "queue inspection limit must be positive and bounded".to_string(),
            ));
        }
        let rows = self
            .store
            .query(
                &format!(
                    "{JET_JOB_QUEUE_SELECT} WHERE authority = ? AND queue = ? ORDER BY sequence LIMIT ?"
                ),
                &[
                    JetJobQueueValue::Text(self.authority.clone()),
                    JetJobQueueValue::Text(self.name.clone()),
                    JetJobQueueValue::Int(limit as i64),
                ],
            )
            .map_err(|error| jet_job_queue_sql_error("inspection", error))?;
        rows.iter()
            .map(jet_job_queue_stored_record)
            .map(|record| {
                record.map(|record| {
                    jet_job_queue_record(
                        &record,
                        self.policy.delivery,
                        include_payload && self.policy.publish_payload,
                    )
                })
            })
            .collect()
    }

    pub fn events(&mut self, id: &str) -> Result<Vec<JetJobQueueEvent>, JetServiceError> {
        self.validate_endpoint()?;
        service_authority_validate_text(id, "queue job id", JET_JOB_QUEUE_MAX_KEY, false)?;
        let record =
            jet_job_queue_find_stored(self.store.as_mut(), &self.authority, &self.name, id)?;
        let rows = self
            .store
            .query(
                "SELECT event_sequence, state, attempts, timestamp_ms, reason, duration_ms, worker FROM jet_job_queue_events WHERE authority = ? AND queue = ? AND job_id = ? ORDER BY event_sequence",
                &[
                    JetJobQueueValue::Text(self.authority.clone()),
                    JetJobQueueValue::Text(self.name.clone()),
                    JetJobQueueValue::Text(record.id.clone()),
                ],
            )
            .map_err(|error| jet_job_queue_sql_error("events", error))?;
        rows.iter()
            .map(|row| {
                Ok(JetJobQueueEvent {
                    sequence: jet_job_queue_row_i64(row, "event_sequence")?,
                    state: JetJobQueueState::parse(&jet_job_queue_row_text(row, "state")?)?,
                    attempts: jet_job_queue_u32(
                        jet_job_queue_row_i64(row, "attempts")?,
                        "event attempts",
                    )?,
                    timestamp_ms: jet_job_queue_row_i64(row, "timestamp_ms")?,
                    reason: jet_job_queue_row_optional_text(row, "reason")?,
                    duration_ms: jet_job_queue_row_optional_i64(row, "duration_ms")?,
                    worker: jet_job_queue_row_optional_text(row, "worker")?,
                })
            })
            .collect()
    }
}

fn jet_job_queue_insert_event(
    store: &mut dyn JetJobQueueStore,
    record: &JetJobQueueStoredRecord,
    state: JetJobQueueState,
    attempts: u32,
    timestamp_ms: i64,
    reason: Option<&str>,
    duration_ms: Option<i64>,
    worker: Option<&str>,
) -> Result<(), JetServiceError> {
    store
        .execute(
            "INSERT INTO jet_job_queue_events (authority, queue, job_id, event_sequence, state, attempts, timestamp_ms, reason, duration_ms, worker) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            &[
                JetJobQueueValue::Text(record.authority.clone()),
                JetJobQueueValue::Text(record.queue.clone()),
                JetJobQueueValue::Text(record.id.clone()),
                JetJobQueueValue::Int(record.event_sequence.saturating_add(1)),
                JetJobQueueValue::Text(state.as_str().to_string()),
                JetJobQueueValue::Int(i64::from(attempts)),
                JetJobQueueValue::Int(timestamp_ms),
                reason.map_or(JetJobQueueValue::Null, |value| {
                    JetJobQueueValue::Text(value.to_string())
                }),
                duration_ms.map_or(JetJobQueueValue::Null, JetJobQueueValue::Int),
                worker.map_or(JetJobQueueValue::Null, |value| {
                    JetJobQueueValue::Text(value.to_string())
                }),
            ],
        )
        .map_err(|error| jet_job_queue_sql_error("event", error))?;
    Ok(())
}

fn jet_job_queue_update_event(
    store: &mut dyn JetJobQueueStore,
    record: &JetJobQueueStoredRecord,
    state: JetJobQueueState,
    attempts: u32,
    now: i64,
    reason: Option<&str>,
    duration_ms: Option<i64>,
    worker: Option<&str>,
    extra: &[(&str, JetJobQueueValue)],
) -> Result<(), JetServiceError> {
    let event_sequence = record.event_sequence.saturating_add(1);
    let mut assignments = String::from(
        "state = ?, attempts = ?, event_sequence = ?, updated_at_ms = ?",
    );
    for (column, _) in extra {
        assignments.push_str(", ");
        assignments.push_str(column);
        assignments.push_str(" = ?");
    }
    let mut params = vec![
        JetJobQueueValue::Text(state.as_str().to_string()),
        JetJobQueueValue::Int(i64::from(attempts)),
        JetJobQueueValue::Int(event_sequence),
        JetJobQueueValue::Int(now),
    ];
    params.extend(extra.iter().map(|(_, value)| value.clone()));
    params.extend([
        JetJobQueueValue::Text(record.authority.clone()),
        JetJobQueueValue::Text(record.queue.clone()),
        JetJobQueueValue::Text(record.id.clone()),
    ]);
    let updated = store
        .execute(
            &format!("UPDATE jet_job_queue_jobs SET {assignments} WHERE authority = ? AND queue = ? AND id = ?"),
            &params,
        )
        .map_err(|error| jet_job_queue_sql_error("transition", error))?;
    if updated != 1 {
        return Err(JetServiceError::Stale(
            "queue job changed before its transition was committed".to_string(),
        ));
    }
    jet_job_queue_insert_event(
        store,
        record,
        state,
        attempts,
        now,
        reason,
        duration_ms,
        worker,
    )
}

/// Temporary move sentinel used only while a queue transaction owns its store.
/// It is never wrapped in a `JetJobQueue` or used for a transition.
struct JetJobQueueNoopStore;

impl JetJobQueueStore for JetJobQueueNoopStore {
    fn begin(&mut self) -> Result<(), JetServiceError> {
        Err(service_authority_error("queue transaction sentinel cannot begin"))
    }

    fn commit(&mut self) -> Result<(), JetServiceError> {
        Err(service_authority_error("queue transaction sentinel cannot commit"))
    }

    fn rollback(&mut self) {}

    fn execute(
        &mut self,
        _sql: &str,
        _params: &[JetJobQueueValue],
    ) -> Result<i64, JetServiceError> {
        Err(service_authority_error("queue transaction sentinel cannot execute"))
    }

    fn query(
        &mut self,
        _sql: &str,
        _params: &[JetJobQueueValue],
    ) -> Result<Vec<JetJobQueueRow>, JetServiceError> {
        Err(service_authority_error("queue transaction sentinel cannot query"))
    }
}

fn jet_job_queue_recover_expired_on_store(
    store: &mut dyn JetJobQueueStore,
    authority: &str,
    name: &str,
    policy: &JetJobQueuePolicy,
    now: i64,
) -> Result<Vec<JetJobQueueDevtoolsTransition>, JetServiceError> {
    let rows = store
        .query(
            &format!(
                "{JET_JOB_QUEUE_SELECT} WHERE authority = ? AND queue = ? AND state = 'running' AND lease_until_ms IS NOT NULL AND lease_until_ms <= ? ORDER BY sequence"
            ),
            &[
                JetJobQueueValue::Text(authority.to_string()),
                JetJobQueueValue::Text(name.to_string()),
                JetJobQueueValue::Int(now),

            ],
        )
        .map_err(|error| jet_job_queue_sql_error("lease recovery", error))?;
    let mut observations = Vec::with_capacity(rows.len());
    for row in &rows {
        let record = jet_job_queue_stored_record(row)?;
        let terminal = record.attempts >= policy.max_attempts;
        let state = if terminal {
            JetJobQueueState::DeadLettered
        } else {
            JetJobQueueState::Retrying
        };
        let reason = if terminal {
            "lease_expired_max_attempts"
        } else {
            "lease_expired"
        };
        let duration_ms = record
            .started_at_ms
            .map(|started| now.saturating_sub(started));
        let extra = [
            ("finished_at_ms", JetJobQueueValue::Int(now)),
            ("due_at_ms", JetJobQueueValue::Int(now)),
            ("retry_at_ms", JetJobQueueValue::Int(now)),
            ("lease_owner", JetJobQueueValue::Null),
            ("lease_token", JetJobQueueValue::Null),
            ("lease_until_ms", JetJobQueueValue::Null),
            ("error_type", JetJobQueueValue::Text("jet.queue.lease".to_string())),
            ("error_reason", JetJobQueueValue::Text(reason.to_string())),
            ("error_detail", JetJobQueueValue::Null),
        ];
        jet_job_queue_update_event(
            store,
            &record,
            state,
            record.attempts,
            now,
            Some(reason),
            duration_ms,
            record.lease_owner.as_deref(),
            &extra,
        )?;
        observations.push(JetJobQueueDevtoolsTransition::from_record(
            &record,
            state,
            record.attempts,
            now,
            Some(reason),
            duration_ms,
            record.lease_owner.as_deref(),
        ));
    }
    Ok(observations)
}

impl<'a> JetJobQueue<'a> {
    pub fn claim(
        &mut self,
        worker: &str,
        limit: usize,
    ) -> Result<Vec<JetJobQueueClaim>, JetServiceError> {
        jet_job_queue_validate_name(worker, "queue worker")?;
        if limit == 0 || limit > self.policy.capacity {
            return Err(JetServiceError::Policy(
                "queue claim limit must be positive and within capacity".to_string(),
            ));
        }
        let authority = self.authority.clone();
        let name = self.name.clone();
        let policy = self.policy.clone();
        self.transaction(|_, store| {
            let now = jet_job_queue_now_ms();
            let rows = store
                .query(
                    "SELECT paused FROM jet_job_queue_meta WHERE authority = ? AND queue = ?",
                    &[
                        JetJobQueueValue::Text(authority.clone()),
                        JetJobQueueValue::Text(name.clone()),
                    ],
                )
                .map_err(|error| jet_job_queue_sql_error("pause lookup", error))?;
            let paused = rows
                .first()
                .map(|row| jet_job_queue_row_bool(row, "paused"))
                .transpose()?
                .unwrap_or(false);
            if paused {
                return Ok((Vec::new(), Vec::new()));
            }
            let mut observations = jet_job_queue_recover_expired_on_store(
                store,
                &authority,
                &name,
                &policy,
                now,
            )?;
            let candidates = store
                .query(
                    &format!(
                        "{JET_JOB_QUEUE_SELECT} WHERE authority = ? AND queue = ? AND state IN ('queued', 'retrying') AND due_at_ms <= ? ORDER BY sequence LIMIT ?"
                    ),
                    &[
                        JetJobQueueValue::Text(authority.clone()),
                        JetJobQueueValue::Text(name.clone()),
                        JetJobQueueValue::Int(now),
                        JetJobQueueValue::Int(limit as i64),
                    ],
                )
                .map_err(|error| jet_job_queue_sql_error("claim lookup", error))?;
            let mut claims = Vec::with_capacity(candidates.len());
            for row in &candidates {
                let record = jet_job_queue_stored_record(row)?;
                if record.attempts >= policy.max_attempts {
                    let extra = [
                        ("finished_at_ms", JetJobQueueValue::Int(now)),
                        ("due_at_ms", JetJobQueueValue::Int(now)),
                        ("retry_at_ms", JetJobQueueValue::Int(now)),
                        ("lease_owner", JetJobQueueValue::Null),
                        ("lease_token", JetJobQueueValue::Null),
                        ("lease_until_ms", JetJobQueueValue::Null),
                        ("error_type", JetJobQueueValue::Text("jet.queue.attempts".to_string())),
                        ("error_reason", JetJobQueueValue::Text("max_attempts".to_string())),
                        ("error_detail", JetJobQueueValue::Null),
                    ];
                    jet_job_queue_update_event(
                        store,
                        &record,
                        JetJobQueueState::DeadLettered,
                        record.attempts,
                        now,
                        Some("max_attempts"),
                        None,
                        None,
                        &extra,
                    )?;
                    observations.push(JetJobQueueDevtoolsTransition::from_record(
                        &record,
                        JetJobQueueState::DeadLettered,
                        record.attempts,
                        now,
                        Some("max_attempts"),
                        None,
                        None,
                    ));
                    continue;
                }
                let attempts = record.attempts.saturating_add(1);
                let lease_until_ms = now.saturating_add(policy.lease_ms);
                let lease_token = jet_job_queue_token(&record.id, worker)?;
                let extra = [
                    ("started_at_ms", JetJobQueueValue::Int(now)),
                    ("finished_at_ms", JetJobQueueValue::Null),
                    ("lease_owner", JetJobQueueValue::Text(worker.to_string())),
                    ("lease_token", JetJobQueueValue::Text(lease_token.clone())),
                    ("lease_until_ms", JetJobQueueValue::Int(lease_until_ms)),
                    ("retry_at_ms", JetJobQueueValue::Null),
                    ("result_type", JetJobQueueValue::Null),
                    ("result", JetJobQueueValue::Null),
                    ("result_public", JetJobQueueValue::Int(0)),
                    ("error_type", JetJobQueueValue::Null),
                    ("error_reason", JetJobQueueValue::Null),
                    ("error_detail", JetJobQueueValue::Null),
                ];
                jet_job_queue_update_event(
                    store,
                    &record,
                    JetJobQueueState::Running,
                    attempts,
                    now,
                    Some("claimed"),
                    None,
                    Some(worker),
                    &extra,
                )?;
                let mut claimed = record.clone();
                claimed.state = JetJobQueueState::Running;
                claimed.attempts = attempts;
                claimed.started_at_ms = Some(now);
                claimed.finished_at_ms = None;
                claimed.lease_owner = Some(worker.to_string());
                claimed.lease_token = Some(lease_token.clone());
                claimed.lease_until_ms = Some(lease_until_ms);
                claimed.retry_at_ms = None;
                claimed.result_type = None;
                claimed.result = None;
                claimed.result_public = false;
                claimed.error_type = None;
                claimed.error_reason = None;
                claimed.error_detail = None;
                claimed.updated_at_ms = now;
                claimed.event_sequence = claimed.event_sequence.saturating_add(1);
                claims.push(JetJobQueueClaim {
                    receipt: jet_job_queue_make_receipt(&claimed, policy.delivery, false),
                    payload: JetJobPayload {
                        type_id: claimed.payload_type.clone(),
                        bytes: claimed.payload.clone(),
                        publish: claimed.payload_public,
                    },
                    worker: worker.to_string(),
                    lease_token,
                    lease_until_ms,
                });
                observations.push(JetJobQueueDevtoolsTransition::from_record(
                    &record,
                    JetJobQueueState::Running,
                    attempts,
                    now,
                    Some("claimed"),
                    None,
                    Some(worker),
                ));
            }
            Ok((claims, observations))
        })
        .map(|(claims, observations)| {
            jet_job_queue_publish_transitions(observations);
            claims
        })
    }
}

fn jet_job_queue_find_stored(
    store: &mut dyn JetJobQueueStore,
    authority: &str,
    queue: &str,
    id: &str,
) -> Result<JetJobQueueStoredRecord, JetServiceError> {
    let rows = store
        .query(
            &format!(
                "{JET_JOB_QUEUE_SELECT} WHERE authority = ? AND queue = ? AND id = ? LIMIT 1"
            ),
            &[
                JetJobQueueValue::Text(authority.to_string()),
                JetJobQueueValue::Text(queue.to_string()),
                JetJobQueueValue::Text(id.to_string()),
            ],
        )
        .map_err(|error| jet_job_queue_sql_error("receipt lookup", error))?;
    rows.first()
        .ok_or_else(|| JetServiceError::Unknown(format!("queue job `{id}` is unknown")))
        .and_then(jet_job_queue_stored_record)
}

fn jet_job_queue_u64(value: i64, label: &str) -> Result<u64, JetServiceError> {
    u64::try_from(value).map_err(|_| {
        service_authority_error(format!("queue {label} is outside the supported range"))
    })
}

impl<'a> JetJobQueue<'a> {
    fn require_claim(
        record: &JetJobQueueStoredRecord,
        claim: &JetJobQueueClaim,
    ) -> Result<(), JetServiceError> {
        if record.id != claim.receipt.id
            || record.authority != claim.receipt.authority
            || record.queue != claim.receipt.queue
        {
            return Err(JetServiceError::Revoked(
                "queue lease belongs to another job authority".to_string(),
            ));
        }
        if record.state != JetJobQueueState::Running {
            return Err(JetServiceError::Stale(
                "queue job is no longer running under this lease".to_string(),
            ));
        }
        if record.lease_owner.as_deref() != Some(claim.worker.as_str())
            || record.lease_token.as_deref() != Some(claim.lease_token.as_str())
        {
            return Err(JetServiceError::Revoked(
                "queue lease token or worker does not match".to_string(),
            ));
        }
        Ok(())
    }

    pub fn heartbeat(
        &mut self,
        claim: &JetJobQueueClaim,
    ) -> Result<JetJobQueueReceipt, JetServiceError> {
        service_authority_validate_text(
            &claim.lease_token,
            "queue lease token",
            JET_JOB_QUEUE_MAX_KEY,
            false,
        )?;
        let authority = self.authority.clone();
        let name = self.name.clone();
        let policy = self.policy.clone();
        let (receipt, observation) = self.transaction(|_, store| {
            let record = jet_job_queue_find_stored(store, &authority, &name, &claim.receipt.id)?;
            if record.state == JetJobQueueState::Completed {
                return Ok((
                    jet_job_queue_make_receipt(&record, policy.delivery, false),
                    None,
                ));
            }
            Self::require_claim(&record, claim)?;
            let now = jet_job_queue_now_ms();
            let lease_until_ms = now.saturating_add(policy.lease_ms);
            let extra = [(
                "lease_until_ms",
                JetJobQueueValue::Int(lease_until_ms),
            )];
            jet_job_queue_update_event(
                store,
                &record,
                JetJobQueueState::Running,
                record.attempts,
                now,
                Some("heartbeat"),
                None,
                Some(&claim.worker),
                &extra,
            )?;
            let observation = JetJobQueueDevtoolsTransition::from_record(
                &record,
                JetJobQueueState::Running,
                record.attempts,
                now,
                Some("heartbeat"),
                None,
                Some(&claim.worker),
            )
            .with_event("heartbeat");
            let mut updated = record;
            updated.lease_until_ms = Some(lease_until_ms);
            updated.updated_at_ms = now;
            updated.event_sequence = updated.event_sequence.saturating_add(1);
            Ok((
                jet_job_queue_make_receipt(&updated, policy.delivery, false),
                Some(observation),
            ))
        })?;
        if let Some(observation) = observation {
            jet_job_queue_publish_transition(&observation);
        }
        Ok(receipt)
    }

    pub fn acknowledge(
        &mut self,
        claim: &JetJobQueueClaim,
        result: JetJobResult,
    ) -> Result<JetJobQueueReceipt, JetServiceError> {
        jet_job_queue_validate_result(&result)?;
        let authority = self.authority.clone();
        let name = self.name.clone();
        let policy = self.policy.clone();
        let (receipt, observation) = self.transaction(|_, store| {
            let record = jet_job_queue_find_stored(store, &authority, &name, &claim.receipt.id)?;
            if record.state == JetJobQueueState::Completed {
                return Ok((
                    jet_job_queue_make_receipt(&record, policy.delivery, false),
                    None,
                ));
            }
            Self::require_claim(&record, claim)?;
            let now = jet_job_queue_now_ms();
            let duration_ms = record
                .started_at_ms
                .map(|started| now.saturating_sub(started));
            let extra = [
                ("finished_at_ms", JetJobQueueValue::Int(now)),
                ("lease_owner", JetJobQueueValue::Null),
                ("lease_token", JetJobQueueValue::Null),
                ("lease_until_ms", JetJobQueueValue::Null),
                ("retry_at_ms", JetJobQueueValue::Null),
                ("result_type", JetJobQueueValue::Text(result.type_id.clone())),
                ("result", JetJobQueueValue::Blob(result.bytes.clone())),
                (
                    "result_public",
                    JetJobQueueValue::Int(if result.publish { 1 } else { 0 }),
                ),
                ("error_type", JetJobQueueValue::Null),
                ("error_reason", JetJobQueueValue::Null),
                ("error_detail", JetJobQueueValue::Null),
            ];
            jet_job_queue_update_event(
                store,
                &record,
                JetJobQueueState::Completed,
                record.attempts,
                now,
                Some("acknowledged"),
                duration_ms,
                Some(&claim.worker),
                &extra,
            )?;
            let observation = JetJobQueueDevtoolsTransition::from_record(
                &record,
                JetJobQueueState::Completed,
                record.attempts,
                now,
                Some("acknowledged"),
                duration_ms,
                Some(&claim.worker),
            );
            let mut updated = record;
            updated.state = JetJobQueueState::Completed;
            updated.finished_at_ms = Some(now);
            updated.lease_owner = None;
            updated.lease_token = None;
            updated.lease_until_ms = None;
            updated.retry_at_ms = None;
            updated.result_type = Some(result.type_id);
            updated.result = Some(result.bytes);
            updated.result_public = result.publish;
            updated.error_type = None;
            updated.error_reason = None;
            updated.error_detail = None;
            updated.updated_at_ms = now;
            updated.event_sequence = updated.event_sequence.saturating_add(1);
            Ok((
                jet_job_queue_make_receipt(&updated, policy.delivery, false),
                Some(observation),
            ))
        })?;
        if let Some(observation) = observation {
            jet_job_queue_publish_transition(&observation);
        }
        Ok(receipt)
    }

    pub fn fail(
        &mut self,
        claim: &JetJobQueueClaim,
        error: JetJobError,
    ) -> Result<JetJobQueueReceipt, JetServiceError> {
        jet_job_queue_validate_error(&error)?;
        let authority = self.authority.clone();
        let name = self.name.clone();
        let policy = self.policy.clone();
        let (receipt, observation) = self.transaction(|_, store| {
            let record = jet_job_queue_find_stored(store, &authority, &name, &claim.receipt.id)?;
            if record.state == JetJobQueueState::DeadLettered
                || record.state == JetJobQueueState::Failed
            {
                return Ok((
                    jet_job_queue_make_receipt(&record, policy.delivery, false),
                    None,
                ));
            }
            Self::require_claim(&record, claim)?;
            let now = jet_job_queue_now_ms();
            let terminal = record.attempts >= policy.max_attempts;
            let state = if terminal {
                JetJobQueueState::DeadLettered
            } else {
                JetJobQueueState::Retrying
            };
            let due_at_ms = if terminal {
                record.due_at_ms
            } else {
                now.saturating_add(jet_job_queue_backoff(&policy, record.attempts))
            };
            let duration_ms = record
                .started_at_ms
                .map(|started| now.saturating_sub(started));
            let extra = [
                ("finished_at_ms", JetJobQueueValue::Int(now)),
                ("due_at_ms", JetJobQueueValue::Int(due_at_ms)),
                (
                    "retry_at_ms",
                    if terminal {
                        JetJobQueueValue::Null
                    } else {
                        JetJobQueueValue::Int(due_at_ms)
                    },
                ),
                ("lease_owner", JetJobQueueValue::Null),
                ("lease_token", JetJobQueueValue::Null),
                ("lease_until_ms", JetJobQueueValue::Null),
                ("result_type", JetJobQueueValue::Null),
                ("result", JetJobQueueValue::Null),
                ("result_public", JetJobQueueValue::Int(0)),
                ("error_type", JetJobQueueValue::Text(error.type_id.clone())),
                (
                    "error_reason",
                    JetJobQueueValue::Text(error.reason.clone()),
                ),
                (
                    "error_detail",
                    error
                        .detail
                        .clone()
                        .map_or(JetJobQueueValue::Null, JetJobQueueValue::Text),
                ),
            ];
            jet_job_queue_update_event(
                store,
                &record,
                state,
                record.attempts,
                now,
                Some(&error.reason),
                duration_ms,
                Some(&claim.worker),
                &extra,
            )?;
            let observation = JetJobQueueDevtoolsTransition::from_record(
                &record,
                state,
                record.attempts,
                now,
                Some(&error.reason),
                duration_ms,
                Some(&claim.worker),
            );
            let mut updated = record;
            updated.state = state;
            updated.finished_at_ms = Some(now);
            updated.due_at_ms = due_at_ms;
            updated.retry_at_ms = (!terminal).then_some(due_at_ms);
            updated.lease_owner = None;
            updated.lease_token = None;
            updated.lease_until_ms = None;
            updated.result_type = None;
            updated.result = None;
            updated.result_public = false;
            updated.error_type = Some(error.type_id);
            updated.error_reason = Some(error.reason);
            updated.error_detail = error.detail;
            updated.updated_at_ms = now;
            updated.event_sequence = updated.event_sequence.saturating_add(1);
            Ok((
                jet_job_queue_make_receipt(&updated, policy.delivery, false),
                Some(observation),
            ))
        })?;
        if let Some(observation) = observation {
            jet_job_queue_publish_transition(&observation);
        }
        Ok(receipt)
    }

    pub fn cancel(
        &mut self,
        id: &str,
        reason: impl Into<String>,
        lease_token: Option<&str>,
    ) -> Result<JetJobQueueReceipt, JetServiceError> {
        service_authority_validate_text(id, "queue job id", JET_JOB_QUEUE_MAX_KEY, false)?;
        let reason = reason.into();
        service_authority_validate_text(&reason, "queue cancellation reason", JET_JOB_QUEUE_MAX_REASON, false)?;
        let authority = self.authority.clone();
        let name = self.name.clone();
        let policy = self.policy.clone();
        let (receipt, observation) = self.transaction(|_, store| {
            let record = jet_job_queue_find_stored(store, &authority, &name, id)?;
            if record.state == JetJobQueueState::Cancelled
                || record.state == JetJobQueueState::Completed
                || record.state == JetJobQueueState::DeadLettered
            {
                return Ok((
                    jet_job_queue_make_receipt(&record, policy.delivery, false),
                    None,
                ));
            }
            if record.state == JetJobQueueState::Running
                && record.lease_token.as_deref() != lease_token
            {
                return Err(JetServiceError::Revoked(
                    "running queue cancellation requires its active lease token".to_string(),
                ));
            }
            let now = jet_job_queue_now_ms();
            let duration_ms = record
                .started_at_ms
                .map(|started| now.saturating_sub(started));
            let extra = [
                ("finished_at_ms", JetJobQueueValue::Int(now)),
                ("lease_owner", JetJobQueueValue::Null),
                ("lease_token", JetJobQueueValue::Null),
                ("lease_until_ms", JetJobQueueValue::Null),
                ("retry_at_ms", JetJobQueueValue::Null),
                ("error_type", JetJobQueueValue::Text("jet.queue.cancel".to_string())),
                ("error_reason", JetJobQueueValue::Text(reason.clone())),
                ("error_detail", JetJobQueueValue::Null),
            ];
            jet_job_queue_update_event(
                store,
                &record,
                JetJobQueueState::Cancelled,
                record.attempts,
                now,
                Some(&reason),
                duration_ms,
                record.lease_owner.as_deref(),
                &extra,
            )?;
            let observation = JetJobQueueDevtoolsTransition::from_record(
                &record,
                JetJobQueueState::Cancelled,
                record.attempts,
                now,
                Some(&reason),
                duration_ms,
                record.lease_owner.as_deref(),
            );
            let mut updated = record;
            updated.state = JetJobQueueState::Cancelled;
            updated.finished_at_ms = Some(now);
            updated.lease_owner = None;
            updated.lease_token = None;
            updated.lease_until_ms = None;
            updated.retry_at_ms = None;
            updated.error_type = Some("jet.queue.cancel".to_string());
            updated.error_reason = Some(reason);
            updated.error_detail = None;
            updated.updated_at_ms = now;
            updated.event_sequence = updated.event_sequence.saturating_add(1);
            Ok((
                jet_job_queue_make_receipt(&updated, policy.delivery, false),
                Some(observation),
            ))
        })?;
        if let Some(observation) = observation {
            jet_job_queue_publish_transition(&observation);
        }
        Ok(receipt)
    }

    pub fn dead_letter(
        &mut self,
        id: &str,
        reason: impl Into<String>,
        lease_token: Option<&str>,
    ) -> Result<JetJobQueueReceipt, JetServiceError> {
        service_authority_validate_text(id, "queue job id", JET_JOB_QUEUE_MAX_KEY, false)?;
        let reason = reason.into();
        service_authority_validate_text(&reason, "queue dead-letter reason", JET_JOB_QUEUE_MAX_REASON, false)?;
        let authority = self.authority.clone();
        let name = self.name.clone();
        let policy = self.policy.clone();
        let (receipt, observation) = self.transaction(|_, store| {
            let record = jet_job_queue_find_stored(store, &authority, &name, id)?;
            if record.state == JetJobQueueState::DeadLettered {
                return Ok((
                    jet_job_queue_make_receipt(&record, policy.delivery, false),
                    None,
                ));
            }
            if record.state == JetJobQueueState::Completed
                || record.state == JetJobQueueState::Cancelled
            {
                return Err(JetServiceError::Policy(
                    "completed or cancelled queue work cannot be dead-lettered".to_string(),
                ));
            }
            if record.state == JetJobQueueState::Running
                && record.lease_token.as_deref() != lease_token
            {
                return Err(JetServiceError::Revoked(
                    "running queue dead-lettering requires its active lease token".to_string(),
                ));
            }
            let now = jet_job_queue_now_ms();
            let duration_ms = record
                .started_at_ms
                .map(|started| now.saturating_sub(started));
            let extra = [
                ("finished_at_ms", JetJobQueueValue::Int(now)),
                ("lease_owner", JetJobQueueValue::Null),
                ("lease_token", JetJobQueueValue::Null),
                ("lease_until_ms", JetJobQueueValue::Null),
                ("retry_at_ms", JetJobQueueValue::Null),
                ("error_type", JetJobQueueValue::Text("jet.queue.dead_letter".to_string())),
                ("error_reason", JetJobQueueValue::Text(reason.clone())),
                ("error_detail", JetJobQueueValue::Null),
            ];
            jet_job_queue_update_event(
                store,
                &record,
                JetJobQueueState::DeadLettered,
                record.attempts,
                now,
                Some(&reason),
                duration_ms,
                record.lease_owner.as_deref(),
                &extra,
            )?;
            let observation = JetJobQueueDevtoolsTransition::from_record(
                &record,
                JetJobQueueState::DeadLettered,
                record.attempts,
                now,
                Some(&reason),
                duration_ms,
                record.lease_owner.as_deref(),
            );
            let mut updated = record;
            updated.state = JetJobQueueState::DeadLettered;
            updated.finished_at_ms = Some(now);
            updated.lease_owner = None;
            updated.lease_token = None;
            updated.lease_until_ms = None;
            updated.retry_at_ms = None;
            updated.error_type = Some("jet.queue.dead_letter".to_string());
            updated.error_reason = Some(reason);
            updated.error_detail = None;
            updated.updated_at_ms = now;
            updated.event_sequence = updated.event_sequence.saturating_add(1);
            Ok((
                jet_job_queue_make_receipt(&updated, policy.delivery, false),
                Some(observation),
            ))
        })?;
        if let Some(observation) = observation {
            jet_job_queue_publish_transition(&observation);
        }
        Ok(receipt)
    }
}

impl<'a> JetJobQueue<'a> {
    pub fn recover_expired(&mut self) -> Result<(), JetServiceError> {
        let authority = self.authority.clone();
        let name = self.name.clone();
        let policy = self.policy.clone();
        let observations = self.transaction(|_, store| {
            jet_job_queue_recover_expired_on_store(
                store,
                &authority,
                &name,
                &policy,
                jet_job_queue_now_ms(),
            )
        })?;
        jet_job_queue_publish_transitions(observations);
        Ok(())
    }
}

impl<'a> JetJobQueue<'a> {
    pub fn status(&mut self) -> Result<JetJobQueueStatus, JetServiceError> {
        self.validate_endpoint()?;
        let queue_params = self.queue_params();
        let rows = self
            .store
            .query(
                "SELECT COALESCE(SUM(CASE WHEN state = 'queued' THEN 1 ELSE 0 END), 0) AS queued, COALESCE(SUM(CASE WHEN state = 'running' THEN 1 ELSE 0 END), 0) AS running, COALESCE(SUM(CASE WHEN state = 'retrying' THEN 1 ELSE 0 END), 0) AS retrying, COALESCE(SUM(CASE WHEN state = 'completed' THEN 1 ELSE 0 END), 0) AS completed, COALESCE(SUM(CASE WHEN state = 'failed' THEN 1 ELSE 0 END), 0) AS failed, COALESCE(SUM(CASE WHEN state = 'dead_lettered' THEN 1 ELSE 0 END), 0) AS dead_lettered, COALESCE(SUM(CASE WHEN state = 'cancelled' THEN 1 ELSE 0 END), 0) AS cancelled, MIN(CASE WHEN state IN ('queued', 'retrying') THEN accepted_at_ms END) AS oldest_accepted, MAX(updated_at_ms) AS latest_update FROM jet_job_queue_jobs WHERE authority = ? AND queue = ?",
                &queue_params,
            )
            .map_err(|error| jet_job_queue_sql_error("status", error))?;
        let row = rows
            .first()
            .ok_or_else(|| service_authority_error("queue status returned no aggregate row"))?;
        let queued = jet_job_queue_u64(jet_job_queue_row_i64(row, "queued")?, "queued")?;
        let running = jet_job_queue_u64(jet_job_queue_row_i64(row, "running")?, "running")?;
        let retrying = jet_job_queue_u64(jet_job_queue_row_i64(row, "retrying")?, "retrying")?;
        let completed = jet_job_queue_u64(jet_job_queue_row_i64(row, "completed")?, "completed")?;
        let failed = jet_job_queue_u64(jet_job_queue_row_i64(row, "failed")?, "failed")?;
        let dead_lettered =
            jet_job_queue_u64(jet_job_queue_row_i64(row, "dead_lettered")?, "dead-lettered")?;
        let cancelled =
            jet_job_queue_u64(jet_job_queue_row_i64(row, "cancelled")?, "cancelled")?;
        let now = jet_job_queue_now_ms();
        let wait_ms = jet_job_queue_row_optional_i64(row, "oldest_accepted")?
            .map(|accepted| now.saturating_sub(accepted).max(0) as u64)
            .unwrap_or(0);
        let freshness_ms = jet_job_queue_row_optional_i64(row, "latest_update")?
            .map(|updated| now.saturating_sub(updated).max(0) as u64)
            .unwrap_or(0);
        let meta = self
            .store
            .query(
                "SELECT capacity, paused FROM jet_job_queue_meta WHERE authority = ? AND queue = ? LIMIT 1",
                &queue_params,
            )
            .map_err(|error| jet_job_queue_sql_error("metadata status", error))?;
        let meta = meta
            .first()
            .ok_or_else(|| service_authority_error("queue metadata is missing"))?;
        let capacity = jet_job_queue_usize(jet_job_queue_row_i64(meta, "capacity")?, "capacity")?;
        let paused = jet_job_queue_row_bool(meta, "paused")?;
        let throughput_rows = self
            .store
            .query(
                "SELECT COUNT(*) AS throughput FROM jet_job_queue_events WHERE authority = ? AND queue = ? AND state = 'completed' AND timestamp_ms >= ?",
                &[
                    JetJobQueueValue::Text(self.authority.clone()),
                    JetJobQueueValue::Text(self.name.clone()),
                    JetJobQueueValue::Int(now.saturating_sub(60_000)),
                ],
            )
            .map_err(|error| jet_job_queue_sql_error("throughput status", error))?;
        let throughput = throughput_rows
            .first()
            .map(|row| jet_job_queue_u64(jet_job_queue_row_i64(row, "throughput")?, "throughput"))
            .transpose()?
            .unwrap_or(0);
        let status = JetJobQueueStatus {
            queue: self.name.clone(),
            authority: self.authority.clone(),
            queued,
            running,
            retrying,
            completed,
            failed,
            dead_lettered,
            cancelled,
            depth: queued.saturating_add(retrying),
            wait_ms,
            throughput,
            capacity,
            paused,
            freshness_ms,
        };
        jet_job_queue_publish_status(&status);
        Ok(status)
    }

    fn set_paused(&mut self, paused: bool) -> Result<JetJobQueueStatus, JetServiceError> {
        let now = jet_job_queue_now_ms();
        let authority = self.authority.clone();
        let name = self.name.clone();
        self.transaction(|_, store| {
            let changed = store
                .execute(
                    "UPDATE jet_job_queue_meta SET paused = ?, updated_at_ms = ? WHERE authority = ? AND queue = ?",
                    &[
                        JetJobQueueValue::Int(if paused { 1 } else { 0 }),
                        JetJobQueueValue::Int(now),
                        JetJobQueueValue::Text(authority),
                        JetJobQueueValue::Text(name),
                    ],
                )
                .map_err(|error| jet_job_queue_sql_error("pause", error))?;
            if changed != 1 {
                return Err(service_authority_error("queue metadata is missing"));
            }
            Ok(())
        })?;
        self.status()
    }

    pub fn pause(&mut self) -> Result<JetJobQueueStatus, JetServiceError> {
        self.set_paused(true)
    }

    pub fn resume(&mut self) -> Result<JetJobQueueStatus, JetServiceError> {
        self.set_paused(false)
    }

    /// Wait only on durable metadata.  No payload is read or published while
    /// waiting, and the timeout is an explicit caller decision.
    pub fn wait(&mut self, timeout_ms: i64) -> Result<JetJobQueueStatus, JetServiceError> {
        if timeout_ms < 0 || timeout_ms > JET_JOB_QUEUE_MAX_DELAY_MS {
            return Err(JetServiceError::Policy(
                "queue wait timeout is outside the supported range".to_string(),
            ));
        }
        let deadline = jet_job_queue_now_ms().saturating_add(timeout_ms);
        loop {
            let status = self.status()?;
            if status.depth > 0 || jet_job_queue_now_ms() >= deadline {
                return Ok(status);
            }
            let remaining = deadline.saturating_sub(jet_job_queue_now_ms());
            std::thread::sleep(std::time::Duration::from_millis(
                remaining.clamp(1, 25) as u64,
            ));
        }
    }

    /// Retention is explicit in the queue policy.  Pruning only removes
    /// terminal records older than that policy; active work is never deleted.
    pub fn prune(&mut self) -> Result<u64, JetServiceError> {
        let cutoff = jet_job_queue_now_ms().saturating_sub(self.policy.retention_ms);
        let authority = self.authority.clone();
        let name = self.name.clone();
        self.transaction(|_, store| {
            let events = store
                .execute(
                    "DELETE FROM jet_job_queue_events WHERE authority = ? AND queue = ? AND job_id IN (SELECT id FROM jet_job_queue_jobs WHERE authority = ? AND queue = ? AND state IN ('completed', 'failed', 'dead_lettered', 'cancelled') AND finished_at_ms IS NOT NULL AND finished_at_ms <= ?)",
                    &[
                        JetJobQueueValue::Text(authority.clone()),
                        JetJobQueueValue::Text(name.clone()),
                        JetJobQueueValue::Text(authority.clone()),
                        JetJobQueueValue::Text(name.clone()),
                        JetJobQueueValue::Int(cutoff),
                    ],
                )
                .map_err(|error| jet_job_queue_sql_error("event retention", error))?;
            let jobs = store
                .execute(
                    "DELETE FROM jet_job_queue_jobs WHERE authority = ? AND queue = ? AND state IN ('completed', 'failed', 'dead_lettered', 'cancelled') AND finished_at_ms IS NOT NULL AND finished_at_ms <= ?",
                    &[
                        JetJobQueueValue::Text(authority),
                        JetJobQueueValue::Text(name),
                        JetJobQueueValue::Int(cutoff),
                    ],
                )

                .map_err(|error| jet_job_queue_sql_error("job retention", error))?;
            Ok(jet_job_queue_u64(events.saturating_add(jobs), "pruned records")?)
        })
    }
}

pub fn jet_job_queue_receipt(
    queue: &mut JetJobQueue<'static>,
    id: &str,
) -> Result<JetJobQueueReceipt, JetServiceError> {
    queue.receipt(id)
}

pub fn jet_job_queue_inspect(
    queue: &mut JetJobQueue<'static>,
    limit: i64,
    include_payload: bool,
) -> Result<Vec<JetJobQueueRecord>, JetServiceError> {
    queue.inspect(jet_job_queue_usize(limit, "inspection limit")?, include_payload)
}

pub fn jet_job_queue_events(
    queue: &mut JetJobQueue<'static>,
    id: &str,
) -> Result<Vec<JetJobQueueEvent>, JetServiceError> {
    queue.events(id)
}

pub fn jet_job_queue_claim(
    queue: &mut JetJobQueue<'static>,
    worker: &str,
    limit: i64,
) -> Result<Vec<JetJobQueueClaim>, JetServiceError> {
    queue.claim(worker, jet_job_queue_usize(limit, "claim limit")?)
}

pub fn jet_job_queue_heartbeat(
    queue: &mut JetJobQueue<'static>,
    claim: &JetJobQueueClaim,
) -> Result<JetJobQueueReceipt, JetServiceError> {
    queue.heartbeat(claim)
}

pub fn jet_job_queue_acknowledge(
    queue: &mut JetJobQueue<'static>,
    claim: &JetJobQueueClaim,
    result: JetJobResult,
) -> Result<JetJobQueueReceipt, JetServiceError> {
    queue.acknowledge(claim, result)
}

pub fn jet_job_queue_fail(
    queue: &mut JetJobQueue<'static>,
    claim: &JetJobQueueClaim,
    error: JetJobError,
) -> Result<JetJobQueueReceipt, JetServiceError> {
    queue.fail(claim, error)
}
const JET_JOB_SERVICE_CLAIM_LIMIT: usize = 16;

/// Run one bounded queue tick under the endpoint that issued the worker.
/// Engines provide only checked identity lookup and typed invocation; claim,
/// lease, validation, and settlement stay here.
pub fn jet_job_service_queue_tick_dispatch<F>(
    endpoint: &JetServiceEndpoint,
    limit: usize,
    mut dispatch: F,
) -> Result<usize, JetServiceError>
where
    F: FnMut(&str, &JetJobPayload) -> Result<JetJobResult, JetJobError>,
{
    if limit == 0 {
        return Ok(0);
    }
    let mut queue = JetJobQueue::open_default(
        endpoint,
        "default".to_string(),
        JetJobQueuePolicy::default(),
    )?;
    let claims = queue.claim(
        &endpoint.worker,
        limit.min(JET_JOB_SERVICE_CLAIM_LIMIT),
    )?;
    let mut settled = 0usize;
    for claim in claims {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            dispatch(claim.receipt.job_type.as_str(), &claim.payload)
        }))
        .map_err(|_| JetJobError {
            type_id: claim.payload.type_id.clone(),
            reason: "panic".to_string(),
            detail: Some(format!(
                "checked #Job `{}` dispatcher panicked",
                claim.receipt.job_type
            )),
        })
        .and_then(|result| result);
        match outcome {
            Ok(result) => {
                let result = JetJobResult::new(result.type_id, result.bytes, result.publish)
                    .map_err(|error| JetJobError {
                        type_id: claim.payload.type_id.clone(),
                        reason: "invalid_result".to_string(),
                        detail: Some(error.jet_show()),
                    });
                match result {
                    Ok(result) => {
                        queue.acknowledge(&claim, result)?;
                    }
                    Err(error) => {
                        queue.fail(&claim, error)?;
                    }
                }
            }
            Err(error) => {
                queue.fail(&claim, error)?;
            }
        }
        settled += 1;
    }
    Ok(settled)
}

pub fn jet_job_queue_cancel(
    queue: &mut JetJobQueue<'static>,
    id: &str,
    reason: &str,
    lease_token: &Option<String>,
) -> Result<JetJobQueueReceipt, JetServiceError> {
    queue.cancel(id, reason, lease_token.as_deref())
}

pub fn jet_job_queue_dead_letter(
    queue: &mut JetJobQueue<'static>,
    id: &str,
    reason: &str,
    lease_token: &Option<String>,
) -> Result<JetJobQueueReceipt, JetServiceError> {
    queue.dead_letter(id, reason, lease_token.as_deref())
}

pub fn jet_job_queue_recover_expired(
    queue: &mut JetJobQueue<'static>,
) -> Result<(), JetServiceError> {
    queue.recover_expired()
}

pub fn jet_job_queue_status(
    queue: &mut JetJobQueue<'static>,
) -> Result<JetJobQueueStatus, JetServiceError> {
    queue.status()
}

pub fn jet_job_queue_pause(
    queue: &mut JetJobQueue<'static>,
) -> Result<JetJobQueueStatus, JetServiceError> {
    queue.pause()
}

pub fn jet_job_queue_resume(
    queue: &mut JetJobQueue<'static>,
) -> Result<JetJobQueueStatus, JetServiceError> {
    queue.resume()
}

pub fn jet_job_queue_wait(
    queue: &mut JetJobQueue<'static>,
    duration: Duration,
) -> Result<JetJobQueueStatus, JetServiceError> {
    let milliseconds = i64::try_from(duration.as_millis())
        .map_err(|_| service_authority_error("queue wait duration is outside the supported range"))?;
    queue.wait(milliseconds)
}

pub fn jet_job_queue_prune(queue: &mut JetJobQueue<'static>) -> Result<i64, JetServiceError> {
    queue.prune().and_then(|count| {
        i64::try_from(count)
            .map_err(|_| service_authority_error("queue pruned count is outside the supported range"))
    })
}
