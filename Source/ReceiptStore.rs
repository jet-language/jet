//! Content-addressed receipts for deterministic development acts.
//!
//! D-DEVR-TWICE1=A: `check`, `build`, `test`, `prove`, and `budget check`
//! consult one local receipt store before doing work. Receipt identity uses
//! input bytes and invocation context; filesystem timestamps never participate.

use crate::SHA256::sha256_hex;
use jet_devserver::WatchService::WatchGraph;
use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const MAGIC: &[u8] = b"jet-receipt-v1\0";
const DIGEST_LEN: usize = 64;
const MAX_FIELD: u64 = 64 * 1024 * 1024;
static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

/// Whole-invocation receipt participants are deterministic verdict/build acts.
/// `run`/`dev` already reuse their compile actions through the tier caches;
/// mutation and interactive verbs do not claim a whole-invocation receipt.
pub const PARTICIPATING_VERBS: &[&str] = &["check", "build", "test", "prove", "budget check"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceiptInput {
    pub path: PathBuf,
    pub digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceiptClaim {
    pub verb: String,
    pub key: String,
    pub inputs: Vec<ReceiptInput>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Receipt {
    pub claim: ReceiptClaim,
    pub status: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub digest: String,
}

pub struct ReceiptStore {
    root: PathBuf,
}

impl ReceiptStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Build an action identity from argv, current tool/environment context,
    /// and the exact input files supplied by the caller.
    pub fn claim(
        &self,
        verb: &str,
        argv: &[String],
        input_paths: &[PathBuf],
    ) -> Result<ReceiptClaim, String> {
        if verb.is_empty() {
            return Err("receipt verb is empty".into());
        }
        let mut inputs = Vec::new();
        let mut seen = BTreeSet::new();
        for path in input_paths {
            let path = canonical_path(path)?;
            if seen.insert(path.clone()) {
                inputs.push(ReceiptInput {
                    digest: file_digest(&path)?,
                    path,
                });
            }
        }
        inputs.sort_by(|a, b| a.path.cmp(&b.path));

        let mut identity = Vec::new();
        identity.extend_from_slice(MAGIC);
        frame(&mut identity, verb.as_bytes());
        frame(&mut identity, &current_dir_bytes());
        frame(&mut identity, &argv_identity(argv));
        frame(&mut identity, &environment_identity());
        frame(&mut identity, &tool_identity());
        frame(&mut identity, &terminal_identity());
        for input in &inputs {
            frame(&mut identity, input.path.to_string_lossy().as_bytes());
            frame(&mut identity, input.digest.as_bytes());
        }

        Ok(ReceiptClaim {
            verb: verb.to_string(),
            key: sha256_hex(&identity),
            inputs,
        })
    }

    /// Return a receipt only when the stored bytes and every current input
    /// still match the claim. A malformed or stale object is a cache miss.
    pub fn lookup(&self, claim: &ReceiptClaim) -> Result<Option<Receipt>, String> {
        if !is_digest(&claim.key) || !inputs_current(&claim.inputs) {
            return Ok(None);
        }
        let path = self.object_path(&claim.key);
        let bytes = match read_regular(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(format!(
                    "could not read receipt {}: {error}",
                    path.display()
                ))
            }
        };
        let receipt = match decode_receipt(&bytes) {
            Ok(receipt) => receipt,
            Err(_) => return Ok(None),
        };
        if receipt.claim != *claim || receipt.digest != receipt_digest(&receipt) {
            return Ok(None);
        }
        Ok(Some(receipt))
    }

    /// Publish one immutable receipt object. `true` means a new object was
    /// created; `false` means the exact object already existed.
    pub fn write(
        &self,
        claim: &ReceiptClaim,
        status: i32,
        stdout: &[u8],
        stderr: &[u8],
    ) -> Result<bool, String> {
        if !is_digest(&claim.key) {
            return Err("receipt claim key is not a lowercase SHA-256 digest".into());
        }
        if !inputs_current(&claim.inputs) {
            return Ok(false);
        }
        let receipt = Receipt {
            claim: claim.clone(),
            status,
            stdout: stdout.to_vec(),
            stderr: stderr.to_vec(),
            digest: String::new(),
        };
        let digest = receipt_digest(&receipt);
        let receipt = Receipt { digest, ..receipt };
        let bytes = encode_receipt(&receipt)?;
        let path = self.object_path(&claim.key);
        let parent = path
            .parent()
            .ok_or_else(|| format!("receipt path has no parent: {}", path.display()))?;
        secure_create_dir(parent)?;

        let temp = parent.join(format!(
            ".{}.{}.{}",
            claim.key,
            std::process::id(),
            NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
                .map_err(|error| format!("could not stage receipt: {error}"))?;
            file.write_all(&bytes)
                .map_err(|error| format!("could not write receipt: {error}"))?;
            file.sync_all()
                .map_err(|error| format!("could not flush receipt: {error}"))?;
            match fs::hard_link(&temp, &path) {
                Ok(()) => {
                    fs::remove_file(&temp)
                        .map_err(|error| format!("could not remove staged receipt: {error}"))?;
                    Ok(true)
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let existing = read_regular(&path)
                        .map_err(|read_error| format!("could not inspect receipt: {read_error}"))?;
                    let _ = fs::remove_file(&temp);
                    if existing == bytes {
                        Ok(false)
                    } else {
                        Err("receipt key collision with different content".into())
                    }
                }
                Err(error) => Err(format!("could not publish receipt: {error}")),
            }
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }

    pub fn object_path(&self, key: &str) -> PathBuf {
        self.root.join("objects").join(key)
    }
}

/// Resolve a CLI invocation's source closure. Direct source files use the
/// compiler's import graph; package-level test/budget actions use the whole
/// package tree because their public operation reads every member.
pub fn input_paths_for(verb: &str, argv: &[String], cwd: &Path) -> Vec<PathBuf> {
    let Some(target) = target_path(verb, argv, cwd) else {
        return Vec::new();
    };
    let mut paths = BTreeSet::new();
    if target.is_dir() || verb == "budget check" {
        collect_tree_inputs(&target, verb, &mut paths);
    } else if target.is_file() {
        match WatchGraph::discover(&target) {
            Ok(graph) => paths.extend(graph.watched_paths()),
            Err(_) => {
                paths.insert(target.clone());
            }
        }
        add_project_inputs(&target, &mut paths);
    } else {
        return Vec::new();
    }
    paths
        .into_iter()
        .filter(|path| regular_file(path))
        .collect()
}

/// Run a cacheable CLI act in a child process, then publish its observed
/// output as one receipt. Returning `Some` means the caller must exit with the
/// supplied status; `None` leaves the normal dispatcher untouched.
pub fn run_if_needed(argv: &[String]) -> Option<i32> {
    if std::env::var_os("JET_RECEIPT_BYPASS").is_some() {
        return None;
    }
    let verb = participating_verb(argv)?;
    if !cacheable_invocation(verb, argv) {
        return None;
    }
    let cwd = std::env::current_dir().ok()?;
    let input_paths = input_paths_for(verb, argv, &cwd);
    if input_paths.is_empty() && verb != "budget check" {
        return None;
    }
    let root = receipt_root(verb, argv, &cwd);
    let store = ReceiptStore::new(root);
    let claim = store.claim(verb, argv, &input_paths).ok()?;
    if let Ok(Some(receipt)) = store.lookup(&claim) {
        replay_receipt(verb, &receipt);
        return Some(receipt.status);
    }

    let executable = std::env::current_exe().ok()?;
    let output = std::process::Command::new(executable)
        .args(argv)
        .current_dir(&cwd)
        .env("JET_RECEIPT_BYPASS", "1")
        .output()
        .ok()?;
    let status = output.status.code().unwrap_or(1);
    write_bytes(std::io::stdout(), &output.stdout);
    write_bytes(std::io::stderr(), &output.stderr);
    let _ = store.write(&claim, status, &output.stdout, &output.stderr);
    Some(status)
}

pub fn participating_verb(argv: &[String]) -> Option<&'static str> {
    match argv.first().map(String::as_str) {
        Some("check") => Some("check"),
        Some("build") => Some("build"),
        Some("test") => Some("test"),
        Some("prove") => Some("prove"),
        Some("budget") if argv.get(1).map(String::as_str) == Some("check") => Some("budget check"),
        _ => None,
    }
}

fn cacheable_invocation(verb: &str, argv: &[String]) -> bool {
    if argv.iter().any(|arg| arg == "--record")
        || argv.iter().any(|arg| arg.starts_with("--record="))
    {
        return false;
    }
    if verb == "test" && argv.iter().any(|arg| arg == "--shuffle") {
        return false;
    }
    if verb == "prove"
        && argv.iter().any(|arg| {
            matches!(
                arg.as_str(),
                "--capture" | "--capture-sensitive" | "--replay"
            ) || arg.starts_with("--capture=")
                || arg.starts_with("--capture-sensitive=")
                || arg.starts_with("--replay=")
        })
    {
        return false;
    }
    true
}

fn receipt_root(verb: &str, argv: &[String], cwd: &Path) -> PathBuf {
    if let Ok(root) = std::env::var("JET_RECEIPT_DIR") {
        return PathBuf::from(root);
    }
    let target = target_path(verb, argv, cwd);
    let base = target
        .as_deref()
        .and_then(|path| {
            let start = if path.is_dir() {
                path
            } else {
                path.parent().unwrap_or(cwd)
            };
            crate::Loader::find_manifest_root(start).or_else(|| Some(start.to_path_buf()))
        })
        .unwrap_or_else(|| cwd.to_path_buf());
    base.join(".jet").join("receipts")
}

fn target_path(verb: &str, argv: &[String], cwd: &Path) -> Option<PathBuf> {
    let mut skip_next = false;
    let mut positionals = Vec::new();
    let start = if verb == "budget check" { 2 } else { 1 };
    for arg in argv.iter().skip(start) {
        if skip_next {
            skip_next = false;
            continue;
        }
        if matches!(
            arg.as_str(),
            "-p" | "--project"
                | "--output"
                | "--target"
                | "--profile"
                | "--builder"
                | "--filter"
                | "--edition"
                | "--scope"
                | "--kind"
                | "--set"
                | "--port"
                | "--seed"
                | "--iterations"
                | "--time"
                | "--corpus"
        ) {
            skip_next = true;
            continue;
        }
        if arg == "--" {
            break;
        }
        if !arg.starts_with('-') {
            positionals.push(arg);
        }
    }
    let Some(candidate) = positionals.first().map(|value| cwd.join(value.as_str())) else {
        let root = crate::Loader::find_manifest_root(cwd).unwrap_or_else(|| cwd.to_path_buf());
        if matches!(verb, "test" | "budget check") {
            return Some(root);
        }
        for entry in [
            root.join(crate::Syntax::DEFAULT_ENTRY_FILE),
            root.join("src").join(crate::Syntax::DEFAULT_ENTRY_FILE),
        ] {
            if regular_file(&entry) {
                return Some(entry);
            }
        }
        return Some(root);
    };
    if candidate.exists() {
        return Some(candidate);
    }
    if candidate
        .extension()
        .is_some_and(|ext| ext == crate::Syntax::FILE_EXT)
    {
        return Some(candidate);
    }
    None
}

fn collect_tree_inputs(root: &Path, verb: &str, out: &mut BTreeSet<PathBuf>) {
    if root.is_file() {
        out.insert(root.to_path_buf());
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if path.is_dir() {
            if matches!(name, ".git" | "target" | "build") {
                continue;
            }
            if name == ".jet" {
                let lock = path.join("lock");
                if regular_file(&lock) {
                    out.insert(lock);
                }
                if verb == "budget check" {
                    collect_tree_inputs(&path.join("perf").join("baselines"), verb, out);
                }
                continue;
            }
            let under_jet = path
                .components()
                .any(|component| component.as_os_str() == ".jet");
            if verb == "budget check" && under_jet && matches!(name, "locks" | "reports") {
                continue;
            }
            collect_tree_inputs(&path, verb, out);
            continue;
        }
        let is_source = path
            .extension()
            .is_some_and(|ext| ext == crate::Syntax::FILE_EXT)
            || matches!(name, crate::Syntax::PACKAGE_FILE | "workspace.jet" | "lock");
        let is_budget_input = verb == "budget check"
            && path
                .components()
                .any(|component| component.as_os_str() == ".jet");
        if is_source || is_budget_input {
            out.insert(path);
        }
    }
}

fn add_project_inputs(entry: &Path, out: &mut BTreeSet<PathBuf>) {
    let start = entry.parent().unwrap_or_else(|| Path::new("."));
    let Some(root) = crate::Loader::find_manifest_root(start) else {
        return;
    };
    for name in [crate::Syntax::PACKAGE_FILE, "workspace.jet"] {
        let path = root.join(name);
        if regular_file(&path) {
            out.insert(path);
        }
    }
    let lock = root.join(".jet").join("lock");
    if regular_file(&lock) {
        out.insert(lock);
    }
}

fn canonical_path(path: &Path) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("could not inspect input {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "receipt input is not a regular file: {}",
            path.display()
        ));
    }
    fs::canonicalize(path)
        .map_err(|error| format!("could not canonicalize input {}: {error}", path.display()))
}

