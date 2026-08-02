// D-SERVICE-AUTHORITY1: one durable authority for AOT and ambient execution.
// The log is append-only, length/hex framed, and fsync'd after every commit.
// A process-wide lock closes the read/append race between runtime instances.

use std::fs::OpenOptions;
use std::io::{Read, Write};
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
    service_authority_validate_text(&runtime.store, "service store", SERVICE_AUTH_MAX_STORE, false)
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

pub fn jet_services_authority_new_tree(name: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in name.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("sa-{hash:016x}")
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
        JetServiceError::Unavailable("service endpoint authority is not registered".to_string())
    })?;
    if state.tree != endpoint.tree || state.worker != endpoint.worker {
        return Err(JetServiceError::Unavailable(
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
        JetServiceError::Unavailable("service endpoint authority is not registered".to_string())
    })?;
    if state.tree != tree || !state.started {
        return Err(JetServiceError::NotStarted(format!(
            "service worker `{worker}` is not running"
        )));
    }
    Ok(JetServiceEndpoint {
        tree: state.tree.clone(),
        worker: state.worker.clone(),
        generation: state.generation,
        authority: authority.to_string(),
    })
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
        JetServiceError::Unavailable("service endpoint authority is not registered".to_string())
    })?;
    if state.tree != endpoint.tree || state.generation != endpoint.generation {
        return Err(JetServiceError::Unavailable(
            "service endpoint is stale or belongs to another tree".to_string(),
        ));
    }
    if !state.started {
        return Err(JetServiceError::NotStarted(format!(
            "service worker `{}` is not running",
            endpoint.worker
        )));
    }
    Ok(())
}

pub fn jet_services_authority_enqueue(
    runtime: &JetServiceRuntime,
    endpoint: &JetServiceEndpoint,
    id: &str,
    message: &str,
) -> Result<(), JetServiceError> {
    service_authority_validate_runtime(runtime)?;
    jet_services_authority_validate(endpoint)?;
    service_authority_validate_text(id, "service id", SERVICE_AUTH_MAX_KEY, false)?;
    service_authority_validate_text(message, "service message", SERVICE_AUTH_MAX_MESSAGE, true)?;
    let key = service_endpoint_key(&endpoint.authority, &endpoint.worker);
    let mut pending = service_pending_registry()
        .lock()
        .map_err(|_| service_authority_error("service pending registry lock is poisoned"))?;
    let queue = pending.entry(key).or_default();
    if queue.iter().any(|(queued_id, _, queued_store)| {
        queued_id == id && queued_store == &runtime.store
    }) {
        return Ok(());
    }
    if queue.len() >= SERVICE_AUTH_MAX_PENDING {
        return Err(JetServiceError::Full(
            "service authority pending delivery queue is full".to_string(),
        ));
    }
    queue.push((id.to_string(), message.to_string(), runtime.store.clone()));
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
    let key = service_endpoint_key(&endpoint.authority, &endpoint.worker);
    let mut pending = service_pending_registry()
        .lock()
        .map_err(|_| service_authority_error("service pending registry lock is poisoned"))?;
    let queue = pending.entry(key).or_default();
    let count = queue.len().min(capacity as usize);
    Ok(queue.drain(..count).collect())
}

/// Record that a pending receipt reached the worker mailbox. This is separate
/// from `commit`: a crash after this record and before application commit is
/// still retryable, while a crash before this record is rebuilt from `S`.
pub fn jet_services_authority_mark_delivered(
    store: &str,
    id: &str,
) -> Result<(), JetServiceError> {
    service_authority_validate_text(store, "service store", SERVICE_AUTH_MAX_STORE, false)?;
    service_authority_validate_text(id, "service id", SERVICE_AUTH_MAX_KEY, false)?;
    let runtime = JetServiceRuntime {
        store: store.to_string(),
        retention_ms: 0,
    };
    let _guard = service_authority_lock()
        .lock()
        .map_err(|_| service_authority_error("service authority lock is poisoned"))?;
    let entries = service_authority_entries(&service_authority_read(&runtime)?)?;
    let entry = entries
        .iter()
        .find(|entry| entry.id.as_str() == id)
        .ok_or_else(|| JetServiceError::Unknown(format!("service id `{id}` is unknown")))?;
    if entry.dead {
        return Err(JetServiceError::Unavailable(
            "cannot deliver a dead-lettered service receipt".to_string(),
        ));
    }
    if !entry.delivered_to_worker {
        service_authority_append(&runtime, 'V', &[id.to_string()])?;
    }
    if let Ok(mut pending) = service_pending_registry().lock() {
        for queue in pending.values_mut() {
            queue.retain(|(queued_id, _, queued_store)| {
                queued_id != id || queued_store != store
            });
        }
    }
    Ok(())
}

