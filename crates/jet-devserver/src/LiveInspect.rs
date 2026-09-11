//! D-OBSERVE-LIVE1=A: bounded readers and projections for the runtime-owned
//! live snapshot. This module never reads process memory and never exposes
//! channel payloads, locals, environment values, or credentials.

use jet_foundation::Devtools::{JetDevtoolsEnvelope, JET_DEVTOOLS_MAX_ENVELOPE_BYTES};
use jet_foundation::DevtoolsControl::JetDevtoolsDecodedEnvelope;
use jet_foundation::DataTree::DataTree;
use jet_foundation::JSON::{json_get, json_int, json_str, parse_json};
use jet_foundation::MIR::MirDecisionLedger;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_SNAPSHOT_BYTES: u64 = 1024 * 1024;
const MAX_SNAPSHOT_AGE_MS: u128 = 2_000;
const MAX_SNAPSHOT_ITEMS: usize = 4096;
const MAX_DEVTOOLS_RELAY_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Debug)]
struct LiveTask {
    id: i64,
    parent: i64,
    state: String,
    wait: String,
    deadline_ms: Option<i64>,
    cancelled: bool,
}

#[derive(Clone, Debug)]
struct LiveChannel {
    id: i64,
    depth: i64,
    capacity: Option<i64>,
    send_waiters: i64,
    recv_waiters: i64,
    closed: bool,
}