fn file_digest(path: &Path) -> Result<String, String> {
    let _ = canonical_path(path)?;
    let bytes = fs::read(path)
        .map_err(|error| format!("could not read input {}: {error}", path.display()))?;
    Ok(sha256_hex(&bytes))
}

fn inputs_current(inputs: &[ReceiptInput]) -> bool {
    inputs
        .iter()
        .all(|input| file_digest(&input.path).is_ok_and(|digest| digest == input.digest))
}

fn regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        .unwrap_or(false)
}

fn secure_create_dir(path: &Path) -> Result<(), String> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(format!("receipt directory is unsafe: {}", path.display()));
        }
        return Ok(());
    }
    fs::create_dir_all(path).map_err(|error| format!("could not create receipt directory: {error}"))
}

fn read_regular(path: &Path) -> std::io::Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "receipt object is not a regular file",
        ));
    }
    fs::read(path)
}

fn frame(out: &mut Vec<u8>, value: &[u8]) {
    out.extend_from_slice(&(value.len() as u64).to_be_bytes());
    out.extend_from_slice(value);
}

fn take_frame(bytes: &[u8], cursor: &mut usize) -> Result<Vec<u8>, String> {
    let end = cursor
        .checked_add(8)
        .ok_or_else(|| "receipt frame length overflows".to_string())?;
    if end > bytes.len() {
        return Err("receipt frame length is truncated".into());
    }
    let mut length = [0u8; 8];
    length.copy_from_slice(&bytes[*cursor..end]);
    *cursor = end;
    let length = u64::from_be_bytes(length);
    if length > MAX_FIELD {
        return Err("receipt frame is too large".into());
    }
    let length = usize::try_from(length).map_err(|_| "receipt frame is too large".to_string())?;
    let end = cursor
        .checked_add(length)
        .ok_or_else(|| "receipt frame end overflows".to_string())?;
    if end > bytes.len() {
        return Err("receipt frame is truncated".into());
    }
    let value = bytes[*cursor..end].to_vec();
    *cursor = end;
    Ok(value)
}

