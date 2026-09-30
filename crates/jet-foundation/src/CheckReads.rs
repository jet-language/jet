//! #2517 S1b: the one audited reader for check-time inputs.
//!
//! Every file, directory listing, and environment variable that loading,
//! sema, or compile-time evaluation reads while checking a program goes
//! through this module. While a [`ReadSession`] is open, each read is
//! recorded as a [`CheckRead`]: a label naming what was read and the digest
//! of what the check saw. A persisted check result (the Receipt today, the
//! package check record next) declares these reads and re-verifies them
//! before reuse, so a result can never be replayed after an input it read
//! has changed. [`undeclared`] is the audit: it lists every recorded read a
//! record does not declare with the same digest.
//!
//! Labels are `file:<path>`, `dir:<path>`, and `env:<NAME>`. Paths are the
//! normalized paths the caller passed; callers that persist labels map them
//! to project-relative form. Digests are lowercase SHA-256 hex of the bytes
//! (a directory: of its sorted entry names joined by newlines; a variable: of
//! its value), or `missing` when the input does not exist.
//!
//! Sessions are process-wide because sema checks bodies on worker threads.
//! Overlapping sessions share one recording, which can only add declared
//! inputs, never lose one.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::SHA256::sha256_hex;

/// Digest recorded for an input that does not exist.
pub const MISSING: &str = "missing";

/// Fault injection for the read audit (#2517 criteria 2/6): when set, every
/// session opens by reading this path, a read no check key declares.
pub const INJECT_ENV: &str = "JET_CHECK_READS_INJECT";

/// One recorded check-time read.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CheckRead {
    pub label: String,
    pub digest: String,
}

#[derive(Default)]
struct Recorder {
    depth: usize,
    reads: BTreeMap<String, String>,
}

static RECORDER: Mutex<Recorder> = Mutex::new(Recorder {
    depth: 0,
    reads: BTreeMap::new(),
});

fn recorder() -> std::sync::MutexGuard<'static, Recorder> {
    RECORDER.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn record(label: String, digest: String) {
    let mut recorder = recorder();
    if recorder.depth == 0 {
        return;
    }
    // Two reads of one input that saw different contents cannot both be
    // declared; the conflict marker verifies against nothing.
    let entry = recorder.reads.entry(label).or_insert_with(|| digest.clone());
    if *entry != digest {
        *entry = "conflict".to_string();
    }
}

/// An open recording of check-time reads. Dropping it without calling
/// [`ReadSession::finish`] discards nothing for other open sessions.
pub struct ReadSession {
    finished: bool,
}

impl ReadSession {
    pub fn begin() -> Self {
        {
            let mut recorder = recorder();
            if recorder.depth == 0 {
                recorder.reads.clear();
            }
            recorder.depth += 1;
        }
        if let Some(path) = std::env::var_os(INJECT_ENV) {
            let _ = read(PathBuf::from(path));
        }
        Self { finished: false }
    }

    /// Close the session and return every read recorded while it was open,
    /// sorted by label.
    pub fn finish(mut self) -> Vec<CheckRead> {
        self.finished = true;
        let mut recorder = recorder();
        let reads = recorder
            .reads
            .iter()
            .map(|(label, digest)| CheckRead {
                label: label.clone(),
                digest: digest.clone(),
            })
            .collect();
        recorder.depth = recorder.depth.saturating_sub(1);
        reads
    }
}

impl Drop for ReadSession {
    fn drop(&mut self) {
        if !self.finished {
            let mut recorder = recorder();
            recorder.depth = recorder.depth.saturating_sub(1);
        }
    }
}

fn file_label(path: &Path) -> String {
    format!("file:{}", path.display())
}

fn dir_label(path: &Path) -> String {
    format!("dir:{}", path.display())
}

fn env_label(name: &str) -> String {
    format!("env:{name}")
}

fn digest_io<T>(result: &io::Result<T>, digest: impl FnOnce(&T) -> String) -> String {
    match result {
        Ok(value) => digest(value),
        Err(error) if error.kind() == io::ErrorKind::NotFound => MISSING.to_string(),
        Err(error) => format!("unreadable:{:?}", error.kind()),
    }
}

/// Read a whole file.
pub fn read(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
    let path = path.as_ref();
    let result = std::fs::read(path);
    record(file_label(path), digest_io(&result, |bytes| sha256_hex(bytes)));
    result
}

