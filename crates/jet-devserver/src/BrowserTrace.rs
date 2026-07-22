//! D-PERF-BROWSER-TRANSPORT1=A: payload-free browser rows relayed by `jet dev`.

use std::collections::hash_map::RandomState;
use std::fs::{self, File, OpenOptions};
use std::hash::{BuildHasher, Hasher};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub const ROW_LIMIT: usize = 4096;
const ENVELOPE_LIMIT: usize = 512;
const SCHEMA: &str = "jet.browser.relay.v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub class: String,
    pub duration_ns: u64,
    pub start_ns: u64,
    pub symbol: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Capture {
    pub rows: Vec<Row>,
    pub source: String,
    pub truncated: bool,
}

pub struct Relay {
    file: Mutex<File>,
    nonce: String,
    path: PathBuf,
    rows: Mutex<usize>,
    started: Instant,
}

impl Relay {
    pub fn new(source: &str) -> Result<Self, String> {
        let path = relay_path(std::process::id());
        let _ = fs::remove_file(&path);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| format!("cannot create browser trace relay: {error}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
                .map_err(|error| format!("cannot secure browser trace relay: {error}"))?;
        }
        let nonce = nonce();
        writeln!(
            file,
            "{SCHEMA}\t{nonce}\t{}\t{}",
            hex(source.as_bytes()),
            process_start_marker(std::process::id()).unwrap_or_else(|| "unknown".into())
        )
        .map_err(|error| format!("cannot initialize browser trace relay: {error}"))?;
        file.flush()
            .map_err(|error| format!("cannot flush browser trace relay: {error}"))?;
        Ok(Self {
            file: Mutex::new(file),
            nonce,
            path,
            rows: Mutex::new(0),
            started: Instant::now(),
        })
    }

    pub fn nonce(&self) -> &str {
        &self.nonce
    }