fn encode_receipt(receipt: &Receipt) -> Result<Vec<u8>, String> {
    let mut out = encode_receipt_body(receipt);
    frame(&mut out, receipt.digest.as_bytes());
    Ok(out)
}

fn encode_receipt_body(receipt: &Receipt) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    frame(&mut out, receipt.claim.verb.as_bytes());
    frame(&mut out, receipt.claim.key.as_bytes());
    frame(&mut out, &receipt.status.to_be_bytes());
    frame(&mut out, &(receipt.claim.inputs.len() as u64).to_be_bytes());
    for input in &receipt.claim.inputs {
        frame(&mut out, input.path.to_string_lossy().as_bytes());
        frame(&mut out, input.digest.as_bytes());
    }
    frame(&mut out, &receipt.stdout);
    frame(&mut out, &receipt.stderr);
    out
}

fn decode_receipt(bytes: &[u8]) -> Result<Receipt, String> {
    if !bytes.starts_with(MAGIC) {
        return Err("receipt magic is invalid".into());
    }
    let mut cursor = MAGIC.len();
    let verb = String::from_utf8(take_frame(bytes, &mut cursor)?)
        .map_err(|_| "receipt verb is not UTF-8".to_string())?;
    let key = String::from_utf8(take_frame(bytes, &mut cursor)?)
        .map_err(|_| "receipt key is not UTF-8".to_string())?;
    let status = take_frame(bytes, &mut cursor)?;
    if status.len() != 4 {
        return Err("receipt status is malformed".into());
    }
    let status = i32::from_be_bytes([status[0], status[1], status[2], status[3]]);
    let count = take_frame(bytes, &mut cursor)?;
    if count.len() != 8 {
        return Err("receipt input count is malformed".into());
    }
    let count = u64::from_be_bytes(count.try_into().expect("checked receipt count length"));
    if count > 100_000 {
        return Err("receipt has too many inputs".into());
    }
    let mut inputs = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let path = String::from_utf8(take_frame(bytes, &mut cursor)?)
            .map_err(|_| "receipt input path is not UTF-8".to_string())?;
        let digest = String::from_utf8(take_frame(bytes, &mut cursor)?)
            .map_err(|_| "receipt input digest is not UTF-8".to_string())?;
        if !is_digest(&digest) {
            return Err("receipt input digest is malformed".into());
        }
        inputs.push(ReceiptInput {
            path: PathBuf::from(path),
            digest,
        });
    }
    let stdout = take_frame(bytes, &mut cursor)?;
    let stderr = take_frame(bytes, &mut cursor)?;
    let digest = String::from_utf8(take_frame(bytes, &mut cursor)?)
        .map_err(|_| "receipt digest is not UTF-8".to_string())?;
    if cursor != bytes.len() || !is_digest(&key) || !is_digest(&digest) {
        return Err("receipt has trailing bytes or malformed digest".into());
    }
    Ok(Receipt {
        claim: ReceiptClaim { verb, key, inputs },
        status,
        stdout,
        stderr,
        digest,
    })
}

