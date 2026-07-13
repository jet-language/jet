//! D-FE-REPL-HISTORY1=A persistent, private REPL submission history.

use std::io;
use std::path::PathBuf;

#[path = "HistoryPlatform.rs"]
mod platform;
use platform::Backend;

pub const DEFAULT_LIMIT: usize = 2_000;

pub(crate) struct History {
    entries: Vec<String>,
    backend: Option<Backend>,
    limit: usize,
}

impl History {
    pub(crate) fn session_only() -> Self {
        Self {
            entries: Vec::new(),
            backend: None,
            limit: DEFAULT_LIMIT,
        }
    }

    pub(crate) fn open_from_env() -> (Self, Option<String>) {
        if std::env::var("JET_REPL_HISTORY").is_ok_and(|v| v.eq_ignore_ascii_case("off")) {
            return (Self::session_only(), None);
        }
        let (limit, mut warnings) = match std::env::var("JET_REPL_HISTORY_LIMIT") {
            Ok(raw) => match raw.parse::<usize>() {
                Ok(n) => (n, Vec::new()),
                Err(_) => (
                    DEFAULT_LIMIT,
                    vec![format!(
                        "warning: JET_REPL_HISTORY_LIMIT={raw:?} is not a number; using {DEFAULT_LIMIT}"
                    )],
                ),
            },
            Err(_) => (DEFAULT_LIMIT, Vec::new()),
        };
        let Some(root) = state_root() else {
            return (Self::fallback(limit), Some(storage_fallback("no platform state directory")));
        };
        let backend = match Backend::open(&root) {
            Ok(backend) => backend,
            Err(error) => return (Self::fallback(limit), Some(storage_fallback(&error.to_string()))),
        };
        let guard = match backend.lock() {
            Ok(guard) => guard,
            Err(error) => return (Self::fallback(limit), Some(storage_fallback(&error.to_string()))),
        };
        let (mut entries, corrupt) = match load_entries(&backend) {
            Ok(loaded) => loaded,
            Err(error) => return (Self::fallback(limit), Some(storage_fallback(&error.to_string()))),
        };
        let before_trim = entries.len();
        trim(&mut entries, limit);
        if corrupt || entries.len() != before_trim {
            if let Err(error) = backend.rewrite(&entries) {
                return (Self::fallback(limit), Some(storage_fallback(&error.to_string())));
            }
        }
        drop(guard);
        if corrupt {
            warnings.push(
                "warning: corrupt history tail discarded; earlier REPL history was recovered"
                    .into(),
            );
        }
        (
            Self {
                entries,
                backend: Some(backend),
                limit,
            },
            (!warnings.is_empty()).then(|| warnings.join("\n")),
        )
    }

    fn fallback(limit: usize) -> Self {
        Self {
            entries: Vec::new(),
            backend: None,
            limit,
        }
    }

    pub(crate) fn entries(&self) -> &[String] {
        &self.entries
    }

    pub(crate) fn record(&mut self, input: &str) -> Option<String> {
        if self.limit == 0 {
            return None;
        }
        let Some(backend) = self.backend.as_ref() else {
            self.entries.push(input.to_string());
            trim(&mut self.entries, self.limit);
            return None;
        };
        let transaction = (|| -> io::Result<(Vec<String>, bool)> {
            let _guard = backend.lock()?;
            let (mut entries, corrupt) = load_entries(backend)?;
            entries.push(input.to_string());
            trim(&mut entries, self.limit);
            backend.rewrite(&entries)?;
            Ok((entries, corrupt))
        })();
        match transaction {
            Ok((entries, corrupt)) => {
                self.entries = entries;
                corrupt.then(|| {
                    "warning: corrupt history tail discarded while saving; earlier REPL history was recovered"
                        .into()
                })
            }
            Err(error) => {
                self.entries.push(input.to_string());
                trim(&mut self.entries, self.limit);
                self.backend = None;
                Some(storage_fallback(&error.to_string()))
            }
        }
    }

