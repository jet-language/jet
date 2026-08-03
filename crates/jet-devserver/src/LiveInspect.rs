//! D-OBSERVE-LIVE1=A: bounded readers and projections for the runtime-owned
//! live snapshot. This module never reads process memory and never exposes
//! channel payloads, locals, environment values, or credentials.

use jet_foundation::JSON::{json_get, json_int, json_str, parse_json, JSONValue};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_SNAPSHOT_BYTES: u64 = 1024 * 1024;
const MAX_SNAPSHOT_AGE_MS: u128 = 2_000;
const MAX_SNAPSHOT_ITEMS: usize = 4096;

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
    schema_version: i64,
    pid: i64,
    start_id: String,
    captured_ms: u128,
    tasks: Vec<LiveTask>,
    channels: Vec<LiveChannel>,
    effects: LiveEffects,
    resources: LiveResources,
    has_event_observations: bool,
}

fn checked_object<'a>(
    value: &'a JSONValue,
    required: &[&str],
    optional: &[&str],
    label: &str,
) -> Result<&'a std::collections::HashMap<String, JSONValue>, String> {
    let JSONValue::Object(fields) = value else {
        return Err(format!("{label} must be an object"));
    };
    if fields.keys().any(|key| {
        !required.contains(&key.as_str()) && !optional.contains(&key.as_str())
    }) || required.iter().any(|key| !fields.contains_key(*key)) {
        return Err(format!("{label} has missing or unsafe fields"));
    }
    Ok(fields)
}

fn int_field(
    object: &JSONValue,
    key: &str,
    label: &str,
    nonnegative: bool,
) -> Result<i64, String> {
    let value = json_get(object, key)
        .and_then(json_int)
        .ok_or_else(|| format!("{label} has invalid `{key}`"))?;
    if nonnegative && value < 0 {
        return Err(format!("{label} has negative `{key}`"));
    }
    Ok(value)
}

fn string_field(object: &JSONValue, key: &str, label: &str) -> Result<String, String> {
    let value = json_get(object, key)
        .and_then(json_str)
        .ok_or_else(|| format!("{label} has invalid `{key}`"))?;
    if value.chars().any(char::is_control) {
        return Err(format!("{label} has control characters in `{key}`"));
    }
    Ok(value.to_string())
}

fn bool_field(object: &JSONValue, key: &str, label: &str) -> Result<bool, String> {
    match json_get(object, key) {
        Some(JSONValue::Bool(value)) => Ok(*value),
        _ => Err(format!("{label} has invalid `{key}`")),
    }
}

fn optional_int_field(
    object: &JSONValue,
    key: &str,
    label: &str,
) -> Result<Option<i64>, String> {
    match json_get(object, key) {
        Some(JSONValue::Null) => Ok(None),
        Some(JSONValue::Number(value)) if *value >= 0 => Ok(Some(*value)),
        _ => Err(format!("{label} has invalid `{key}`")),
    }
}

fn parse_live_snapshot(snapshot: &str) -> Result<LiveSnapshot, String> {
    let root = parse_json(snapshot)
        .map_err(|()| "live runtime snapshot is not valid JSON".to_string())?;
    let root_fields = checked_object(
        &root,
        &[
            "schema_version",
            "pid",
            "start_id",
            "captured_ms",
            "tasks",
            "channels",
            "effects",
            "resources",
        ],
        &["event_observations"],
        "live runtime snapshot",
    )?;
    let schema_version = int_field(&root, "schema_version", "live runtime snapshot", true)?;
    if schema_version != 1 {
        return Err("live runtime snapshot has an unsupported schema version".to_string());
    }
    let pid = int_field(&root, "pid", "live runtime snapshot", true)?;
    let start_id = string_field(&root, "start_id", "live runtime snapshot")?;
    let captured = int_field(&root, "captured_ms", "live runtime snapshot", true)?;
    let captured_ms = u128::try_from(captured)
        .map_err(|_| "live runtime snapshot capture time is out of range".to_string())?;

    let tasks_value = root_fields
        .get("tasks")
        .ok_or_else(|| "live runtime snapshot has no tasks".to_string())?;
    let JSONValue::Array(task_values) = tasks_value else {
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
            &[],
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
    let JSONValue::Array(channel_values) = channels_value else {
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
        schema_version,
        pid,
        start_id,
        captured_ms,
        tasks,
        channels,
        effects,
        resources,
        has_event_observations: root_fields.contains_key("event_observations"),
    })
}

pub fn snapshot_path(pid: u32) -> PathBuf {
    std::env::temp_dir().join(format!("jet-observe-{pid}.json"))
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
    let parsed = parse_live_snapshot(&snapshot)
        .map_err(|error| format!("live runtime snapshot has an invalid or unsafe schema: {error}"))?;
    if parsed.pid != i64::from(pid) {
        return Err(format!("live runtime snapshot does not belong to process {pid}"));
    }
    jet_debug::render_event_observations(&snapshot)
        .map_err(|error| format!("live runtime event observations are invalid: {error}"))?;
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    if now_ms.saturating_sub(parsed.captured_ms) > MAX_SNAPSHOT_AGE_MS {
        return Err(format!("process {pid} is not publishing a live runtime snapshot"));
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
            return Err(format!("live runtime snapshot does not belong to process {pid}"));
        }
    }
    Ok(snapshot)
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
    let mut out = format!("jet inspect live · pid {pid}\n\ntask tree\n");
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
                if task.wait.is_empty() { "-" } else { &task.wait },
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
        let snapshot = "{\"schema_version\":1,\"pid\":42,\"captured_ms\":1,\"tasks\":[{\"id\":1,\"parent\":0,\"state\":\"running\",\"wait\":\"\",\"deadline_ms\":null,\"cancelled\":false},{\"id\":2,\"parent\":1,\"state\":\"blocked\",\"wait\":\"channel send\",\"deadline_ms\":480,\"cancelled\":false}],\"channels\":[{\"id\":1,\"depth\":4,\"capacity\":4,\"send_waiters\":1,\"recv_waiters\":0,\"closed\":false}],\"effects\":{\"compute\":1,\"waiting\":1,\"channel\":1,\"time\":0,\"io\":0},\"resources\":{\"workers\":4,\"running\":1,\"queued\":0,\"cancelled\":0,\"arenas\":0,\"arena_allocations\":0,\"arena_bytes\":0}}";
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