fn service_authority_enqueue_entry(
    runtime: &JetServiceRuntime,
    entry: &ServiceAuthorityEntry,
) -> Result<(), JetServiceError> {
    let endpoint =
        service_authority_current_endpoint(&entry.authority, &entry.tree, &entry.worker)?;
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
    let record = service_authority_record(op, fields);
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
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

fn service_authority_id(endpoint: &JetServiceEndpoint, message: &str, key: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    let generation = endpoint.generation.to_string();
    for field in [
        endpoint.authority.as_bytes(),
        endpoint.tree.as_bytes(),
        endpoint.worker.as_bytes(),
        generation.as_bytes(),
        message.as_bytes(),
        key.as_bytes(),
    ] {
        for byte in field {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("svc-{hash:016x}")
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
    service_authority_validate_text(key, "idempotency key", SERVICE_AUTH_MAX_KEY, false)?;
    service_authority_validate_text(message, "service message", SERVICE_AUTH_MAX_MESSAGE, true)?;
    let _guard = service_authority_lock()
        .lock()
        .map_err(|_| service_authority_error("service authority lock is poisoned"))?;
    let records = service_authority_read(runtime)?;
    let entries = service_authority_entries(&records)?;
    let now = service_authority_now();
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
        service_authority_append(
            runtime,
            'D',
            &[entry.id.clone(), "retention expired".to_string()],
        )?;
        return Ok(JetServiceReceipt::DeadLettered(entry.id.clone()));
    }
    let id = service_authority_id(endpoint, message, key);
    let expires = now.saturating_add(runtime.retention_ms.max(0));
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
    let entries = service_authority_entries(&records)?;
    let entry = entries
        .iter()
        .find(|entry| entry.id.as_str() == id.as_str())
        .ok_or_else(|| JetServiceError::Unknown(format!("service id `{id}` is unknown")))?;
    if entry.authority.is_empty() {
        return Err(JetServiceError::Unavailable(
            "service receipt has no endpoint authority".to_string(),
        ));
    }
    let now = service_authority_now();
    if let Some(until) = entry.retained_until {
        if until > now {
            service_authority_enqueue_entry(runtime, entry)?;
            return Ok(JetServiceReceipt::Retained {
                id: entry.id.clone(),
                until,
            });
        }
        service_authority_append(
            runtime,
            'D',
            &[entry.id.clone(), "retention expired".to_string()],
        )?;
        return Ok(JetServiceReceipt::DeadLettered(entry.id.clone()));
    }
    if entry.dead {
        return Ok(JetServiceReceipt::DeadLettered(entry.id.clone()));
    }
    if entry.expires <= now {
        let expires = now.saturating_add(runtime.retention_ms.max(0));
        service_authority_append(runtime, 'R', &[id.clone(), now.to_string(), expires.to_string()])?;
        service_authority_enqueue_entry(runtime, entry)?;
        return Ok(JetServiceReceipt::Accepted(id.clone()));
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
    let entries = service_authority_entries(&service_authority_read(runtime)?)?;
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
    if !entry.authority.is_empty() {
        let key = service_endpoint_key(&entry.authority, &entry.worker);
        if let Ok(mut pending) = service_pending_registry().lock() {
            if let Some(queue) = pending.get_mut(&key) {
                queue.retain(|(queued_id, _, _)| queued_id != id);
            }
        }
    }
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
    let entries = service_authority_entries(&service_authority_read(runtime)?)?;
    let Some(entry) = entries
        .iter()
        .find(|entry| entry.id.as_str() == id.as_str())
    else {
        return Err(JetServiceError::Unknown(format!("service id `{id}` is unknown")));
    };
    if entry.dead {
        return Ok(JetServiceReceipt::DeadLettered(id.clone()));
    }
    let until = service_authority_now().saturating_add(runtime.retention_ms.max(0));
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
    let entries = service_authority_entries(&service_authority_read(runtime)?)?;
    let entry = entries
        .iter()
        .find(|entry| entry.id.as_str() == id.as_str())
        .ok_or_else(|| JetServiceError::Unknown(format!("service id `{id}` is unknown")))?;
    if entry.dead {
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
    if let Ok(mut pending) = service_pending_registry().lock() {
        for queue in pending.values_mut() {
            queue.retain(|(queued_id, _, _)| queued_id != id);
        }
    }
    Ok(())
}