    pub(crate) fn search(&self, needle: &str) -> Vec<&str> {
        self.entries
            .iter()
            .rev()
            .filter(|entry| entry.contains(needle))
            .map(String::as_str)
            .collect()
    }

    pub(crate) fn clear(&mut self) -> io::Result<()> {
        if let Some(backend) = self.backend.as_ref() {
            let _guard = backend.lock()?;
            // Re-read while serialized so clear participates in same
            // transaction order as writers. Unlink then directory-sync makes
            // successful clear durable before lock release.
            let _ = load_entries(backend)?;
            backend.clear()?;
        }
        self.entries.clear();
        Ok(())
    }
}

fn storage_fallback(detail: &str) -> String {
    format!(
        "warning: REPL history storage is unavailable ({detail}); continuing with session-only history"
    )
}

fn load_entries(backend: &Backend) -> io::Result<(Vec<String>, bool)> {
    let Some(bytes) = backend.read()? else {
        return Ok((Vec::new(), false));
    };
    let mut entries = Vec::new();
    let mut corrupt = false;
    let mut start = 0;
    while start < bytes.len() {
        let Some(relative_end) = bytes[start..].iter().position(|byte| *byte == b'\n') else {
            corrupt = true;
            break;
        };
        let end = start + relative_end;
        match decode(&bytes[start..end]) {
            Some(entry) => entries.push(entry),
            None => {
                corrupt = true;
                break;
            }
        }
        start = end + 1;
    }
    Ok((entries, corrupt))
}

fn trim(entries: &mut Vec<String>, limit: usize) {
    if entries.len() > limit {
        entries.drain(..entries.len() - limit);
    }
}

fn render(entries: &[String]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for entry in entries {
        bytes.extend_from_slice(encode(entry).as_bytes());
        bytes.push(b'\n');
    }
    bytes
}

fn encode(text: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(text.len() * 2);
    for byte in text.as_bytes() {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 15) as usize] as char);
    }
    out
}

fn decode(bytes: &[u8]) -> Option<String> {
    if bytes.len() % 2 != 0 {
        return None;
    }
    let mut decoded = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        decoded.push((hex(pair[0])? << 4) | hex(pair[1])?);
    }
    String::from_utf8(decoded).ok()
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn state_root() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()) {
        let dir = PathBuf::from(dir);
        return dir.is_absolute().then_some(dir);
    }
    #[cfg(target_os = "windows")]
    if let Some(dir) = std::env::var_os("LOCALAPPDATA") {
        return Some(PathBuf::from(dir));
    }
    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME") {
        return Some(PathBuf::from(home).join("Library/Application Support"));
    }
    std::env::var_os("HOME").and_then(|home| {
        let home = PathBuf::from(home);
        home.is_absolute().then(|| home.join(".local/state"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "jet_repl_history_unit_{tag}_{}_{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("thread")
        ))
    }

    #[test]
    fn atomic_replacement_supports_repeated_writes() {
        let root = root("replace");
        std::fs::remove_dir_all(&root).ok();
        let backend = Backend::open(&root).unwrap();
        let _guard = backend.lock().unwrap();
        backend.rewrite(&["first".into()]).unwrap();
        backend.rewrite(&["second".into()]).unwrap();
        let (entries, corrupt) = load_entries(&backend).unwrap();
        assert_eq!(entries, ["second"]);
        assert!(!corrupt);
        drop(_guard);
        drop(backend);
        std::fs::remove_dir_all(root).ok();
    }

    #[cfg(unix)]
    #[test]
    fn transaction_lock_times_out_then_recovers_after_release() {
        let root = root("lock");
        std::fs::remove_dir_all(&root).ok();
        let first = Backend::open(&root).unwrap();
        let second = Backend::open(&root).unwrap();
        let held = first.lock().unwrap();
        let started = std::time::Instant::now();
        let error = match second.lock() {
            Ok(_) => panic!("second store acquired held history lock"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < std::time::Duration::from_secs(3));
        drop(held);
        let recovered = second.lock().unwrap();
        drop(recovered);
        drop(first);
        drop(second);
        std::fs::remove_dir_all(root).ok();
    }
}