#[derive(Clone, Debug)]
struct LiveEffects {
    compute: i64,
    waiting: i64,
    channel: i64,
    time: i64,
    io: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveInspectValue {
    pub value_id: String,
    pub type_identity: String,
    pub disposition: String,
    pub reason: String,
    pub rendered_value: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveInspectShape {
    pub type_identity: String,
    pub value_ids: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveInspectProjection {
    pub plane: String,
    pub scope: String,
    pub pid: u32,
    pub protocol: String,
    pub session_id: String,
    pub source_id: String,
    pub build_id: String,
    pub revision: String,
    pub world_id: String,
    pub values: Vec<LiveInspectValue>,
    pub shapes: Vec<LiveInspectShape>,
    pub decision_ledger: Option<MirDecisionLedger>,
}


#[derive(Clone, Debug)]
struct LiveResources {
    workers: i64,
    running: i64,
    queued: i64,
    cancelled: i64,
    arenas: i64,
    arena_allocations: i64,
    arena_bytes: i64,
}


#[derive(Clone, Debug)]
struct LiveSnapshot {
    protocol: String,
    session_id: String,
    source_id: String,
    build_id: String,
    revision: String,
    world_id: String,
    pid: i64,
    start_id: String,
    captured_ms: u128,
    tasks: Vec<LiveTask>,
    channels: Vec<LiveChannel>,
    effects: LiveEffects,
    resources: LiveResources,
    values: Vec<LiveInspectValue>,
    decision_ledger: Option<MirDecisionLedger>,
    has_event_observations: bool,
}

fn checked_object(
    value: &DataTree,
    required: &[&str],
    optional: &[&str],
    label: &str,
) -> Result<BTreeMap<String, DataTree>, String> {
    let DataTree::Object(fields) = value else {
        return Err(format!("{label} must be an object"));
    };
    let fields: BTreeMap<String, DataTree> = fields.iter().cloned().collect();
    if fields
        .keys()
        .any(|key| !required.contains(&key.as_str()) && !optional.contains(&key.as_str()))
        || required.iter().any(|key| !fields.contains_key(*key))
    {
        return Err(format!("{label} has missing or unsafe fields"));
    }
    Ok(fields)
}

fn int_field(object: &DataTree, key: &str, label: &str, nonnegative: bool) -> Result<i64, String> {
    let value = json_get(object, key)
        .and_then(json_int)
        .ok_or_else(|| format!("{label} has invalid `{key}`"))?;
    if nonnegative && value < 0 {
        return Err(format!("{label} has negative `{key}`"));
    }
    Ok(value)
}

fn optional_int_field(
    object: &DataTree,
    key: &str,
    label: &str,
) -> Result<Option<i64>, String> {
    match json_get(object, key) {
        None | Some(DataTree::Null) => Ok(None),
        Some(value) => json_int(value)
            .map(Some)
            .ok_or_else(|| format!("{label} has invalid `{key}`")),
    }
}

fn string_field(object: &DataTree, key: &str, label: &str) -> Result<String, String> {
    let value = json_get(object, key)
        .and_then(json_str)
        .ok_or_else(|| format!("{label} has invalid `{key}`"))?;
    if value.chars().any(char::is_control) {
        return Err(format!("{label} has control characters in `{key}`"));
    }
    Ok(value.to_string())
}

fn bool_field(object: &DataTree, key: &str, label: &str) -> Result<bool, String> {
    match json_get(object, key) {
        Some(DataTree::Bool(value)) => Ok(*value),
        _ => Err(format!("{label} has invalid `{key}`")),
    }
}

fn canonical_json_value(value: &DataTree) -> Result<String, String> {
    match value {
        DataTree::Null => Ok("null".to_string()),
        DataTree::Bool(value) => Ok(value.to_string()),
        DataTree::Int(value) => Ok(value.to_string()),
        DataTree::Float(value) if value.is_finite() => Ok(value.to_string()),
        DataTree::Float(_) => Err("decision ledger contains a non-finite number".to_string()),
        DataTree::Number(value) => Ok(value.clone()),
        DataTree::TypedText(value) | DataTree::Text(value) => {
            Ok(jet_foundation::JSON::quote(value))
        }
        DataTree::Bytes(values) => Ok(format!(
            "[{}]",
            values
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        )),
        DataTree::Array(values) => Ok(format!(
            "[{}]",
            values
                .iter()
                .map(canonical_json_value)
                .collect::<Result<Vec<_>, _>>()?
                .join(",")
        )),
        DataTree::Object(fields) => {
            let mut fields = fields.iter().collect::<Vec<_>>();
            fields.sort_by(|left, right| left.0.cmp(&right.0));
            let rendered = fields
                .into_iter()
                .map(|(key, value)| {
                    Ok(format!(
                        "{}:{}",
                        jet_foundation::JSON::quote(key),
                        canonical_json_value(value)?
                    ))
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(format!("{{{}}}", rendered.join(",")))
        }
    }
}

fn parse_decision_ledger(value: Option<&DataTree>) -> Result<Option<MirDecisionLedger>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    if matches!(value, DataTree::Null) {
        return Ok(None);
    }
    let payload = canonical_json_value(value)?;
    if payload.len() > 512 * 1024 {
        return Err("live decision ledger exceeds its transport limit".to_string());
    }
    MirDecisionLedger::from_json(&payload).map(Some)
}

fn parse_live_snapshot(snapshot: &str) -> Result<LiveSnapshot, String> {
    let root =
        parse_json(snapshot).map_err(|()| "live runtime snapshot is not valid JSON".to_string())?;
    let root_fields = checked_object(
        &root,
        &[
            "protocol",
            "session_id",
            "source_id",
            "build_id",
            "revision",
            "world_id",
            "values",
            "schema_version",
            "pid",
            "start_id",
            "captured_ms",
            "tasks",
            "channels",
            "effects",
            "resources",
        ],
        &["event_observations", "decisions"],
        "live runtime snapshot",
    )?;
    let decision_ledger = parse_decision_ledger(root_fields.get("decisions"))?;
    let schema_version = int_field(&root, "schema_version", "live runtime snapshot", true)?;
    let protocol = string_field(&root, "protocol", "live runtime snapshot")?;
    if protocol != "jet.live.v1" {
        return Err("live runtime snapshot protocol must be jet.live.v1".to_string());
    }
    let session_id = string_field(&root, "session_id", "live runtime snapshot")?;
    let source_id = string_field(&root, "source_id", "live runtime snapshot")?;
    let build_id = string_field(&root, "build_id", "live runtime snapshot")?;
    let revision = string_field(&root, "revision", "live runtime snapshot")?;
    let world_id = string_field(&root, "world_id", "live runtime snapshot")?;
    if schema_version != 1 {
        return Err("live runtime snapshot has an unsupported schema version".to_string());
    }
    let pid = int_field(&root, "pid", "live runtime snapshot", true)?;
    let start_id = string_field(&root, "start_id", "live runtime snapshot")?;
    let captured = int_field(&root, "captured_ms", "live runtime snapshot", true)?;
    let captured_ms = u128::try_from(captured)
        .map_err(|_| "live runtime snapshot capture time is out of range".to_string())?;

    let values_value = root_fields
        .get("values")
        .ok_or_else(|| "live runtime snapshot has no values".to_string())?;
    let DataTree::Array(value_values) = values_value else {
        return Err("live runtime snapshot values are not an array".to_string());
    };
    if value_values.len() > MAX_SNAPSHOT_ITEMS {
        return Err("live runtime snapshot has too many values".to_string());
    }
    let mut values = Vec::with_capacity(value_values.len());
    for value in value_values {
        checked_object(
            value,
            &["value_id", "type_identity", "disposition", "reason"],
            &["rendered_value"],
            "live value",
        )?;
        let disposition = string_field(value, "disposition", "live value")?;
        if !matches!(disposition.as_str(), "kept" | "migrated" | "reset" | "rejected") {
            return Err("live value has an unsupported disposition".to_string());
        }
        let rendered_value = match json_get(value, "rendered_value") {
            None | Some(DataTree::Null) => None,
            Some(DataTree::Text(rendered)) => {
                if rendered.len() > 16 * 1024 {
                    return Err("live value `rendered_value` exceeds the Prelude limit".to_string());
                }
                if rendered.chars().any(char::is_control) {
                    return Err("live value has control characters in `rendered_value`".to_string());
                }
                Some(rendered.to_string())
            }
            Some(_) => return Err("live value has invalid `rendered_value`".to_string()),
        };
        values.push(LiveInspectValue {
            value_id: string_field(value, "value_id", "live value")?,
            type_identity: string_field(value, "type_identity", "live value")?,
            disposition,
            reason: string_field(value, "reason", "live value")?,
            rendered_value,
        });
    }
    let tasks_value = root_fields
        .get("tasks")
        .ok_or_else(|| "live runtime snapshot has no tasks".to_string())?;
    let DataTree::Array(task_values) = tasks_value else {
        return Err("live runtime snapshot tasks are not an array".to_string());
    };
    if task_values.len() > MAX_SNAPSHOT_ITEMS {
        return Err("live runtime snapshot has too many tasks".to_string());
    }
    let mut tasks = Vec::with_capacity(task_values.len());
    for value in task_values {
        checked_object(
            value,
            &["id", "parent", "state", "wait", "deadline_ms", "cancelled"],
            &["label", "spawn_site"],
            "live task",
        )?;
        tasks.push(LiveTask {
            id: int_field(value, "id", "live task", true)?,
            parent: int_field(value, "parent", "live task", true)?,
            state: string_field(value, "state", "live task")?,
            wait: string_field(value, "wait", "live task")?,
            deadline_ms: optional_int_field(value, "deadline_ms", "live task")?,
            cancelled: bool_field(value, "cancelled", "live task")?,
        });
    }

    let channels_value = root_fields
        .get("channels")
        .ok_or_else(|| "live runtime snapshot has no channels".to_string())?;
    let DataTree::Array(channel_values) = channels_value else {
        return Err("live runtime snapshot channels are not an array".to_string());
    };
    if channel_values.len() > MAX_SNAPSHOT_ITEMS {
        return Err("live runtime snapshot has too many channels".to_string());
    }
    let mut channels = Vec::with_capacity(channel_values.len());
    for value in channel_values {
        checked_object(
            value,
            &[
                "id",
                "depth",
                "capacity",
                "send_waiters",
                "recv_waiters",
                "closed",
            ],
            &[],
            "live channel",
        )?;
        channels.push(LiveChannel {
            id: int_field(value, "id", "live channel", true)?,
            depth: int_field(value, "depth", "live channel", true)?,
            capacity: optional_int_field(value, "capacity", "live channel")?,
            send_waiters: int_field(value, "send_waiters", "live channel", true)?,
            recv_waiters: int_field(value, "recv_waiters", "live channel", true)?,
            closed: bool_field(value, "closed", "live channel")?,
        });
    }

    let effects = root_fields
        .get("effects")
        .ok_or_else(|| "live runtime snapshot has no effects".to_string())?;
    checked_object(
        effects,
        &["compute", "waiting", "channel", "time", "io"],
        &[],
        "live effects",
    )?;
    let effects = LiveEffects {
        compute: int_field(effects, "compute", "live effects", true)?,
        waiting: int_field(effects, "waiting", "live effects", true)?,
        channel: int_field(effects, "channel", "live effects", true)?,
        time: int_field(effects, "time", "live effects", true)?,
        io: int_field(effects, "io", "live effects", true)?,
    };

    let resources = root_fields
        .get("resources")
        .ok_or_else(|| "live runtime snapshot has no resources".to_string())?;
    checked_object(
        resources,
        &[
            "workers",
            "running",
            "queued",
            "cancelled",
            "arenas",
            "arena_allocations",
            "arena_bytes",
        ],
        &[],
        "live resources",
    )?;
    let resources = LiveResources {
        workers: int_field(resources, "workers", "live resources", true)?,
        running: int_field(resources, "running", "live resources", true)?,
        queued: int_field(resources, "queued", "live resources", true)?,
        cancelled: int_field(resources, "cancelled", "live resources", true)?,
        arenas: int_field(resources, "arenas", "live resources", true)?,
        arena_allocations: int_field(resources, "arena_allocations", "live resources", true)?,
        arena_bytes: int_field(resources, "arena_bytes", "live resources", true)?,
    };

    Ok(LiveSnapshot {
        protocol,
        session_id,
        source_id,
        build_id,
        revision,
        world_id,
        pid,
        start_id,
        captured_ms,
        tasks,
        channels,
        effects,
        resources,
        values,
        decision_ledger,
        has_event_observations: root_fields.contains_key("event_observations"),
    })
}

pub fn snapshot_path(pid: u32) -> PathBuf {
    std::env::temp_dir().join(format!("jet-observe-{pid}.json"))
}

/// The process-scoped canonical `jet.devtools.v1` relay.  The runtime writes
/// one length-delimited envelope frame atomically; the envelope's bounded event
/// deque is the retained ring.
pub fn devtools_relay_path(pid: u32) -> PathBuf {
    std::env::temp_dir().join(format!("jet-devtools-relay-{pid}.ring"))
}

pub fn read_devtools_relay(pid: u32) -> Result<JetDevtoolsEnvelope, String> {
    if pid == 0 {
        return Err("process id must be greater than zero".to_string());
    }
    read_devtools_relay_path(&devtools_relay_path(pid))
}

/// Read a caller-provisioned relay (used by `jet perf run` after its child
/// exits).  This shares the LiveInspect transport and decoder with attach.
pub fn read_devtools_relay_path(path: &Path) -> Result<JetDevtoolsEnvelope, String> {
    let link_metadata = std::fs::symlink_metadata(path)
        .map_err(|_| "no live jet.devtools.v1 relay is available".to_string())?;
    if !link_metadata.file_type().is_file() {
        return Err("jet.devtools.v1 relay is not a regular file".to_string());
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(0o400000);
    }
    let file = options
        .open(path)
        .map_err(|_| "cannot securely open jet.devtools.v1 relay".to_string())?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("cannot inspect jet.devtools.v1 relay: {error}"))?;
    if !metadata.file_type().is_file() {
        return Err("jet.devtools.v1 relay is not a regular file".to_string());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("jet.devtools.v1 relay permissions expose runtime state".to_string());
        }
        if let Ok(self_metadata) = std::fs::metadata("/proc/self") {
            if metadata.uid() != self_metadata.uid() {
                return Err("jet.devtools.v1 relay belongs to another user".to_string());
            }
        }
    }
    if metadata.len() > MAX_DEVTOOLS_RELAY_BYTES {
        return Err("jet.devtools.v1 relay exceeds its byte limit".to_string());
    }
    let mut bytes = Vec::new();
    use std::io::Read;
    file.take(MAX_DEVTOOLS_RELAY_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read jet.devtools.v1 relay: {error}"))?;
    if bytes.len() as u64 > MAX_DEVTOOLS_RELAY_BYTES {
        return Err("jet.devtools.v1 relay exceeds its byte limit".to_string());
    }
    let mut offset = 0usize;
    let mut latest = None;
    while offset < bytes.len() {
        let header_end = offset
            .checked_add(4)
            .ok_or_else(|| "jet.devtools.v1 relay length overflow".to_string())?;
        if header_end > bytes.len() {
            return Err("jet.devtools.v1 relay has a truncated frame length".to_string());
        }
        let frame_len = u32::from_be_bytes(
            bytes[offset..header_end]
                .try_into()
                .map_err(|_| "jet.devtools.v1 relay frame length is invalid".to_string())?,
        ) as usize;
        if frame_len == 0 || frame_len > JET_DEVTOOLS_MAX_ENVELOPE_BYTES {
            return Err("jet.devtools.v1 relay frame exceeds its envelope limit".to_string());
        }
        let frame_end = header_end
            .checked_add(frame_len)
            .ok_or_else(|| "jet.devtools.v1 relay frame length overflow".to_string())?;
        if frame_end > bytes.len() {
            return Err("jet.devtools.v1 relay has a truncated envelope frame".to_string());
        }
        latest = Some(&bytes[header_end..frame_end]);
        offset = frame_end;
    }
    let frame = latest.ok_or_else(|| "jet.devtools.v1 relay has no envelope frame".to_string())?;
    let payload = std::str::from_utf8(frame)
        .map_err(|_| "jet.devtools.v1 relay envelope is not UTF-8".to_string())?;
    match crate::WebHost::decode_devtools_envelope(payload)? {
        JetDevtoolsDecodedEnvelope::Events(envelope) => Ok(envelope),
        JetDevtoolsDecodedEnvelope::Commands(_) => Err(
            "jet.devtools.v1 relay must contain runtime events, not host commands".to_string(),
        ),
    }
}

/// Read one canonical relay and enforce the identity promised by its
/// provisioning caller before any typed game bodies are projected.
pub fn read_devtools_relay_path_with_identity(
    path: &Path,
    expected_session_id: Option<&str>,
    expected_source_id: Option<&str>,
    expected_build_id: Option<&str>,
    expected_revision: Option<&str>,
    expected_world_id: Option<&str>,
) -> Result<JetDevtoolsEnvelope, String> {
    let envelope = read_devtools_relay_path(path)?;
    validate_devtools_relay_identity(
        &envelope,
        expected_session_id,
        expected_source_id,
        expected_build_id,
        expected_revision,
        expected_world_id,
    )?;
    Ok(envelope)
}

/// Identity and completeness gate shared by `jet perf run` and `attach`.
/// Expectations are optional only where the caller cannot know a producer's
/// value (for example, attach does not provision the session id).
pub fn validate_devtools_relay_identity(
    envelope: &JetDevtoolsEnvelope,
    expected_session_id: Option<&str>,
    expected_source_id: Option<&str>,
    expected_build_id: Option<&str>,
    expected_revision: Option<&str>,
    expected_world_id: Option<&str>,
) -> Result<(), String> {
    if envelope.truncated {
        return Err("jet.devtools.v1 relay is truncated; refusing incomplete trace".to_string());
    }
    if let Some(expected) = expected_session_id {
        if envelope.session_id != expected {
            return Err("jet.devtools.v1 relay session identity mismatch".to_string());
        }
    }
    let identity_expected = expected_source_id.is_some()
        || expected_build_id.is_some()
        || expected_revision.is_some()
        || expected_world_id.is_some();
    let identity = envelope.source_identity.as_ref();
    if identity_expected && identity.is_none() {
        return Err("jet.devtools.v1 relay is missing source/build identity".to_string());
    }
    if let Some(identity) = identity {
        identity.validate()?;
        for (expected, actual, label) in [
            (expected_source_id, identity.source_id.as_deref(), "source"),
            (expected_build_id, identity.build_id.as_deref(), "build"),
            (expected_revision, identity.revision.as_deref(), "revision"),
            (expected_world_id, identity.world_id.as_deref(), "world"),
        ] {
            if let Some(expected) = expected {
                if actual != Some(expected) {
                    return Err(format!(
                        "jet.devtools.v1 relay {label} identity mismatch"
                    ));
                }
            }
        }
    }
    Ok(())
}

pub fn read(pid: u32) -> Result<String, String> {
    if pid == 0 {
        return Err("process id must be greater than zero".to_string());
    }
    let path = snapshot_path(pid);
    let link_metadata = std::fs::symlink_metadata(&path)
        .map_err(|_| format!("no live Jet runtime is observable at process {pid}"))?;
    if !link_metadata.file_type().is_file() {
        return Err("live runtime snapshot is not a regular file".to_string());
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Linux O_NOFOLLOW: refuse a symlink substituted after symlink_metadata.
        options.custom_flags(0o400000);
    }
    let file = options
        .open(&path)
        .map_err(|_| format!("cannot securely open live runtime snapshot for process {pid}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("cannot inspect live runtime snapshot: {error}"))?;
    if !metadata.file_type().is_file() {
        return Err("live runtime snapshot is not a regular file".to_string());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("live runtime snapshot permissions expose runtime state".to_string());
        }
        #[cfg(any(target_os = "linux", target_os = "android"))]
        if let Ok(self_metadata) = std::fs::metadata("/proc/self") {
            if metadata.uid() != self_metadata.uid() {
                return Err("live runtime snapshot belongs to another user".to_string());
            }
        }
    }
    if metadata.len() > MAX_SNAPSHOT_BYTES {
        return Err("live runtime snapshot exceeds the 1 MiB safety limit".to_string());
    }
    let mut snapshot = String::new();
    use std::io::Read;
    file.take(MAX_SNAPSHOT_BYTES + 1)
        .read_to_string(&mut snapshot)
        .map_err(|error| format!("cannot read live runtime snapshot: {error}"))?;
    if snapshot.len() as u64 > MAX_SNAPSHOT_BYTES {
        return Err("live runtime snapshot exceeds the 1 MiB safety limit".to_string());
    }
    let parsed = parse_live_snapshot(&snapshot).map_err(|error| {
        format!("live runtime snapshot has an invalid or unsafe schema: {error}")
    })?;
    if parsed.pid != i64::from(pid) {
        return Err(format!(
            "live runtime snapshot does not belong to process {pid}"
        ));
    }
    jet_debug::render_event_observations(&snapshot)
        .map_err(|error| format!("live runtime event observations are invalid: {error}"))?;
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    if now_ms.saturating_sub(parsed.captured_ms) > MAX_SNAPSHOT_AGE_MS {
        return Err(format!(
            "process {pid} is not publishing a live runtime snapshot"
        ));
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let proc_path = PathBuf::from(format!("/proc/{pid}"));
        if !proc_path.is_dir() {
            return Err(format!("process {pid} is no longer running"));
        }
        let expected_start_id = std::fs::read_to_string(proc_path.join("stat"))
            .ok()
            .and_then(|stat| {
                stat.rsplit_once(") ")
                    .and_then(|(_, tail)| tail.split_whitespace().nth(19))
                    .map(str::to_string)
            })
            .ok_or_else(|| format!("cannot verify process {pid} identity"))?;
        if parsed.start_id != expected_start_id {
            return Err(format!(
                "live runtime snapshot does not belong to process {pid}"
            ));
        }
    }
    Ok(snapshot)
}
/// Read the bounded live snapshot and project the values/shapes contract used
/// by `jet inspect <plane> --live PID`.
///
/// The live runtime publishes canonical value identities and type identities,
/// not a plane-specific compiler table.  Every inspect plane therefore shares
/// this identity projection; a plane-specific caller selects the envelope
/// action while this API remains the one source of live facts.
pub fn read_plane(pid: u32, plane: &str) -> Result<LiveInspectProjection, String> {
    if !matches!(
        plane,
        "types" | "rights" | "claims" | "shapes" | "structure" | "build" | "gates"
            | "decisions"
    ) {
        return Err(format!("unknown live inspect plane `{plane}`"));
    }
    let snapshot = read(pid)?;
    let parsed = parse_live_snapshot(&snapshot)?;
    let pid = u32::try_from(parsed.pid)
        .map_err(|_| "live runtime snapshot pid is out of range".to_string())?;
    let mut shape_values = BTreeMap::<String, Vec<String>>::new();
    for value in &parsed.values {
        shape_values
            .entry(value.type_identity.clone())
            .or_default()
            .push(value.value_id.clone());
    }
    let shapes = shape_values
        .into_iter()
        .map(|(type_identity, value_ids)| LiveInspectShape {
            type_identity,
            value_ids,
        })
        .collect();
    Ok(LiveInspectProjection {
        plane: plane.to_string(),
        scope: "live".to_string(),
        pid,
        protocol: parsed.protocol,
        session_id: parsed.session_id,
        source_id: parsed.source_id,
        build_id: parsed.build_id,
        revision: parsed.revision,
        world_id: parsed.world_id,
        values: parsed.values,
        shapes,
        decision_ledger: parsed.decision_ledger,
    })
}


pub fn render(snapshot: &str) -> String {
    let parsed = match parse_live_snapshot(snapshot) {
        Ok(parsed) => parsed,
        Err(error) => return format!("jet inspect live · invalid snapshot: {error}\n"),
    };
    let event_lines = if parsed.has_event_observations {
        match jet_debug::render_event_observations(snapshot) {
            Ok(events) => Some(events),
            Err(error) => return format!("jet inspect live · invalid snapshot: {error}\n"),
        }
    } else {
        None
    };
    let pid = parsed.pid.to_string();
    let mut out = format!(
        "jet inspect live · pid {pid}\nprotocol={} session={} source={} build={} revision={} world={}\n\ntask tree\n",
        parsed.protocol,
        parsed.session_id,
        parsed.source_id,
        parsed.build_id,
        parsed.revision,
        parsed.world_id,
    );
    if parsed.tasks.is_empty() {
        out.push_str("  (no live tasks)\n");
    } else {
        for task in &parsed.tasks {
            let deadline = task
                .deadline_ms
                .map_or_else(|| "-".to_string(), |deadline| deadline.to_string());
            out.push_str(&format!(
                "  task {:<5} parent {:<5} {:<8} wait={} deadline={} cancelled={}\n",
                task.id,
                task.parent,
                task.state,
                if task.wait.is_empty() {
                    "-"
                } else {
                    &task.wait
                },
                deadline,
                task.cancelled
            ));
        }
    }
    out.push_str("\nchannels\n");
    if parsed.channels.is_empty() {
        out.push_str("  (no live channels)\n");
    } else {
        for channel in &parsed.channels {
            let capacity = channel
                .capacity
                .map_or_else(|| "∞".to_string(), |capacity| capacity.to_string());
            out.push_str(&format!(
                "  channel {:<5} depth {}/{} blocked send={} recv={} closed={}\n",
                channel.id,
                channel.depth,
                capacity,
                channel.send_waiters,
                channel.recv_waiters,
                channel.closed
            ));
        }
    }
    out.push_str(&format!(
        "\neffects: compute={} waiting={} channel={} time={} io={}\nresources: workers={} running={} queued={} cancelled={} arenas={} arena_allocations={} arena_bytes={}\n",
        parsed.effects.compute,
        parsed.effects.waiting,
        parsed.effects.channel,
        parsed.effects.time,
        parsed.effects.io,
        parsed.resources.workers,
        parsed.resources.running,
        parsed.resources.queued,
        parsed.resources.cancelled,
        parsed.resources.arenas,
        parsed.resources.arena_allocations,
        parsed.resources.arena_bytes,
    ));
    out.push_str("\nvalues\n");
    if parsed.values.is_empty() {
        out.push_str("  (no live value decisions)\n");
    } else {
        for value in &parsed.values {
            let rendered = value
                .rendered_value
                .as_deref()
                .map(|value| format!(" value={value}"))
                .unwrap_or_default();
            out.push_str(&format!(
                "  {} type={} {}{}{}\n",
                value.value_id,
                value.type_identity,
                value.disposition,
                if value.reason.is_empty() {
                    String::new()
                } else {
                    format!(" reason={}", value.reason)
                },
                rendered,
            ));
        }
    }
    if let Some(events) = event_lines {
        out.push_str("\nevents\n");
        if events.is_empty() {
            out.push_str("  (no runtime event observations)\n");
        } else {
            for event in events.lines() {
                out.push_str("  ");
                out.push_str(event);
                out.push('\n');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_uses_only_bounded_runtime_facts() {
        let snapshot = "{\"protocol\":\"jet.live.v1\",\"session_id\":\"session\",\"source_id\":\"app.jet\",\"build_id\":\"build-1\",\"revision\":\"rev-1\",\"world_id\":\"world-1\",\"values\":[{\"value_id\":\"app::counter\",\"type_identity\":\"Int\",\"disposition\":\"kept\",\"reason\":\"\"}],\"schema_version\":1,\"pid\":42,\"start_id\":\"test\",\"captured_ms\":1,\"tasks\":[{\"id\":1,\"parent\":0,\"state\":\"running\",\"wait\":\"\",\"deadline_ms\":null,\"cancelled\":false},{\"id\":2,\"parent\":1,\"state\":\"blocked\",\"wait\":\"channel send\",\"deadline_ms\":480,\"cancelled\":false}],\"channels\":[{\"id\":1,\"depth\":4,\"capacity\":4,\"send_waiters\":1,\"recv_waiters\":0,\"closed\":false}],\"effects\":{\"compute\":1,\"waiting\":1,\"channel\":1,\"time\":0,\"io\":0},\"resources\":{\"workers\":4,\"running\":1,\"queued\":0,\"cancelled\":0,\"arenas\":0,\"arena_allocations\":0,\"arena_bytes\":0}}";
        let rendered = render(snapshot);
        assert!(rendered.contains("task 2"));
        assert!(rendered.contains("channel send"));
        assert!(rendered.contains("depth 4/4"));
        assert!(!rendered.contains("payload"));
    }

    #[cfg(unix)]
    #[test]
    fn reader_rejects_symlinks_and_open_permissions() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let pid = std::process::id().saturating_add(1_000_000_000);
        let path = snapshot_path(pid);
        let target = path.with_extension("hostile-target");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&target);

        std::fs::write(&target, "attacker-controlled").unwrap();
        symlink(&target, &path).unwrap();
        assert!(read(pid).unwrap_err().contains("not a regular file"));
        std::fs::remove_file(&path).unwrap();

        std::fs::write(&path, "{}").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read(pid).unwrap_err().contains("permissions expose"));

        std::fs::remove_file(&path).unwrap();
        std::fs::remove_file(&target).unwrap();
    }
}
