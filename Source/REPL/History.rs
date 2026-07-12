//! D-FE-REPL-HISTORY1=A persistent, private REPL submission history.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub const DEFAULT_LIMIT: usize = 2_000;

pub(crate) struct History {
    entries: Vec<String>,
    path: Option<PathBuf>,
    limit: usize,
}

impl History {
    pub(crate) fn session_only() -> Self {
        Self {
            entries: Vec::new(),
            path: None,
            limit: DEFAULT_LIMIT,
        }
    }

    pub(crate) fn open_from_env() -> (Self, Option<String>) {
        if std::env::var("JET_REPL_HISTORY").is_ok_and(|v| v.eq_ignore_ascii_case("off")) {
            return (Self::session_only(), None);
        }
        let (limit, limit_warning) = match std::env::var("JET_REPL_HISTORY_LIMIT") {
            Ok(raw) => match raw.parse::<usize>() {
                Ok(n) => (n, None),
                Err(_) => (DEFAULT_LIMIT, Some(format!(
                    "warning: JET_REPL_HISTORY_LIMIT={raw:?} is not a number; using {DEFAULT_LIMIT}"
                ))),
            },
            Err(_) => (DEFAULT_LIMIT, None),
        };
        let Some(path) = state_path() else {
            return (
                Self {
                    entries: Vec::new(),
                    path: None,
                    limit,
                },
                Some(
                    "warning: REPL history storage is unavailable; continuing with session-only history"
                        .into(),
                ),
            );
        };
        match Self::open(path, limit) {
            Ok((history, recovery_warning)) => {
                let warning = recovery_warning.or(limit_warning);
                (history, warning)
            }
            Err(error) => (
                Self {
                    entries: Vec::new(),
                    path: None,
                    limit,
                },
                Some(format!(
                    "warning: REPL history storage is unavailable ({error}); continuing with session-only history"
                )),
            ),
        }
    }

    fn open(path: PathBuf, limit: usize) -> io::Result<(Self, Option<String>)> {
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "history path has no parent")
        })?;
        private_dir(parent)?;
        let mut entries = Vec::new();
        let mut corrupt = false;
        if path.exists() {
            if fs::symlink_metadata(&path)?.file_type().is_symlink() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "history path is a symlink",
                ));
            }
            let bytes = fs::read(&path)?;
            let mut start = 0;
            while start < bytes.len() {
                let Some(relative_end) = bytes[start..].iter().position(|b| *b == b'\n') else {
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
        }
        let before_trim = entries.len();
        trim(&mut entries, limit);
        let history = Self {
            entries,
            path: Some(path),
            limit,
        };
        if corrupt || history.entries.len() != before_trim {
            history.rewrite()?;
        } else if history.path.as_ref().is_some_and(|p| p.exists()) {
            private_file(history.path.as_ref().unwrap())?;
        }
        Ok((
            history,
            corrupt.then(|| {
                "warning: corrupt history tail discarded; earlier REPL history was recovered"
                    .into()
            }),
        ))
    }

    pub(crate) fn entries(&self) -> &[String] {
        &self.entries
    }

    pub(crate) fn record(&mut self, input: &str) -> Option<String> {
        if self.limit == 0 {
            return None;
        }
        self.entries.push(input.to_string());
        trim(&mut self.entries, self.limit);
        let result = self.rewrite();
        if let Err(error) = result {
            self.path = None;
            return Some(format!(
                "warning: REPL history could not be saved ({error}); continuing with session-only history"
            ));
        }
        None
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
        self.entries.clear();
        if let Some(path) = &self.path {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    fn rewrite(&self) -> io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let parent = path.parent().unwrap();
        private_dir(parent)?;
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT_TMP: AtomicU64 = AtomicU64::new(0);
        let tmp = parent.join(format!(
            ".repl-history.{}.{}.tmp",
            std::process::id(),
            NEXT_TMP.fetch_add(1, Ordering::Relaxed)
        ));
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        for entry in &self.entries {
            file.write_all(encode(entry).as_bytes())?;
            file.write_all(b"\n")?;
        }
        file.sync_all()?;
        fs::rename(&tmp, path)?;
        private_file(path)
    }
}

fn trim(entries: &mut Vec<String>, limit: usize) {
    if entries.len() > limit {
        entries.drain(..entries.len() - limit);
    }
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

fn state_path() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_STATE_HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(dir).join("jet/repl-history"));
    }
    #[cfg(target_os = "windows")]
    if let Some(dir) = std::env::var_os("LOCALAPPDATA") {
        return Some(PathBuf::from(dir).join("Jet/repl-history"));
    }
    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME") {
        return Some(PathBuf::from(home).join("Library/Application Support/Jet/repl-history"));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state/jet/repl-history"))
}

fn private_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn private_file(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
