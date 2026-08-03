// D-SERVICE-AUTHORITY1: one durable authority for AOT and ambient execution.
// The log is append-only, length/hex framed, and fsync'd after every commit.
// A process-wide lock closes the read/append race between runtime instances.

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const SERVICE_AUTH_MAX_STORE: usize = 4096;
const SERVICE_AUTH_MAX_KEY: usize = 1024;
const SERVICE_AUTH_MAX_MESSAGE: usize = 1024 * 1024;
const SERVICE_AUTH_MAX_RECORDS: usize = 100_000;
const SERVICE_AUTH_MAX_BYTES: u64 = 128 * 1024 * 1024;
const SERVICE_AUTH_MAX_PENDING: usize = 100_000;

#[derive(Clone, Debug)]
pub struct JetServiceRuntime {
    pub store: String,
    pub retention_ms: i64,
}

#[derive(Clone, Debug)]
pub enum JetServiceReceipt {
    Accepted(String),
    Duplicate(String),
    Retained { id: String, until: i64 },
    DeadLettered(String),
    Rejected(String),
    Unavailable(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetServiceEndpoint {
    pub tree: String,
    pub worker: String,
    pub generation: i64,
    /// Opaque authority capability. It is carried by the endpoint value but
    /// never exposed as a user-selectable field.
    pub authority: String,
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
    expires: i64,
    retained_until: Option<i64>,
    delivered_to_worker: bool,
    delivered: bool,
    dead: bool,
}

#[derive(Clone, Debug)]
struct ServiceAuthorityEndpointState {
    tree: String,
    worker: String,
    generation: i64,
    started: bool,
    store: Option<String>,
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

fn service_authority_validate_text(
    value: &str,
    label: &str,
    max: usize,
    allow_empty: bool,
) -> Result<(), JetServiceError> {
    if (!allow_empty && value.is_empty()) || value.len() > max || value.chars().any(char::is_control)
    {
        return Err(JetServiceError::Policy(format!(
            "{label} is empty, too long, or contains control characters"
        )));
    }
    Ok(())
}

fn service_authority_validate_runtime(runtime: &JetServiceRuntime) -> Result<(), JetServiceError> {
    service_authority_validate_text(&runtime.store, "service store", SERVICE_AUTH_MAX_STORE, false)?;
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
    !entry.dead
        && match entry.retained_until {
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
    service_authority_validate_text(&endpoint.authority, "service authority", 256, false)?;
    if endpoint.generation < 1 {
        return Err(JetServiceError::Policy(
            "service endpoint generation must be positive".to_string(),
        ));
    }
    Ok(())
}

fn service_endpoint_key(authority: &str, worker: &str) -> String {
    format!("{authority}\u{1f}{worker}")
}

fn service_pending_key(store: &str, endpoint: &JetServiceEndpoint) -> String {
    format!(
        "{}\u{1e}{}\u{1d}{}",
        service_authority_store_identity(store),
        service_endpoint_key(&endpoint.authority, &endpoint.worker),
        endpoint.generation
    )
}

pub fn jet_services_authority_new_tree(name: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in name.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("sa-{hash:016x}")
}

pub fn jet_services_authority_endpoint(
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
    };
    service_authority_validate_endpoint(&endpoint)?;
    Ok(endpoint)
}

pub fn jet_services_authority_register(
    endpoint: &JetServiceEndpoint,
    started: bool,
) -> Result<(), JetServiceError> {
    service_authority_validate_endpoint(endpoint)?;
    let key = service_endpoint_key(&endpoint.authority, &endpoint.worker);
    let mut registry = service_endpoint_registry()
        .lock()
        .map_err(|_| service_authority_error("service endpoint registry lock is poisoned"))?;
    if registry.contains_key(&key) {
        return Err(JetServiceError::Policy(
            "service endpoint authority is already registered".to_string(),
        ));
    }
    registry.insert(
        key,
        ServiceAuthorityEndpointState {
            tree: endpoint.tree.clone(),
            worker: endpoint.worker.clone(),
            generation: endpoint.generation,
            started,
            store: None,
        },
    );
    Ok(())
}

pub fn jet_services_authority_update(
    endpoint: &JetServiceEndpoint,
    started: bool,
) -> Result<(), JetServiceError> {
    service_authority_validate_endpoint(endpoint)?;
    let key = service_endpoint_key(&endpoint.authority, &endpoint.worker);
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
    state.generation = endpoint.generation;
    state.started = started;
    Ok(())
}

fn service_authority_current_endpoint(
    authority: &str,
    tree: &str,
    worker: &str,
) -> Result<JetServiceEndpoint, JetServiceError> {
    let key = service_endpoint_key(authority, worker);
    let registry = service_endpoint_registry()
        .lock()
        .map_err(|_| service_authority_error("service endpoint registry lock is poisoned"))?;
    let state = registry.get(&key).ok_or_else(|| {
        JetServiceError::Partitioned("service endpoint authority is not registered".to_string())
    })?;
    if state.tree != tree {
        return Err(JetServiceError::Revoked(format!(
            "service endpoint authority belongs to tree `{}`",
            state.tree
        )));
    }
    if !state.started {
        return Err(JetServiceError::NotStarted(format!(
            "service worker `{worker}` is not running"
        )));
    }
    jet_services_authority_endpoint(
        state.tree.clone(),
        state.worker.clone(),
        state.generation,
        authority.to_string(),
    )
}

pub fn jet_services_authority_validate(
    endpoint: &JetServiceEndpoint,
) -> Result<(), JetServiceError> {
    service_authority_validate_endpoint(endpoint)?;
    let key = service_endpoint_key(&endpoint.authority, &endpoint.worker);
    let registry = service_endpoint_registry()
        .lock()
        .map_err(|_| service_authority_error("service endpoint registry lock is poisoned"))?;
    let state = registry.get(&key).ok_or_else(|| {
        JetServiceError::Partitioned("service endpoint authority is not registered".to_string())
    })?;
    if state.tree != endpoint.tree {
        return Err(JetServiceError::Revoked(
            "service endpoint belongs to another tree".to_string(),
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
    Ok(())
}

fn service_authority_bound_store(
    endpoint: &JetServiceEndpoint,
) -> Result<Option<String>, JetServiceError> {
    jet_services_authority_validate(endpoint)?;
    let key = service_endpoint_key(&endpoint.authority, &endpoint.worker);
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
    Ok(state.store.clone())
}

fn service_authority_bind_store(
    runtime: &JetServiceRuntime,
    endpoint: &JetServiceEndpoint,
) -> Result<String, JetServiceError> {
    service_authority_validate_runtime(runtime)?;
    jet_services_authority_validate(endpoint)?;
    let store = service_authority_store_identity(&runtime.store);
    let key = service_endpoint_key(&endpoint.authority, &endpoint.worker);
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
    if let Some(bound) = &state.store {
        if bound != &store {
            return Err(JetServiceError::Revoked(
                "service endpoint authority is bound to another store".to_string(),
            ));
        }
    } else {
        state.store = Some(store.clone());
    }
    Ok(store)
}

fn service_authority_remove_pending(
    store: &str,
    authority: &str,
    worker: &str,
    generation: i64,
    id: &str,
) -> Result<(), JetServiceError> {
    let key = format!(
        "{}\u{1e}{}\u{1d}{}",
        service_authority_store_identity(store),
        service_endpoint_key(authority, worker),
        generation
    );
    let mut pending = service_pending_registry()
        .lock()
        .map_err(|_| service_authority_error("service pending registry lock is poisoned"))?;
    if let Some(queue) = pending.get_mut(&key) {
        let store_identity = service_authority_store_identity(store);
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
    service_authority_remove_pending(
        &runtime.store,
        &entry.authority,
        &entry.worker,
        entry.generation,
        &entry.id,
    )
}

fn service_authority_cleanup_expired(
    runtime: &JetServiceRuntime,
    entries: &[ServiceAuthorityEntry],
    now: i64,
) -> Result<bool, JetServiceError> {
    let mut cleaned = false;
    for entry in entries {
        if service_authority_entry_expired(entry, now) {
            service_authority_append(
                runtime,
                'D',
                &[entry.id.clone(), "retention expired".to_string()],
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
    if queue.iter().any(|(queued_id, _, queued_store)| {
        queued_id == id && queued_store == &store
    }) {
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
    let mut entries = service_authority_entries(&records)?;
    if service_authority_cleanup_expired(&runtime, &entries, service_authority_now())? {
        entries = service_authority_entries(&service_authority_read(&runtime)?)?;
    }
    let key = service_pending_key(&store, endpoint);
    let mut pending = service_pending_registry()
        .lock()
        .map_err(|_| service_authority_error("service pending registry lock is poisoned"))?;
    let queue = pending.entry(key).or_default();
    let store_identity = service_authority_store_identity(&store);
    for entry in &entries {
        if entry.authority != endpoint.authority
            || entry.tree != endpoint.tree
            || entry.worker != endpoint.worker
            || entry.generation != endpoint.generation
            || entry.dead
            || entry.delivered_to_worker
            || entry.retained_until.is_some()
            || service_authority_entry_expired(entry, service_authority_now())
        {
            continue;
        }
        if queue.iter().any(|(queued_id, _, queued_store)| {
            queued_id == &entry.id
                && service_authority_store_identity(queued_store) == store_identity
        }) {
            continue;
        }
        if queue.len() >= SERVICE_AUTH_MAX_PENDING {
            return Err(JetServiceError::Full(
                "service authority pending delivery queue is full".to_string(),
            ));
        }
        queue.push((entry.id.clone(), entry.message.clone(), store.clone()));
    }
    let count = queue.len().min(capacity as usize);
    Ok(queue.drain(..count).collect())
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
    let mut authority_entries = service_authority_entries(&service_authority_read(&runtime)?)?;
    if service_authority_cleanup_expired(
        &runtime,
        &authority_entries,
        service_authority_now(),
    )? {
        authority_entries = service_authority_entries(&service_authority_read(&runtime)?)?;
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
        if authority_entry.authority != endpoint.authority
            || authority_entry.tree != endpoint.tree
            || authority_entry.worker != endpoint.worker
            || authority_entry.generation != endpoint.generation
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
pub fn jet_services_authority_mark_delivered(
    store: &str,
    id: &str,
) -> Result<(), JetServiceError> {
    service_authority_validate_text(id, "service id", SERVICE_AUTH_MAX_KEY, false)?;
    let runtime = JetServiceRuntime {
        store: store.to_string(),
        retention_ms: 0,
    };
    service_authority_validate_runtime(&runtime)?;
    let _guard = service_authority_lock()
        .lock()
        .map_err(|_| service_authority_error("service authority lock is poisoned"))?;
    let entries = service_authority_entries(&service_authority_read(&runtime)?)?;
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
    if !entry.delivered_to_worker {
        // Remove the in-memory reservation before the durable marker. If the
        // marker append fails, the log remains the recovery source and the
        // caller can put the whole suffix back without losing this receipt.
        service_authority_remove_pending_entry(&runtime, entry)?;
        service_authority_append(&runtime, 'V', &[id.to_string()])?;
    } else {
        service_authority_remove_pending_entry(&runtime, entry)?;
    }
    Ok(())
}

fn service_authority_enqueue_entry(
    runtime: &JetServiceRuntime,
    entry: &ServiceAuthorityEntry,
) -> Result<(), JetServiceError> {
    let endpoint =
        service_authority_current_endpoint(&entry.authority, &entry.tree, &entry.worker)?;
    if endpoint.generation != entry.generation {
        return Err(JetServiceError::Stale(format!(
            "service receipt generation {} is no longer current (current generation {})",
            entry.generation, endpoint.generation
        )));
    }
    jet_services_authority_enqueue(runtime, &endpoint, &entry.id, &entry.message)
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
    format!("{}:{}", value.len(), service_authority_hex(value.as_bytes()))
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
        .map_err(|error| service_authority_error(format!("could not inspect service store: {error}")))?
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
    // Ignore only that incomplete tail; a malformed complete record still fails
    // closed instead of being silently repaired.
    let complete = contents
        .rfind('\n')
        .map(|end| &contents[..end])
        .unwrap_or("");
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
    let record = service_authority_record(op, fields);
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&runtime.store)
        .map_err(|error| service_authority_error(format!("could not open service store: {error}")))?;
    let current_size = file
        .metadata()
        .map_err(|error| service_authority_error(format!("could not inspect service store: {error}")))?
        .len();
    let record_size = u64::try_from(record.len()).map_err(|_| {
        JetServiceError::Policy("service receipt record is too large".to_string())
    })?;
    if current_size
        .checked_add(record_size)
        .is_none_or(|size| size > SERVICE_AUTH_MAX_BYTES)
    {
        return Err(JetServiceError::Policy(
            "service authority log exceeds its byte limit".to_string(),
        ));
    }
    file.write_all(record.as_bytes())
        .map_err(|error| service_authority_error(format!("could not append service receipt: {error}")))?;
    file.sync_all()
        .map_err(|error| service_authority_error(format!("could not commit service receipt: {error}")))
}

fn service_authority_id(
    runtime: &JetServiceRuntime,
    endpoint: &JetServiceEndpoint,
    message: &str,
    key: &str,
) -> String {
    // Receipt IDs cross process boundaries and are persisted in the authority
    // log. Use the shared Core SHA-256 primitive with length-framed fields so
    // distinct endpoint/message/key tuples cannot alias through concatenation
    // or a small 64-bit hash.
    let mut input = Vec::new();
    let store = service_authority_store_identity(&runtime.store);
    let generation = endpoint.generation.to_string();
    for field in [
        store.as_bytes(),
        endpoint.authority.as_bytes(),
        endpoint.tree.as_bytes(),
        endpoint.worker.as_bytes(),
        generation.as_bytes(),
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

fn service_authority_entries(
    records: &[(char, Vec<String>)],
) -> Result<Vec<ServiceAuthorityEntry>, JetServiceError> {
    let mut entries: Vec<ServiceAuthorityEntry> = Vec::new();
    for (op, fields) in records {
        match (*op, fields.as_slice()) {
            ('S', [id, key, authority, tree, worker, generation, message, _created, expires]) => {
                let generation = generation.parse::<i64>().map_err(|_| {
                    service_authority_error("service send generation is malformed")
                })?;
                let expires = expires
                    .parse::<i64>()
                    .map_err(|_| service_authority_error("service send expiry is malformed"))?;
                if let Some(previous) = entries
                    .iter_mut()
                    .find(|entry| entry.id.as_str() == id.as_str())
                {
                    *previous = ServiceAuthorityEntry {
                        id: id.clone(),
                        key: key.clone(),
                        tree: tree.clone(),
                        worker: worker.clone(),
                        authority: authority.clone(),
                        generation,
                        message: message.clone(),
                        expires,
                        retained_until: None,
                        delivered_to_worker: false,
                        delivered: false,
                        dead: false,
                    };
                } else {
                    entries.push(ServiceAuthorityEntry {
                        id: id.clone(),
                        key: key.clone(),
                        tree: tree.clone(),
                        worker: worker.clone(),
                        authority: authority.clone(),
                        generation,
                        message: message.clone(),
                        expires,
                        retained_until: None,
                        delivered_to_worker: false,
                        delivered: false,
                        dead: false,
                    });
                }
            }
            ('S', [id, key, tree, worker, generation, message, _created, expires]) => {
                let generation = generation.parse::<i64>().map_err(|_| {
                    service_authority_error("service send generation is malformed")
                })?;
                let expires = expires
                    .parse::<i64>()
                    .map_err(|_| service_authority_error("service send expiry is malformed"))?;
                entries.push(ServiceAuthorityEntry {
                    id: id.clone(),
                    key: key.clone(),
                    tree: tree.clone(),
                    worker: worker.clone(),
                    authority: String::new(),
                    generation,
                    message: message.clone(),
                    expires,
                    retained_until: None,
                    delivered_to_worker: false,
                    delivered: false,
                    dead: false,
                });
            }
            ('K', [id, until]) => {
                let until = until
                    .parse::<i64>()
                    .map_err(|_| service_authority_error("service retention deadline is malformed"))?;
                let entry = entries
                    .iter_mut()
                    .find(|entry| entry.id.as_str() == id.as_str())
                    .ok_or_else(|| service_authority_error("service retention references an unknown id"))?;
                entry.retained_until = Some(until);
                entry.delivered_to_worker = false;
                entry.delivered = false;
                entry.dead = false;
            }
            ('D', [id, _reason]) => {
                let entry = entries
                    .iter_mut()
                    .find(|entry| entry.id.as_str() == id.as_str())
                    .ok_or_else(|| service_authority_error("dead letter references an unknown id"))?;
                entry.dead = true;
                entry.retained_until = None;
            }
            ('R', [id, created, expires]) => {
                let created = created
                    .parse::<i64>()
                    .map_err(|_| service_authority_error("service retry timestamp is malformed"))?;
                let expires = expires
                    .parse::<i64>()
                    .map_err(|_| service_authority_error("service retry expiry is malformed"))?;
                let entry = entries
                    .iter_mut()
                    .find(|entry| entry.id.as_str() == id.as_str())
                    .ok_or_else(|| service_authority_error("retry references an unknown id"))?;
                let _ = created;
                entry.expires = expires;
                entry.delivered_to_worker = false;
                entry.delivered = false;
                entry.dead = false;
                entry.retained_until = None;
            }
            ('A', [id]) => {
                let entry = entries
                    .iter_mut()
                    .find(|entry| entry.id.as_str() == id.as_str())
                    .ok_or_else(|| service_authority_error("commit references an unknown id"))?;
                if !entry.delivered_to_worker {
                    return Err(service_authority_error(
                        "commit precedes durable worker delivery",
                    ));
                }
                entry.delivered = true;
            }
            ('V', [id]) => {
                let entry = entries
                    .iter_mut()
                    .find(|entry| entry.id.as_str() == id.as_str())
                    .ok_or_else(|| service_authority_error("delivery references an unknown id"))?;
                entry.delivered_to_worker = true;
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
) -> Result<JetServiceReceipt, JetServiceError> {
    service_authority_validate_runtime(runtime)?;
    service_authority_validate_endpoint(endpoint)?;
    jet_services_authority_validate(endpoint)?;
    service_authority_bind_store(runtime, endpoint)?;
    service_authority_validate_text(key, "idempotency key", SERVICE_AUTH_MAX_KEY, false)?;
    service_authority_validate_text(message, "service message", SERVICE_AUTH_MAX_MESSAGE, true)?;
    let _guard = service_authority_lock()
        .lock()
        .map_err(|_| service_authority_error("service authority lock is poisoned"))?;
    let records = service_authority_read(runtime)?;
    let mut entries = service_authority_entries(&records)?;
    let now = service_authority_now();
    if service_authority_cleanup_expired(runtime, &entries, now)? {
        entries = service_authority_entries(&service_authority_read(runtime)?)?;
    }
    if let Some(entry) = entries.iter().find(|entry| {
        entry.key.as_str() == key.as_str()
            && entry.authority == endpoint.authority
            && entry.generation == endpoint.generation
    }) {
        if entry.tree != endpoint.tree
            || entry.worker != endpoint.worker
            || entry.message.as_str() != message.as_str()
        {
            return Ok(JetServiceReceipt::Rejected(
                "idempotency key was already used for a different delivery".to_string(),
            ));
        }
        if entry.dead {
            return Ok(JetServiceReceipt::DeadLettered(entry.id.clone()));
        }
        if let Some(until) = entry.retained_until {
            if until > now {
                return Ok(JetServiceReceipt::Retained {
                    id: entry.id.clone(),
                    until,
                });
            }
        }
        if entry.expires > now {
            if !entry.delivered {
                service_authority_enqueue_entry(runtime, entry)?;
            }
            return Ok(JetServiceReceipt::Duplicate(entry.id.clone()));
        }
        return Ok(JetServiceReceipt::DeadLettered(entry.id.clone()));
    }
    let id = service_authority_id(runtime, endpoint, message, key);
    let expires = service_authority_retention_deadline(now, runtime.retention_ms);
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
        ],
    )?;
    jet_services_authority_enqueue(runtime, endpoint, &id, message)?;
    Ok(JetServiceReceipt::Accepted(id))
}

pub fn jet_services_runtime_retry(
    runtime: &JetServiceRuntime,
    id: &String,
) -> Result<JetServiceReceipt, JetServiceError> {
    service_authority_validate_runtime(runtime)?;
    service_authority_validate_text(id, "service id", SERVICE_AUTH_MAX_KEY, false)?;
    let _guard = service_authority_lock()
        .lock()
        .map_err(|_| service_authority_error("service authority lock is poisoned"))?;
    let records = service_authority_read(runtime)?;
    let mut entries = service_authority_entries(&records)?;
    let now = service_authority_now();
    if service_authority_cleanup_expired(runtime, &entries, now)? {
        entries = service_authority_entries(&service_authority_read(runtime)?)?;
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
    if let Some(until) = entry.retained_until {
        if until > service_authority_now() {
            service_authority_enqueue_entry(runtime, entry)?;
            return Ok(JetServiceReceipt::Retained {
                id: entry.id.clone(),
                until,
            });
        }
    }
    if entry.dead {
        return Ok(JetServiceReceipt::DeadLettered(entry.id.clone()));
    }
    if !entry.delivered {
        service_authority_enqueue_entry(runtime, entry)?;
    }
    Ok(JetServiceReceipt::Duplicate(id.clone()))
}

pub fn jet_services_runtime_dead_letter(
    runtime: &JetServiceRuntime,
    id: &String,
) -> Result<JetServiceReceipt, JetServiceError> {
    service_authority_validate_runtime(runtime)?;
    service_authority_validate_text(id, "service id", SERVICE_AUTH_MAX_KEY, false)?;
    let _guard = service_authority_lock()
        .lock()
        .map_err(|_| service_authority_error("service authority lock is poisoned"))?;
    let mut entries = service_authority_entries(&service_authority_read(runtime)?)?;
    if service_authority_cleanup_expired(runtime, &entries, service_authority_now())? {
        entries = service_authority_entries(&service_authority_read(runtime)?)?;
    }
    let Some(entry) = entries
        .iter()
        .find(|entry| entry.id.as_str() == id.as_str())
    else {
        return Err(JetServiceError::Unknown(format!("service id `{id}` is unknown")));
    };
    if entry.dead {
        return Ok(JetServiceReceipt::DeadLettered(id.clone()));
    }
    service_authority_append(runtime, 'D', &[id.clone(), "explicit dead letter".to_string()])?;
    service_authority_remove_pending_entry(runtime, entry)?;
    Ok(JetServiceReceipt::DeadLettered(id.clone()))
}

pub fn jet_services_runtime_retain(
    runtime: &JetServiceRuntime,
    id: &String,
) -> Result<JetServiceReceipt, JetServiceError> {
    service_authority_validate_runtime(runtime)?;
    service_authority_validate_text(id, "service id", SERVICE_AUTH_MAX_KEY, false)?;
    let _guard = service_authority_lock()
        .lock()
        .map_err(|_| service_authority_error("service authority lock is poisoned"))?;
    let mut entries = service_authority_entries(&service_authority_read(runtime)?)?;
    if service_authority_cleanup_expired(runtime, &entries, service_authority_now())? {
        entries = service_authority_entries(&service_authority_read(runtime)?)?;
    }
    let Some(entry) = entries
        .iter()
        .find(|entry| entry.id.as_str() == id.as_str())
    else {
        return Err(JetServiceError::Unknown(format!("service id `{id}` is unknown")));
    };
    if entry.dead {
        return Ok(JetServiceReceipt::DeadLettered(id.clone()));
    }
    let until = service_authority_retention_deadline(service_authority_now(), runtime.retention_ms);
    service_authority_append(runtime, 'K', &[id.clone(), until.to_string()])?;
    Ok(JetServiceReceipt::Retained {
        id: id.clone(),
        until,
    })
}

/// Durably acknowledge that a delivered receipt has completed its service
/// transaction. The acknowledgement is separate from mailbox enqueue so a
/// crash between durable send and worker delivery remains recoverable by
/// `retry`; an already acknowledged receipt is idempotent.
pub fn jet_services_runtime_commit(
    runtime: &JetServiceRuntime,
    id: &String,
) -> Result<(), JetServiceError> {
    service_authority_validate_runtime(runtime)?;
    service_authority_validate_text(id, "service id", SERVICE_AUTH_MAX_KEY, false)?;
    let _guard = service_authority_lock()
        .lock()
        .map_err(|_| service_authority_error("service authority lock is poisoned"))?;
    let mut entries = service_authority_entries(&service_authority_read(runtime)?)?;
    let now = service_authority_now();
    let target_expired = entries
        .iter()
        .find(|entry| entry.id.as_str() == id.as_str())
        .is_some_and(|entry| service_authority_entry_expired(entry, now));
    if service_authority_cleanup_expired(runtime, &entries, now)? {
        entries = service_authority_entries(&service_authority_read(runtime)?)?;
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
        service_authority_append(runtime, 'A', &[id.clone()])?;
    }
    service_authority_remove_pending_entry(runtime, entry)?;
    Ok(())
}
