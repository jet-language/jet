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
const REQUEST_SCHEMA: &str = "jet.browser.request.v1";

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
    pub sources: Vec<Source>,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source { pub path: String, pub sha256: String, pub symbols: Vec<(String, String)> }

pub struct Relay {
    file: Mutex<File>,
    nonce: String,
    path: PathBuf,
    rows: Mutex<usize>,
    started: Instant,
}

impl Relay {
    pub fn new(manifest: &str) -> Result<Self, String> {
        supported()?;
        let sources = sources_from_manifest(manifest)?;
        let path = relay_path(std::process::id());
        let _ = fs::remove_file(&path);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&path)
            .map_err(|error| format!("cannot create browser trace relay: {error}"))?;
        let nonce = nonce();
        let pid = std::process::id();
        let started = process_start_marker(pid).ok_or("browser trace process identity cannot be verified")?;
        writeln!(
            file,
            "{SCHEMA}\t{nonce}\t{pid}\t{started}\t{}",
            hex(encode_sources(&sources).as_bytes())
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
            || !symbol
                .bytes()
                .all(|b| matches!(b, b'_' | b'$') || b.is_ascii_alphanumeric())
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
    supported()?;
    read_with_process_state(pid, process_state(pid))
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ProcessState {
    Alive(String),
    Dead,
    Unknown,
}

fn read_with_process_state(pid: u32, process: ProcessState) -> Result<Capture, String> {
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
    let recorded_pid = parts
        .next()
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or("browser trace relay process id is missing")?;
    let started = parts.next().ok_or("browser trace relay process identity is missing")?;
    let sources = decode_sources(&unhex(parts.next().ok_or("browser trace relay source map is missing")?)?)?;
    if matches!(process, ProcessState::Unknown) {
        return Err("browser trace process identity cannot be verified".into());
    }
    let stale = recorded_pid != pid
        || parts.next().is_some()
        || matches!(&process, ProcessState::Dead)
        || matches!(&process, ProcessState::Alive(current) if current != started);
    if stale {
        let _ = fs::remove_file(&path);
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
    Ok(Capture { rows, sources, truncated })
}

pub fn relay_path(pid: u32) -> PathBuf {
    std::env::temp_dir().join(format!("jet-browser-trace-{pid}.relay"))
}

pub fn request_path(pid: u32) -> PathBuf { std::env::temp_dir().join(format!("jet-browser-trace-{pid}.request")) }

pub fn request(pid: u32) -> Result<(), String> {
    supported()?;
    let ProcessState::Alive(started) = process_state(pid) else { return Err(format!("process {pid} identity cannot be verified")) };
    let path = request_path(pid);
    let _ = fs::remove_file(&path);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
    let mut file = options.open(&path).map_err(|error| format!("cannot request browser trace collection: {error}"))?;
    writeln!(file, "{REQUEST_SCHEMA}\t{pid}\t{started}").map_err(|error| format!("cannot write browser trace request: {error}"))?;
    file.flush().map_err(|error| format!("cannot flush browser trace request: {error}"))
}

pub fn take_request() -> Result<bool, String> {
    supported()?;
    let pid = std::process::id();
    let path = request_path(pid);
    if !path.exists() { return Ok(false) }
    let raw = secure_read(&path, 256, "browser trace request")?;
    let mut parts = raw.trim_end().split('\t');
    let current = process_start_marker(pid).ok_or("browser trace process identity cannot be verified")?;
    let pid_text = pid.to_string();
    let valid = parts.next() == Some(REQUEST_SCHEMA) && parts.next() == Some(pid_text.as_str()) && parts.next() == Some(current.as_str()) && parts.next().is_none();
    let _ = fs::remove_file(&path);
    if !valid { return Err("browser trace request belongs to a stale process session".into()) }
    Ok(true)
}

pub fn cancel_request(pid: u32) { let _ = fs::remove_file(request_path(pid)); }

fn supported() -> Result<(), String> {
    #[cfg(any(target_os = "linux", target_os = "android"))] { Ok(()) }
    #[cfg(not(any(target_os = "linux", target_os = "android")))] { Err("browser trace relay requires verified process and file ownership".into()) }
}

fn secure_read(path: &PathBuf, limit: u64, label: &str) -> Result<String, String> {
    let link = fs::symlink_metadata(path).map_err(|_| format!("{label} is unavailable"))?;
    if !link.file_type().is_file() { return Err(format!("{label} is not a regular file")) }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(any(target_os = "linux", target_os = "android"))] { use std::os::unix::fs::OpenOptionsExt; options.custom_flags(0o400000); }
    let file = options.open(path).map_err(|_| format!("cannot securely open {label}"))?;
    let metadata = file.metadata().map_err(|_| format!("cannot inspect {label}"))?;
    #[cfg(unix)] {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.permissions().mode() & 0o077 != 0 { return Err(format!("{label} permissions expose session data")) }
        let self_uid = fs::metadata("/proc/self").map_err(|_| "cannot verify browser trace owner".to_string())?.uid();
        if metadata.uid() != self_uid { return Err(format!("{label} belongs to another user")) }
    }
    if !metadata.is_file() || metadata.len() > limit { return Err(format!("{label} is not bounded")) }
    let mut raw = String::new();
    file.take(limit + 1).read_to_string(&mut raw).map_err(|_| format!("cannot read {label}"))?;
    if raw.len() as u64 > limit { return Err(format!("{label} exceeds its limit")) }
    Ok(raw)
}

pub fn sources_from_manifest(manifest: &str) -> Result<Vec<Source>, String> {
    let key = "\"traceMap\": \"";
    let start = manifest.find(key).map(|index| index + key.len()).ok_or("web manifest has no compiler trace map")?;
    let end = manifest[start..].find('"').map(|index| start + index).ok_or("web manifest trace map is malformed")?;
    decode_sources(&unhex(&manifest[start..end])?)
}

fn encode_sources(sources: &[Source]) -> String {
    let mut lines = Vec::new();
    for source in sources {
        lines.push(format!("source\t{}\t{}", hex(source.path.as_bytes()), source.sha256));
        for (name, kind) in &source.symbols { lines.push(format!("symbol\t{}\t{}\t{kind}", hex(source.path.as_bytes()), hex(name.as_bytes()))); }
    }
    lines.sort(); lines.join("\n")
}

fn decode_sources(text: &str) -> Result<Vec<Source>, String> {
    let mut sources = Vec::<Source>::new();
    let mut symbols = Vec::<(String, String, String)>::new();
    for line in text.lines() {
        let mut parts = line.split('\t');
        match parts.next() {
            Some("source") => {
                let path = unhex(parts.next().ok_or("trace source path is missing")?)?;
                let sha256 = parts.next().ok_or("trace source hash is missing")?.to_string();
                if parts.next().is_some() || sha256.len() != 64 || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) { return Err("compiler trace source identity is malformed".into()) }
                sources.push(Source { path, sha256, symbols: Vec::new() });
            }
            Some("symbol") => {
                let path = unhex(parts.next().ok_or("trace symbol path is missing")?)?;
                let name = unhex(parts.next().ok_or("trace symbol name is missing")?)?;
                let kind = parts.next().ok_or("trace symbol kind is missing")?.to_string();
                if parts.next().is_some() || name.is_empty() || !matches!(kind.as_str(), "fn" | "handler") { return Err("compiler trace symbol identity is malformed".into()) }
                symbols.push((path, name, kind));
            }
            _ => return Err("compiler trace map row is malformed".into()),
        }
    }
    if sources.is_empty() { return Err("compiler trace map has no sources".into()) }
    for (path, name, kind) in symbols {
        if sources.iter().any(|source| source.symbols.iter().any(|(existing, _)| existing == &name)) { return Err(format!("compiler trace symbol `{name}` is ambiguous")) }
        let source = sources.iter_mut().find(|source| source.path == path).ok_or("compiler trace symbol names an unknown source")?;
        source.symbols.push((name, kind));
    }
    Ok(sources)
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

fn process_state(pid: u32) -> ProcessState {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let path = PathBuf::from(format!("/proc/{pid}"));
        if !path.exists() {
            return ProcessState::Dead;
        }
        return process_start_marker(pid)
            .map(ProcessState::Alive)
            .unwrap_or(ProcessState::Unknown);
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let _ = pid;
        ProcessState::Unknown
    }
}

