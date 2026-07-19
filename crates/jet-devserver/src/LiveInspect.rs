//! D-OBSERVE-LIVE1=A: bounded readers and projections for the runtime-owned
//! live snapshot. This module never reads process memory and never exposes
//! channel payloads, locals, environment values, or credentials.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_SNAPSHOT_BYTES: u64 = 1024 * 1024;
const MAX_SNAPSHOT_AGE_MS: u128 = 2_000;

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
    if !snapshot.starts_with("{\"schema_version\":1,")
        || !snapshot.contains(&format!("\"pid\":{pid},"))
        || !snapshot.contains("\"tasks\":[")
        || !snapshot.contains("\"channels\":[")
        || !snapshot.contains("\"event_observations\":[")
        || snapshot.contains("\"payload\"")
        || snapshot.contains("\"locals\"")
        || snapshot.contains("\"environment\"")
    {
        return Err("live runtime snapshot has an invalid or unsafe schema".to_string());
    }
    jet_debug::render_event_observations(&snapshot)
        .map_err(|error| format!("live runtime event observations are invalid: {error}"))?;
    let captured_ms = number(&snapshot, "captured_ms")
        .and_then(|value| value.parse::<u128>().ok())
        .ok_or_else(|| "live runtime snapshot has no capture time".to_string())?;
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    if now_ms.saturating_sub(captured_ms) > MAX_SNAPSHOT_AGE_MS {
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
        if string(&snapshot, "start_id").as_deref() != Some(expected_start_id.as_str()) {
            return Err(format!("live runtime snapshot does not belong to process {pid}"));
        }
    }
    Ok(snapshot)
}

fn number(object: &str, key: &str) -> Option<String> {
    let tail = object.split_once(&format!("\"{key}\":"))?.1;
    Some(
        tail.chars()
            .take_while(|ch| ch.is_ascii_digit() || *ch == '-')
            .collect(),
    )
}

fn string(object: &str, key: &str) -> Option<String> {
    let tail = object.split_once(&format!("\"{key}\":\""))?.1;
    Some(tail.split('"').next()?.to_string())
}

fn array<'a>(snapshot: &'a str, key: &str) -> Option<&'a str> {
    let tail = snapshot.split_once(&format!("\"{key}\":["))?.1;
    Some(tail.split_once(']')?.0)
}

pub fn render(snapshot: &str) -> String {
    let pid = number(snapshot, "pid").unwrap_or_else(|| "?".to_string());
    let mut out = format!("jet inspect live · pid {pid}\n\ntask tree\n");
    let tasks = array(snapshot, "tasks").unwrap_or("");
    if tasks.is_empty() {
        out.push_str("  (no live tasks)\n");
    } else {
        for object in tasks.split("},{") {
            let id = number(object, "id").unwrap_or_else(|| "?".to_string());
            let parent = number(object, "parent").unwrap_or_else(|| "?".to_string());
            let state = string(object, "state").unwrap_or_else(|| "unknown".to_string());
            let wait = string(object, "wait").unwrap_or_default();
            let deadline = number(object, "deadline_ms").filter(|value| !value.is_empty());
            out.push_str(&format!(
                "  task {id:<5} parent {parent:<5} {state:<8} wait={} deadline={}\n",
                if wait.is_empty() { "-" } else { &wait },
                deadline.as_deref().unwrap_or("-")
            ));
        }
    }
    out.push_str("\nchannels\n");
    let channels = array(snapshot, "channels").unwrap_or("");
    if channels.is_empty() {
        out.push_str("  (no live channels)\n");
    } else {
        for object in channels.split("},{") {
            let id = number(object, "id").unwrap_or_else(|| "?".to_string());
            let depth = number(object, "depth").unwrap_or_else(|| "?".to_string());
            let capacity = number(object, "capacity").filter(|value| !value.is_empty());
            let senders = number(object, "send_waiters").unwrap_or_else(|| "0".to_string());
            let receivers = number(object, "recv_waiters").unwrap_or_else(|| "0".to_string());
            out.push_str(&format!(
                "  channel {id:<5} depth {}/{} blocked send={} recv={}\n",
                depth,
                capacity.as_deref().unwrap_or("∞"),
                senders,
                receivers
            ));
        }
    }
    let effects = snapshot
        .split_once("\"effects\":")
        .and_then(|(_, tail)| tail.split_once("},").map(|(value, _)| value))
        .unwrap_or("{}");
    let resources = snapshot
        .split_once("\"resources\":")
        .map(|(_, tail)| tail.trim_end_matches('}'))
        .unwrap_or("{}");
    out.push_str(&format!("\neffects: {effects}}}\nresources: {resources}\n"));
    if let Ok(events) = jet_debug::render_event_observations(snapshot) {
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