    pub fn record(&self, body: &[u8]) -> Result<(), RecordError> {
        if body.len() > ENVELOPE_LIMIT {
            return Err(RecordError::Oversized);
        }
        let body = std::str::from_utf8(body).map_err(|_| RecordError::Malformed)?;
        let mut keys = body
            .split('&')
            .map(|part| part.split_once('=').map(|(key, _)| key).unwrap_or(part))
            .collect::<Vec<_>>();
        keys.sort_unstable();
        if keys != ["class", "clock_ns", "duration_ns", "start_ns", "symbol"] {
            return Err(RecordError::Malformed);
        }
        let param = |name| crate::query_param(&format!("?{body}"), name);
        let class = param("class").ok_or(RecordError::Malformed)?;
        let symbol = param("symbol").ok_or(RecordError::Malformed)?;
        if !matches!(class.as_str(), "event" | "wasm" | "dom")
            || symbol.is_empty()
            || symbol.len() > 128
            || !symbol.bytes().all(|b| b == b'_' || b.is_ascii_alphanumeric())
        {
            return Err(RecordError::Malformed);
        }
        let start = number(param("start_ns"))?;
        let duration = number(param("duration_ns"))?;
        let clock = number(param("clock_ns"))?;
        if start > clock || duration > clock - start {
            return Err(RecordError::Malformed);
        }
        let host_now = self
            .started
            .elapsed()
            .as_nanos()
            .min(u128::from(u64::MAX)) as u64;
        let mapped_start = host_now.saturating_sub(clock - start);
        let mut rows = self.rows.lock().map_err(|_| RecordError::Unavailable)?;
        let mut file = self.file.lock().map_err(|_| RecordError::Unavailable)?;
        if *rows >= ROW_LIMIT {
            if *rows == ROW_LIMIT {
                writeln!(file, "truncated").map_err(|_| RecordError::Unavailable)?;
                file.flush().map_err(|_| RecordError::Unavailable)?;
                *rows += 1;
            }
            return Ok(());
        }
        writeln!(file, "row\t{mapped_start}\t{duration}\t{class}\t{symbol}")
            .map_err(|_| RecordError::Unavailable)?;
        file.flush().map_err(|_| RecordError::Unavailable)?;
        *rows += 1;
        Ok(())
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordError {
    Malformed,
    Oversized,
    Unavailable,
}

pub fn read(pid: u32) -> Result<Capture, String> {
    let path = relay_path(pid);
    let link_metadata = fs::symlink_metadata(&path)
        .map_err(|_| format!("process {pid} has no browser trace relay"))?;
    if !link_metadata.file_type().is_file() {
        return Err("browser trace relay is not a regular file".into());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(0o400000);
    }
    let file = options
        .open(&path)
        .map_err(|_| "cannot securely open browser trace relay")?;
    let metadata = file
        .metadata()
        .map_err(|_| "cannot inspect browser trace relay")?;
    if !metadata.file_type().is_file() || metadata.len() > 1024 * 1024 {
        return Err("browser trace relay is not a bounded regular file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("browser trace relay permissions expose session rows".into());
        }
        #[cfg(any(target_os = "linux", target_os = "android"))]
        if let Ok(self_metadata) = fs::metadata("/proc/self") {
            if metadata.uid() != self_metadata.uid() {
                return Err("browser trace relay belongs to another user".into());
            }
        }
    }
    let mut raw = String::new();
    file.take(1024 * 1024 + 1)
        .read_to_string(&mut raw)
        .map_err(|_| "cannot read browser trace relay")?;
    if raw.len() > 1024 * 1024 {
        return Err("browser trace relay exceeds 1 MiB".into());
    }
    let mut lines = raw.lines();
    let header = lines.next().ok_or("browser trace relay is empty")?;
    let mut parts = header.split('\t');
    if parts.next() != Some(SCHEMA) {
        return Err("browser trace relay schema is not supported".into());
    }
    let _nonce = parts.next().ok_or("browser trace relay nonce is missing")?;
    let source = unhex(parts.next().ok_or("browser trace relay source is missing")?)?;
    let started = parts.next().ok_or("browser trace relay process identity is missing")?;
    if parts.next().is_some()
        || process_start_marker(pid).is_some_and(|current| started != "unknown" && current != started)
    {
        return Err("browser trace relay belongs to a stale process session".into());
    }
    let mut rows = Vec::new();
    let mut truncated = false;
    for line in lines {
        if line == "truncated" {
            truncated = true;
            continue;
        }
        let mut parts = line.split('\t');
        if parts.next() != Some("row") {
            return Err("browser trace relay row is malformed".into());
        }
        let start_ns = number(parts.next().map(str::to_string))
            .map_err(|_| "browser trace relay start is malformed")?;
        let duration_ns = number(parts.next().map(str::to_string))
            .map_err(|_| "browser trace relay duration is malformed")?;
        let class = parts
            .next()
            .ok_or("browser trace relay class is missing")?
            .to_string();
        let symbol = parts
            .next()
            .ok_or("browser trace relay symbol is missing")?
            .to_string();
        if parts.next().is_some() || rows.len() >= ROW_LIMIT {
            return Err("browser trace relay exceeds its closed row schema".into());
        }
        rows.push(Row {
            class,
            duration_ns,
            start_ns,
            symbol,
        });
    }
    Ok(Capture { rows, source, truncated })
}

pub fn relay_path(pid: u32) -> PathBuf {
    std::env::temp_dir().join(format!("jet-browser-trace-{pid}.relay"))
}

fn number(value: Option<String>) -> Result<u64, RecordError> {
    value
        .ok_or(RecordError::Malformed)?
        .parse()
        .map_err(|_| RecordError::Malformed)
}

fn nonce() -> String {
    let state = RandomState::new();
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut out = String::new();
    for domain in [0u64, 1] {
        let mut hasher = state.build_hasher();
        hasher.write_u128(seed);
        hasher.write_u32(std::process::id());
        hasher.write_u64(domain);
        out.push_str(&format!("{:016x}", hasher.finish()));
    }
    out
}

fn process_start_marker(pid: u32) -> Option<String> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let fields = stat.rsplit_once(") ")?.1.split_whitespace().collect::<Vec<_>>();
        fields.get(19).map(|value| (*value).to_string())
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let _ = pid;
        None
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Result<String, String> {
    if text.len() % 2 != 0 {
        return Err("browser trace relay source encoding is malformed".into());
    }
    let bytes = text
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let digit = |byte| match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                _ => None,
            };
            Some(digit(pair[0])? * 16 + digit(pair[1])?)
        })
        .collect::<Option<Vec<_>>>()
        .ok_or("browser trace relay source encoding is malformed")?;
    String::from_utf8(bytes).map_err(|_| "browser trace relay source is not UTF-8".into())
}

#[cfg(test)]
pub static TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_maps_payload_free_rows_and_rejects_extra_fields() {
        let _guard = TEST_LOCK.lock().unwrap();
        let relay = Relay::new("app.jet").unwrap();
        relay
            .record(b"class=event&symbol=load_report&start_ns=10&duration_ns=5&clock_ns=15")
            .unwrap();
        assert_eq!(
            relay.record(
                b"class=event&symbol=run&start_ns=1&duration_ns=1&clock_ns=2&url=secret"
            ),
            Err(RecordError::Malformed)
        );
        let capture = read(std::process::id()).unwrap();
        assert_eq!(capture.source, "app.jet");
        assert_eq!(capture.rows.len(), 1);
        assert_eq!(capture.rows[0].symbol, "load_report");
        assert!(!capture.truncated);
    }

    #[test]
    fn relay_caps_rows_and_records_truncation() {
        let _guard = TEST_LOCK.lock().unwrap();
        let relay = Relay::new("app.jet").unwrap();
        for _ in 0..=ROW_LIMIT {
            relay
                .record(b"class=dom&symbol=run&start_ns=1&duration_ns=1&clock_ns=2")
                .unwrap();
        }
        let capture = read(std::process::id()).unwrap();
        assert_eq!(capture.rows.len(), ROW_LIMIT);
        assert!(capture.truncated);
    }
}