/// Read a file below `root` without following links (the embed and build
/// input law in `SHA256::read_file_nofollow_at_root`).
pub fn read_nofollow_at_root(root: &Path, relative: &Path, max_bytes: u64) -> io::Result<Vec<u8>> {
    let result = crate::SHA256::read_file_nofollow_at_root(root, relative, max_bytes);
    record(file_label(&root.join(relative)), digest_io(&result, |bytes| sha256_hex(bytes)));
    result
}

/// Read a whole UTF-8 file.
pub fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> {
    let bytes = read(path)?;
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// List a directory's entries, sorted by path.
pub fn read_dir_sorted(path: impl AsRef<Path>) -> io::Result<Vec<PathBuf>> {
    let path = path.as_ref();
    let result = std::fs::read_dir(path).and_then(|entries| {
        let mut paths = entries
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<io::Result<Vec<_>>>()?;
        paths.sort();
        Ok(paths)
    });
    record(dir_label(path), digest_io(&result, |paths| listing_digest(paths)));
    result
}

fn listing_digest(paths: &[PathBuf]) -> String {
    let names = paths
        .iter()
        .map(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();
    sha256_hex(names.join("\n").as_bytes())
}

/// Read an environment variable as text. A variable that is set but not
/// valid Unicode reads as unset, as `std::env::var(..).ok()` does.
pub fn env_var(name: &str) -> Option<String> {
    env_var_os(name).and_then(|value| value.into_string().ok())
}

/// Read an environment variable.
pub fn env_var_os(name: &str) -> Option<OsString> {
    let value = std::env::var_os(name);
    record(env_label(name), env_digest(value.as_ref()));
    value
}

fn env_digest(value: Option<&OsString>) -> String {
    value.map_or_else(
        || MISSING.to_string(),
        |value| sha256_hex(value.to_string_lossy().as_bytes()),
    )
}

/// Recompute the digest a label would record now, without recording it.
/// Used to verify a declared read before reusing a result.
pub fn current_digest(label: &str) -> Option<String> {
    if let Some(path) = label.strip_prefix("file:") {
        let result = std::fs::read(path);
        return Some(digest_io(&result, |bytes| sha256_hex(bytes)));
    }
    if let Some(path) = label.strip_prefix("dir:") {
        let result = std::fs::read_dir(path).and_then(|entries| {
            let mut paths = entries
                .map(|entry| entry.map(|entry| entry.path()))
                .collect::<io::Result<Vec<_>>>()?;
            paths.sort();
            Ok(paths)
        });
        return Some(digest_io(&result, |paths| listing_digest(paths)));
    }
    label
        .strip_prefix("env:")
        .map(|name| env_digest(std::env::var_os(name).as_ref()))
}

/// The audit: every recorded read that `declared` (label to digest) does not
/// carry with the same digest.
pub fn undeclared(reads: &[CheckRead], declared: &BTreeMap<String, String>) -> Vec<CheckRead> {
    reads
        .iter()
        .filter(|read| declared.get(&read.label) != Some(&read.digest))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_records_reads_and_audit_reports_undeclared_ones() {
        let root = std::env::temp_dir().join(format!("jet-check-reads-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let declared_file = root.join("declared.txt");
        let injected_file = root.join("injected.txt");
        std::fs::write(&declared_file, b"one").unwrap();
        std::fs::write(&injected_file, b"two").unwrap();

        let session = ReadSession::begin();
        assert_eq!(read(&declared_file).unwrap(), b"one");
        assert!(read(root.join("absent.txt")).is_err());
        let reads = session.finish();
        let declared = reads
            .iter()
            .map(|read| (read.label.clone(), read.digest.clone()))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(declared[&format!("file:{}", root.join("absent.txt").display())], MISSING);
        assert!(undeclared(&reads, &declared).is_empty());

        // An injected read the record does not declare fails the audit.
        let session = ReadSession::begin();
        read(&declared_file).unwrap();
        read(&injected_file).unwrap();
        let reads = session.finish();
        let missing = undeclared(&reads, &declared);
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].label, format!("file:{}", injected_file.display()));

        // A declared input whose bytes changed no longer verifies.
        std::fs::write(&declared_file, b"changed").unwrap();
        let label = format!("file:{}", declared_file.display());
        assert_ne!(current_digest(&label).as_ref(), declared.get(&label));

        // Reads outside a session are not recorded.
        read(&declared_file).unwrap();
        let session = ReadSession::begin();
        assert!(session.finish().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