fn receipt_digest(receipt: &Receipt) -> String {
    sha256_hex(&encode_receipt_body(receipt))
}

fn is_digest(value: &str) -> bool {
    value.len() == DIGEST_LEN
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn current_dir_bytes() -> Vec<u8> {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .to_string_lossy()
        .as_bytes()
        .to_vec()
}

fn argv_identity(argv: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    for arg in argv {
        frame(&mut out, arg.as_bytes());
    }
    out
}

fn environment_identity() -> Vec<u8> {
    let mut env: Vec<(String, String)> = std::env::vars().collect();
    env.retain(|(key, _)| key != "JET_RECEIPT_BYPASS");
    env.sort();
    let mut out = Vec::new();
    for (key, value) in env {
        frame(&mut out, key.as_bytes());
        frame(&mut out, value.as_bytes());
    }
    out
}

fn tool_identity() -> Vec<u8> {
    let mut out = Vec::new();
    for name in ["jet", "jetpack", "rustc", "cargo", "wasm-tools"] {
        frame(&mut out, name.as_bytes());
        let Some(path) = executable_on_path(name) else {
            frame(&mut out, b"missing");
            continue;
        };
        frame(&mut out, path.to_string_lossy().as_bytes());
        let digest = fs::read(&path)
            .map(|bytes| sha256_hex(&bytes))
            .unwrap_or_else(|_| "unreadable".into());
        frame(&mut out, digest.as_bytes());
    }
    out
}

fn executable_on_path(name: &str) -> Option<PathBuf> {
    if name == "jet" {
        return std::env::current_exe().ok();
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if regular_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn terminal_identity() -> Vec<u8> {
    use std::io::IsTerminal;
    [
        std::io::stdin().is_terminal(),
        std::io::stdout().is_terminal(),
        std::io::stderr().is_terminal(),
    ]
    .iter()
    .map(|value| if *value { b'1' } else { b'0' })
    .collect()
}

fn replay_receipt(command: &str, receipt: &Receipt) {
    write_bytes(std::io::stdout(), &receipt.stdout);
    write_bytes(std::io::stderr(), &receipt.stderr);
    let short = &receipt.claim.key[..12];
    let _ = writeln!(std::io::stderr(), "ok: {command} current (receipt {short})");
}

fn write_bytes(mut writer: impl Write, bytes: &[u8]) {
    let _ = writer.write_all(bytes);
    let _ = writer.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_body_digest_ignores_timestamp_because_there_is_none() {
        let claim = ReceiptClaim {
            verb: "check".into(),
            key: "a".repeat(DIGEST_LEN),
            inputs: Vec::new(),
        };
        let first = Receipt {
            claim: claim.clone(),
            status: 0,
            stdout: b"ok".to_vec(),
            stderr: Vec::new(),
            digest: String::new(),
        };
        let second = Receipt {
            claim,
            ..first.clone()
        };
        assert_eq!(receipt_digest(&first), receipt_digest(&second));
    }
}