pub(crate) fn hex(bytes: &[u8]) -> String {
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

    fn manifest() -> String {
        let map = "source\t6170702e6a6574\taaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\nsymbol\t6170702e6a6574\t6c6f61645f7265706f7274\tfn";
        format!("{{\n  \"traceMap\": \"{}\"\n}}\n", hex(map.as_bytes()))
    }

    #[test]
    fn relay_maps_payload_free_rows_and_rejects_extra_fields() {
        let _guard = TEST_LOCK.lock().unwrap();
        let relay = Relay::new(&manifest()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(relay_path(std::process::id()))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "relay mode must be atomic owner-only");
        }
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
        assert_eq!(capture.sources[0].path, "app.jet");
        assert_eq!(capture.rows.len(), 1);
        assert_eq!(capture.rows[0].symbol, "load_report");
        assert!(!capture.truncated);
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn dead_devserver_relay_is_rejected_and_removed() {
        let _guard = TEST_LOCK.lock().unwrap();
        let relay = Relay::new(&manifest()).unwrap();
        let path = relay_path(std::process::id());
        let error = read_with_process_state(std::process::id(), ProcessState::Dead).unwrap_err();
        assert!(error.contains("stale process session"), "{error}");
        assert!(!path.exists(), "dead devserver relay was not removed");
        drop(relay);
    }

    #[test]
    fn unknown_process_identity_fails_closed_without_consuming_relay() {
        let _guard = TEST_LOCK.lock().unwrap();
        let relay = Relay::new(&manifest()).unwrap();
        let path = relay_path(std::process::id());
        let error = read_with_process_state(std::process::id(), ProcessState::Unknown).unwrap_err();
        assert!(error.contains("cannot be verified"), "{error}");
        assert!(path.exists(), "unknown identity must not mutate relay ownership");
        drop(relay);
    }

    #[test]
    fn relay_caps_rows_and_records_truncation() {
        let _guard = TEST_LOCK.lock().unwrap();
        let relay = Relay::new(&manifest()).unwrap();
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
