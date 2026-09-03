//! Content-addressed receipts for deterministic development acts.
//!
//! D-DEVR-TWICE1=A: `check`, `build`, `test`, `prove`, and `budget check`
//! consult one local receipt store before doing work. Receipt identity uses
//! input bytes and invocation context; filesystem timestamps never participate.
//! Result payloads are opaque bytes in this one codec; a claim never selects a
//! legacy result format.

use crate::SHA256::sha256_hex;
use jet_devserver::WatchService::WatchGraph;
use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::LazyLock;

const MAGIC: &[u8] = b"jet-receipt-v2\0";
const DIGEST_LEN: usize = 64;
const MAX_FIELD: u64 = 64 * 1024 * 1024;
const MAX_RECEIPT_BYTES: u64 = MAX_FIELD * 2 + 4 * 1024 * 1024;
const CAPTURE_TRUNCATION_MARKER: &[u8] = b"\n<output truncated>\n";
const RECEIPT_SECRET_NAME_PARTS: &[&str] =
    &["secret", "token", "password", "passwd", "credential", "key"];
const REDACTION_MARKER: &[u8] = b"<redacted>";
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

#[derive(Clone, PartialEq, Eq)]
pub struct Receipt {
    pub claim: ReceiptClaim,
    pub status: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub digest: String,
}

impl std::fmt::Debug for Receipt {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Receipt")
            .field("claim", &self.claim)
            .field("status", &self.status)
            .field("stdout", &"<redacted>")
            .field("stderr", &"<redacted>")
            .field("digest", &self.digest)
            .finish()
    }
}

pub struct ReceiptStore {
    root: PathBuf,
}

impl ReceiptStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn context_key(&self, verb: &str, argv: &[String]) -> Result<String, String> {
        Ok(sha256_hex(&context_identity(verb, argv)?))
    }

    fn context_path(&self, context_key: &str) -> PathBuf {
        self.root.join("contexts").join(context_key)
    }

    /// Build an action identity from argv, current tool/environment context,
    /// and the exact input files supplied by the caller.
    pub fn claim(
        &self,
        verb: &str,
        argv: &[String],
        input_paths: &[PathBuf],
    ) -> Result<ReceiptClaim, String> {
        self.claim_with_identity(verb, context_identity(verb, argv)?, input_paths)
    }

    fn claim_with_identity(
        &self,
        verb: &str,
        mut identity: Vec<u8>,
        input_paths: &[PathBuf],
    ) -> Result<ReceiptClaim, String> {
        // A check claim includes the authority and entry-selection state even
        // when a candidate or generated input is absent.  Existing files are
        // also carried below as ordinary inputs so stale claims can name the
        // exact file that changed.
        if verb == "check" {
            append_check_context_identity(&mut identity);
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

    /// Find a receipt through the cheap invocation index. The receipt carries
    /// its prior input closure, so current bytes are checked without loading
    /// the compiler dependency graph. Any mismatch falls back to discovery.
    fn lookup_context(
        &self,
        verb: &str,
        argv: &[String],
        cwd: &Path,
    ) -> Result<Option<Receipt>, String> {
        let identity = context_identity(verb, argv)?;
        let context_key = sha256_hex(&identity);
        let pointer = self.context_path(&context_key);
        let pointer_bytes = match read_regular(&pointer) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(format!(
                    "could not read receipt context {}: {error}",
                    pointer.display()
                ))
            }
        };
        let key = match String::from_utf8(pointer_bytes) {
            Ok(key) => key,
            Err(_) => return Ok(None),
        };
        if !is_digest(&key) {
            return Ok(None);
        }
        let object = self.object_path(&key);
        let bytes = match read_regular(&object) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(format!(
                    "could not read receipt {}: {error}",
                    object.display()
                ))
            }
        };
        let receipt = match decode_receipt(&bytes) {
            Ok(receipt) => receipt,
            Err(_) => return Ok(None),
        };
        if receipt.claim.key != key || receipt.claim.verb != verb {
            return Ok(None);
        }
        let input_paths = if verb == "check" && !has_explicit_target(verb, argv) {
            project_check_input_paths(cwd).unwrap_or_else(|| {
                target_path(verb, argv, cwd)
                    .filter(|target| target.is_dir())
                    .map(|_| input_paths_for(verb, argv, cwd))
                    .unwrap_or_else(|| {
                        receipt
                            .claim
                            .inputs
                            .iter()
                            .map(|input| input.path.clone())
                            .collect()
                    })
            })
        } else {
            match target_path(verb, argv, cwd) {
                Some(target) if target.is_dir() || verb == "budget check" => {
                    input_paths_for(verb, argv, cwd)
                }
                _ => {
                    let mut paths = receipt
                        .claim
                        .inputs
                        .iter()
                        .map(|input| input.path.clone())
                        .collect::<BTreeSet<_>>();
                    // Keep the stored WatchGraph closure cheap to validate,
                    // but rediscover authority paths so a newly-created
                    // workspace, lock, generated input, or entry candidate
                    // cannot hide behind an old claim.
                    if let Some(target) = target_path(verb, argv, cwd) {
                        add_project_inputs(&target, &mut paths);
                    }
                    paths.into_iter().collect()
                }
            }
        };
        let claim = match self.claim_with_identity(verb, identity, &input_paths) {
            Ok(claim) => claim,
            Err(_) => return Ok(None),
        };
        if claim != receipt.claim {
            let changes = changed_receipt_inputs(&receipt.claim.inputs, &claim.inputs);
            if changes.is_empty() {
                eprintln!(
                    "receipt: {verb} invalidated (entry candidates or authority context changed)"
                );
            } else {
                for path in changes {
                    eprintln!(
                        "receipt: {verb} invalidated (input changed: `{}`)",
                        path.display()
                    );
                }
            }
            return Ok(None);
        }
        if let Some(path) = stale_receipt_input(&claim.inputs) {
            eprintln!(
                "receipt: {verb} invalidated (input changed: `{}`)",
                path.display()
            );
            return Ok(None);
        }
        if receipt.digest != receipt_digest(&receipt) {
            eprintln!("receipt: {verb} invalidated (receipt authentication changed)");
            return Ok(None);
        }
        Ok(Some(receipt))
    }

    /// Publish the latest claim for one invocation. This index is advisory:
    /// a torn, stale, or forged pointer can only cause a cache miss.
    fn remember_context(
        &self,
        verb: &str,
        argv: &[String],
        claim: &ReceiptClaim,
    ) -> Result<(), String> {
        if claim.verb != verb || !is_digest(&claim.key) {
            return Err("receipt context claim is malformed".into());
        }
        let context_key = self.context_key(verb, argv)?;
        let path = self.context_path(&context_key);
        let parent = path
            .parent()
            .ok_or_else(|| "receipt context path has no parent".to_string())?;
        secure_create_dir(parent)?;
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(format!("receipt context is unsafe: {}", path.display()));
            }
        }
        let temp = parent.join(format!(
            ".{}.{}.{}",
            context_key,
            std::process::id(),
            NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
                .map_err(|error| format!("could not stage receipt context: {error}"))?;
            file.write_all(claim.key.as_bytes())
                .map_err(|error| format!("could not write receipt context: {error}"))?;
            file.sync_all()
                .map_err(|error| format!("could not flush receipt context: {error}"))?;
            fs::rename(&temp, &path)
                .map_err(|error| format!("could not publish receipt context: {error}"))?;
            sync_directory(parent)
                .map_err(|error| format!("could not flush receipt context directory: {error}"))?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
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

    /// Publish one immutable receipt object after redacting secret values.
    /// `true` means a new object was created; `false` means the exact object
    /// already existed.
    pub fn write(
        &self,
        claim: &ReceiptClaim,
        argv: &[String],
        status: i32,
        stdout: &[u8],
        stderr: &[u8],
    ) -> Result<bool, String> {
        let secret_values = receipt_secret_values(argv);
        if !is_digest(&claim.key) {
            return Err("receipt claim key is not a lowercase SHA-256 digest".into());
        }
        if !inputs_current(&claim.inputs) {
            return Ok(false);
        }
        let receipt = Receipt {
            claim: claim.clone(),
            status,
            stdout: bounded_redact_bytes(stdout, &secret_values)?,
            stderr: bounded_redact_bytes(stderr, &secret_values)?,
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
                    sync_directory(parent)
                        .map_err(|error| format!("could not flush receipt directory: {error}"))?;
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

    /// Record one command result through the canonical claim/write path and
    /// return the published receipt. Secret values are redacted before output
    /// reaches the stored receipt.
    pub fn record(
        &self,
        verb: &str,
        argv: &[String],
        input_paths: &[PathBuf],
        status: i32,
        stdout: &[u8],
        stderr: &[u8],
    ) -> Result<Receipt, String> {
        let claim = self.claim(verb, argv, input_paths)?;
        self.write(&claim, argv, status, stdout, stderr)?;
        self.lookup(&claim)?
            .ok_or_else(|| "receipt was not current after publication".into())
    }

    /// List valid immutable receipt objects in stable claim-key order.
    /// Temporary files and malformed objects are ignored; a ledger can only
    /// consume a fully decoded, self-authenticated receipt.
    pub fn list(&self) -> Result<Vec<Receipt>, String> {
        let objects = self.root.join("objects");
        let metadata = match fs::symlink_metadata(&objects) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(format!("could not inspect receipt store: {error}")),
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(format!(
                "receipt object store is not a regular directory: {}",
                objects.display()
            ));
        }

        let mut receipts = Vec::new();
        let entries =
            fs::read_dir(&objects).map_err(|error| format!("could not list receipts: {error}"))?;
        for entry in entries {
            let entry = entry.map_err(|error| format!("could not inspect receipt: {error}"))?;
            let path = entry.path();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if name.is_empty() || name.starts_with('.') {
                continue;
            }
            if !is_digest(name) {
                continue;
            }
            let bytes = match read_regular(&path) {
                Ok(bytes) => bytes,
                Err(_) => continue,
            };
            let Ok(receipt) = decode_receipt(&bytes) else {
                continue;
            };
            if receipt.claim.key != name || receipt.digest != receipt_digest(&receipt) {
                continue;
            }
            receipts.push(receipt);
        }
        receipts.sort_by(|left, right| left.claim.key.cmp(&right.claim.key));
        Ok(receipts)
    }

    /// A receipt is current only when its input closure still hashes to the
    /// claim and its immutable object still authenticates.
    pub fn is_current(&self, receipt: &Receipt) -> Result<bool, String> {
        let current = self.lookup(&receipt.claim)?;
        Ok(current.as_ref().is_some_and(|current| current == receipt))
    }

    /// Return only receipts safe for a status projection. Stale and malformed
    /// objects never become ledger claims.
    pub fn list_current(&self) -> Result<Vec<Receipt>, String> {
        self.list()?
            .into_iter()
            .filter_map(|receipt| match self.is_current(&receipt) {
                Ok(true) => Some(Ok(receipt)),
                Ok(false) => None,
                Err(error) => Some(Err(error)),
            })
            .collect()
    }

    pub fn object_path(&self, key: &str) -> PathBuf {
        self.root.join("objects").join(key)
    }
}

/// Resolve a CLI invocation's source closure. Direct source files use the
/// compiler's import graph; bare project checks and package-level test/budget
/// actions use the whole package/workspace authority tree.
pub fn input_paths_for(verb: &str, argv: &[String], cwd: &Path) -> Vec<PathBuf> {
    if verb == "check" && !has_explicit_target(verb, argv) {
        if let Some(paths) = project_check_input_paths(cwd) {
            return paths;
        }
    }
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
    // Timed invocations must execute the producer so timing and cache
    // diagnostics describe this invocation. The inner content caches remain
    // active; only the whole-invocation receipt replay is disabled.
    if std::env::var_os("JET_RECEIPT_BYPASS").is_some() {
        return None;
    }
    let verb = participating_verb(argv)?;
    if !cacheable_invocation(verb, argv) {
        return None;
    }
    let cwd = std::env::current_dir().ok()?;
    let root = receipt_root(verb, argv, &cwd);
    let store = ReceiptStore::new(root);
    if let Ok(Some(receipt)) = store.lookup_context(verb, argv, &cwd) {
        let secret_values = receipt_secret_values(argv);
        replay_receipt(verb, &receipt, &secret_values);
        return Some(receipt.status);
    }

    let input_paths = if verb == "check" && !has_explicit_target(verb, argv) {
        project_check_input_paths(&cwd)
            .unwrap_or_else(|| input_paths_for(verb, argv, &cwd))
    } else {
        input_paths_for(verb, argv, &cwd)
    };
    if input_paths.is_empty() && verb != "budget check" {
        return None;
    }
    let claim = store.claim(verb, argv, &input_paths).ok()?;

    let executable = std::env::current_exe().ok()?;
    let mut child = std::process::Command::new(executable)
        .args(argv)
        .current_dir(&cwd)
        .env("JET_RECEIPT_BYPASS", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    let stderr = child.stderr.take()?;
    let stdout_reader = std::thread::spawn(move || capture_stream(stdout, std::io::stdout()));
    let stderr_reader = std::thread::spawn(move || capture_stream(stderr, std::io::stderr()));
    let status = child.wait().ok()?.code().unwrap_or(1);
    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    let receipt_stderr = canonicalize_receipt_stderr(verb, &stderr);
    if store
        .write(&claim, argv, status, &stdout, &receipt_stderr)
        .is_ok()
    {
        let _ = store.remember_context(verb, argv, &claim);
    }
    Some(status)
}

fn canonicalize_receipt_stderr(verb: &str, stderr: &[u8]) -> Vec<u8> {
    if verb != "build" {
        return stderr.to_vec();
    }
    let mut canonical = Vec::with_capacity(stderr.len());
    let mut offset = 0;
    while offset < stderr.len() {
        let line_end = stderr[offset..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|index| offset + index)
            .unwrap_or(stderr.len());
        let line = &stderr[offset..line_end];
        let rewritten = find_bytes(line, b"Built ").and_then(|built| {
            let start = built + b"Built ".len();
            let timing = find_bytes(&line[start..], b" in ")?;
            let timing_start = start + timing;
            let check = find_bytes(&line[timing_start + 4..], b"\xE2\x9C\x93")?;
            let check_start = timing_start + 4 + check;
            let mut output = Vec::with_capacity(line.len());
            output.extend_from_slice(&line[..timing_start]);
            output.push(b' ');
            output.extend_from_slice(&line[check_start..]);
            Some(output)
        });
        canonical.extend_from_slice(rewritten.as_deref().unwrap_or(line));
        if line_end < stderr.len() {
            canonical.push(b'\n');
        }
        offset = line_end.saturating_add(1);
    }
    canonical
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
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
            receipt_package_root(start).or_else(|| Some(start.to_path_buf()))
        })
        .unwrap_or_else(|| cwd.to_path_buf());
    base.join(".jet").join("receipts")
}

fn receipt_package_root(start: &Path) -> Option<PathBuf> {
    crate::Loader::find_package_root_checked(start)
        .ok()
        .flatten()
}

fn receipt_authority_roots(start: &Path) -> BTreeSet<PathBuf> {
    let mut roots = BTreeSet::new();
    if let Ok(Some(root)) = crate::Loader::find_workspace_root_checked(start) {
        roots.insert(root);
    }
    if let Some(root) = receipt_package_root(start) {
        roots.insert(root);
    }

    // Checked discovery is authoritative when it succeeds.  Keep a lexical
    // filename fallback as an invalidation floor when an authority is newly
    // malformed: an old receipt must not replay merely because discovery can
    // no longer parse the changed boundary.
    let mut package_seen = false;
    let mut dir = if start.is_dir() {
        start.to_path_buf()
    } else {
        start
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    };
    loop {
        let package = regular_file(&dir.join(crate::Syntax::PACKAGE_FILE))
            || regular_file(&dir.join(crate::Syntax::PAYLOAD_FILE));
        let workspace = regular_file(&dir.join("workspace.jet"));
        if package && !package_seen {
            roots.insert(dir.clone());
            package_seen = true;
        }
        if workspace {
            roots.insert(dir.clone());
            break;
        }
        let Some(parent) = dir.parent() else {
            break;
        };
        if parent == dir {
            break;
        }
        dir = parent.to_path_buf();
    }
    roots
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
        let root = receipt_package_root(cwd).unwrap_or_else(|| cwd.to_path_buf());
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

    // Match the command's extension-optional source resolution.  The CLI's
    // resolver is binary-only, so mirror its missing-stem path here while
    // keeping a nonexistent stem uncached.
    let resolved = PathBuf::from(format!(
        "{}.{}",
        candidate.display(),
        crate::Syntax::FILE_EXT
    ));
    regular_file(&resolved).then_some(resolved)
}

/// Locate the project receipt store without running the act it stores.
pub fn receipt_root_for(verb: &str, argv: &[String], cwd: &Path) -> PathBuf {
    receipt_root(verb, argv, cwd)
}

fn has_explicit_target(verb: &str, argv: &[String]) -> bool {
    let mut skip_next = false;
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
            return true;
        }
    }
    false
}

fn project_check_input_paths(cwd: &Path) -> Option<Vec<PathBuf>> {
    let roots = receipt_authority_roots(cwd);
    if roots.is_empty() {
        return None;
    }
    let mut paths = BTreeSet::new();
    for root in roots {
        collect_tree_inputs(&root, "check", &mut paths);
    }
    let sources = paths
        .iter()
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == crate::Syntax::FILE_EXT)
        })
        .cloned()
        .collect::<Vec<_>>();
    for source in sources {
        if let Ok(graph) = WatchGraph::discover(&source) {
            paths.extend(graph.watched_paths());
        }
    }
    Some(paths.into_iter().filter(|path| regular_file(path)).collect())
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
                collect_tree_inputs(&path.join("generated"), verb, out);
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
            || matches!(
                name,
                crate::Syntax::PACKAGE_FILE
                    | crate::Syntax::PAYLOAD_FILE
                    | "workspace.jet"
                    | "lock"
            );
        let is_generated_input = path
            .components()
            .collect::<Vec<_>>()
            .windows(2)
            .any(|components| {
                components[0].as_os_str() == ".jet"
                    && components[1].as_os_str() == "generated"
            });
        let is_budget_input = verb == "budget check"
            && path
                .components()
                .any(|component| component.as_os_str() == ".jet");
        if is_source || is_generated_input || is_budget_input {
            out.insert(path);
        }
    }
}

fn add_project_inputs(entry: &Path, out: &mut BTreeSet<PathBuf>) {
    let start = entry.parent().unwrap_or_else(|| Path::new("."));
    let roots = receipt_authority_roots(start);

    // A file-scoped check still reads its owning package/workspace authorities.
    // Keep source reuse on WatchGraph, but fingerprint every authority input that
    // can change the graph or the selected output without widening to unrelated
    // source files in the enclosing workspace.
    for root in roots {
        for name in [
            crate::Syntax::PACKAGE_FILE,
            crate::Syntax::PAYLOAD_FILE,
            "workspace.jet",
        ] {
            let path = root.join(name);
            if regular_file(&path) {
                out.insert(path);
            }
        }
        let lock = root.join(crate::Syntax::UNIFIED_LOCK_FILE);
        if regular_file(&lock) {
            out.insert(lock);
        }
        for candidate in check_entry_candidates(&root) {
            if regular_file(&candidate) {
                out.insert(candidate);
            }
        }
        collect_tree_inputs(&root.join(".jet").join("generated"), "check", out);
    }
}
fn append_check_context_identity(identity: &mut Vec<u8>) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    frame(identity, b"check-context-v2");
    append_context_path(identity, b"working-directory", &cwd);

    let roots = receipt_authority_roots(&cwd);
    if roots.is_empty() {
        frame(identity, b"authority-roots");
        frame(identity, b"none");
        return;
    }
    for root in roots {
        append_context_path(identity, b"package-root", &root);
        for (priority, candidate) in check_entry_candidates(&root).into_iter().enumerate() {
            frame(identity, b"entry-candidate");
            frame(identity, priority.to_string().as_bytes());
            append_context_path(identity, b"path", &candidate);
        }
        for name in [
            crate::Syntax::PACKAGE_FILE,
            crate::Syntax::PAYLOAD_FILE,
            "workspace.jet",
            crate::Syntax::UNIFIED_LOCK_FILE,
        ] {
            append_context_path(identity, name.as_bytes(), &root.join(name));
        }
        append_context_tree(identity, &root.join(".jet").join("generated"));
    }
}

fn check_entry_candidates(root: &Path) -> Vec<PathBuf> {
    let mut candidates = vec![
        root.join(crate::Syntax::DEFAULT_ENTRY_FILE),
        root.join("src").join(crate::Syntax::DEFAULT_ENTRY_FILE),
        root.join(crate::Syntax::LEGACY_ENTRY_FILE),
    ];
    if let Ok(Some(facts)) = crate::Loader::package_facts_for_root(root) {
        if !facts.name.is_empty() {
            candidates.push(root.join(format!("{}.{}", facts.name, crate::Syntax::FILE_EXT)));
        }
    }
    candidates.dedup();
    candidates
}

fn append_context_tree(identity: &mut Vec<u8>, root: &Path) {
    frame(identity, b"generated-inputs");
    append_context_path(identity, b"root", root);
    let mut files = BTreeSet::new();
    collect_tree_inputs(root, "check", &mut files);
    for path in files {
        append_context_path(identity, b"file", &path);
    }
}

fn append_context_path(identity: &mut Vec<u8>, label: &[u8], path: &Path) {
    frame(identity, label);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    let display = fs::canonicalize(&absolute).unwrap_or(absolute);
    frame(identity, display.to_string_lossy().as_bytes());
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => frame(identity, b"symlink"),
        Ok(metadata) if metadata.is_file() => {
            frame(identity, b"file");
            frame(
                identity,
                file_digest(path)
                    .unwrap_or_else(|_| "unreadable".to_string())
                    .as_bytes(),
            );
        }
        Ok(metadata) if metadata.is_dir() => frame(identity, b"directory"),
        Ok(_) => frame(identity, b"other"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            frame(identity, b"missing")
        }
        Err(error) => {
            frame(identity, b"unreadable");
            frame(identity, error.kind().to_string().as_bytes());
        }
    }
}

fn changed_receipt_inputs(old: &[ReceiptInput], new: &[ReceiptInput]) -> Vec<PathBuf> {
    let mut changed = Vec::new();
    for input in old {
        if new
            .iter()
            .find(|candidate| candidate.path == input.path)
            .is_none_or(|candidate| candidate.digest != input.digest)
        {
            changed.push(input.path.clone());
        }
    }
    for input in new {
        if old
            .iter()
            .all(|candidate| candidate.path != input.path)
        {
            changed.push(input.path.clone());
        }
    }
    changed.sort();
    changed.dedup();
    changed
}

fn stale_receipt_input(inputs: &[ReceiptInput]) -> Option<PathBuf> {
    inputs.iter().find_map(|input| {
        file_digest(&input.path)
            .ok()
            .filter(|digest| digest == &input.digest)
            .is_none()
            .then(|| input.path.clone())
    })
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
    crate::SHA256::sha256_file_hex(path)
        .map_err(|error| format!("could not read input {}: {error}", path.display()))
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
    crate::SHA256::read_file_nofollow(path, MAX_RECEIPT_BYTES)
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

fn context_identity(verb: &str, argv: &[String]) -> Result<Vec<u8>, String> {
    if verb.is_empty() {
        return Err("receipt verb is empty".into());
    }
    let mut identity = Vec::new();
    identity.extend_from_slice(MAGIC);
    frame(&mut identity, verb.as_bytes());
    frame(&mut identity, &current_dir_bytes());
    frame(&mut identity, &argv_identity(argv));
    frame(&mut identity, &environment_identity());
    frame(&mut identity, tool_identity());
    frame(&mut identity, &terminal_identity());
    Ok(identity)
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
    let mut redact_next = false;
    for arg in argv {
        if redact_next {
            frame(&mut out, REDACTION_MARKER);
            redact_next = false;
            continue;
        }
        if let Some((name, _)) = arg.split_once('=') {
            if is_secret_name(name) {
                let mut redacted = String::with_capacity(name.len() + 1 + REDACTION_MARKER.len());
                redacted.push_str(name);
                redacted.push_str("=<redacted>");
                frame(&mut out, redacted.as_bytes());
            } else {
                frame(&mut out, arg.as_bytes());
            }
            continue;
        }
        frame(&mut out, arg.as_bytes());
        redact_next = is_secret_name(arg);
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
        frame(
            &mut out,
            if is_secret_environment_name(&key) {
                REDACTION_MARKER
            } else {
                value.as_bytes()
            },
        );
    }
    out
}

fn tool_identity() -> &'static [u8] {
    static IDENTITY: LazyLock<Vec<u8>> = LazyLock::new(|| {
        let mut out = Vec::new();
        for name in ["jet", "jetpack", "rustc", "cargo", "wasm-tools"] {
            frame(&mut out, name.as_bytes());
            let Some(path) = executable_on_path(name) else {
                frame(&mut out, b"missing");
                continue;
            };
            frame(&mut out, path.to_string_lossy().as_bytes());
            let digest = tool_digest(name, &path);
            frame(&mut out, digest.as_bytes());
        }
        out
    });
    IDENTITY.as_slice()
}
fn tool_digest(name: &str, path: &Path) -> String {
    if name == "jet" {
        env!("JET_COMPILER_BUILD_ID").to_string()
    } else {
        crate::SHA256::sha256_file_hex(path).unwrap_or_else(|_| "unreadable".into())
    }
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

fn replay_receipt(command: &str, receipt: &Receipt, secret_values: &[String]) {
    let (stdout, stderr) = replay_output(receipt, secret_values);
    write_bytes(std::io::stdout(), &stdout);
    write_bytes(std::io::stderr(), &stderr);
    let short = &receipt.claim.key[..12];
    let _ = writeln!(std::io::stderr(), "ok: {command} current (receipt {short})");
}

fn replay_output(receipt: &Receipt, secret_values: &[String]) -> (Vec<u8>, Vec<u8>) {
    (
        redact_bytes(&receipt.stdout, secret_values),
        redact_bytes(&receipt.stderr, secret_values),
    )
}

fn receipt_secret_values(argv: &[String]) -> Vec<String> {
    // Keep this value policy aligned with
    // `jet_process_policy_secret_values` in the shared Prelude redactor.
    let mut values = std::env::vars()
        .filter(|(name, value)| !value.is_empty() && is_secret_environment_name(name))
        .map(|(_, value)| value)
        .collect::<Vec<_>>();
    for (index, argument) in argv.iter().enumerate() {
        if let Some((name, value)) = argument.split_once('=') {
            if is_secret_name(name) && !value.is_empty() {
                values.push(value.to_string());
            }
        } else if is_secret_name(argument) {
            if let Some(value) = argv.get(index + 1).filter(|value| !value.is_empty()) {
                values.push(value.clone());
            }
        }
    }
    values.sort_by_key(|value| std::cmp::Reverse(value.len()));
    values.dedup();
    values
}

fn is_secret_name(name: &str) -> bool {
    let characters: Vec<_> = name.trim_start_matches('-').chars().collect();
    let mut component = String::new();
    for index in 0..=characters.len() {
        let Some(character) = characters.get(index).copied() else {
            return is_secret_component(&component);
        };
        let previous = index
            .checked_sub(1)
            .and_then(|previous| characters.get(previous).copied());
        let next = characters.get(index + 1).copied();
        let camel_boundary = character.is_ascii_uppercase()
            && previous.is_some_and(|previous| {
                previous.is_ascii_lowercase()
                    || previous.is_ascii_digit()
                    || (previous.is_ascii_uppercase()
                        && next.is_some_and(|next| next.is_ascii_lowercase()))
            });
        if !character.is_ascii_alphanumeric() || camel_boundary {
            if is_secret_component(&component) {
                return true;
            }
            component.clear();
            if !character.is_ascii_alphanumeric() {
                continue;
            }
        }
        component.push(character.to_ascii_lowercase());
    }
    false
}

fn is_secret_component(component: &str) -> bool {
    let component = component.trim_end_matches(|character: char| character.is_ascii_digit());
    RECEIPT_SECRET_NAME_PARTS
        .iter()
        .any(|part| *part == component)
}

fn is_secret_environment_name(name: &str) -> bool {
    let normalized = name.to_ascii_lowercase();
    is_secret_name(name)
        || RECEIPT_SECRET_NAME_PARTS
            .iter()
            .any(|part| normalized.contains(part))
}

fn redact_bytes(bytes: &[u8], secret_values: &[String]) -> Vec<u8> {
    secret_values.iter().fold(bytes.to_vec(), |bytes, value| {
        replace_bytes(&bytes, value.as_bytes())
    })
}

fn bounded_redact_bytes(bytes: &[u8], secret_values: &[String]) -> Result<Vec<u8>, String> {
    if bytes.len() as u64 > MAX_FIELD {
        return Err("receipt output exceeds its size limit".into());
    }
    let redacted = redact_bytes(bytes, secret_values);
    if redacted.len() as u64 > MAX_FIELD {
        return Err("redacted receipt output exceeds its size limit".into());
    }
    Ok(redacted)
}

fn replace_bytes(bytes: &[u8], needle: &[u8]) -> Vec<u8> {
    if needle.is_empty() || needle.len() > bytes.len() {
        return bytes.to_vec();
    }
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index <= bytes.len().saturating_sub(needle.len()) {
        if &bytes[index..index + needle.len()] == needle {
            output.extend_from_slice(REDACTION_MARKER);
            index += needle.len();
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    output.extend_from_slice(&bytes[index..]);
    output
}

fn capture_stream(mut reader: impl Read, mut writer: impl Write) -> Vec<u8> {
    let mut captured = Vec::new();
    let mut buffer = [0; 8192];
    let mut truncated = false;
    loop {
        let Ok(read) = reader.read(&mut buffer) else {
            break;
        };
        if read == 0 {
            break;
        }
        let remaining = (MAX_FIELD as usize).saturating_sub(captured.len());
        let keep = remaining.min(read);
        captured.extend_from_slice(&buffer[..keep]);
        truncated |= keep < read;
        if writer.write_all(&buffer[..read]).is_err() {
            break;
        }
        let _ = writer.flush();
    }
    if truncated {
        captured.extend_from_slice(CAPTURE_TRUNCATION_MARKER);
    }
    captured
}

fn sync_directory(path: &Path) -> std::io::Result<()> {
    fs::File::open(path)?.sync_all()
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

    #[test]
    fn compiler_tool_identity_never_reads_the_running_binary() {
        assert_eq!(
            tool_digest("jet", Path::new("/definitely/missing/jet")),
            env!("JET_COMPILER_BUILD_ID")
        );
    }

    #[test]
    fn captured_child_output_is_streamed_and_retained() {
        let input = b"one\ntwo\n".as_slice();
        let mut streamed = Vec::new();
        let captured = capture_stream(input, &mut streamed);
        assert_eq!(captured, input);
        assert_eq!(streamed, input);
    }

    #[test]
    fn malformed_context_pointer_is_a_cache_miss() {
        let root = std::env::temp_dir().join(format!(
            "jet-receipt-malformed-context-{}-{}",
            std::process::id(),
            NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        let store = ReceiptStore::new(&root);
        let argv = vec!["check".to_string()];
        let context_key = store.context_key("check", &argv).unwrap();
        let pointer = store.context_path(&context_key);
        fs::create_dir_all(pointer.parent().unwrap()).unwrap();
        fs::write(&pointer, [0xff, 0xfe]).unwrap();

        assert!(store
            .lookup_context("check", &argv, &root)
            .unwrap()
            .is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn project_check_receipt_rejects_new_higher_priority_entry() {
        let project = std::env::temp_dir().join(format!(
            "jet-receipt-entry-priority-{}-{}",
            std::process::id(),
            NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let receipt_root = project.join("receipts");
        let _ = fs::remove_dir_all(&project);
        fs::create_dir_all(project.join("src")).unwrap();
        fs::write(
            project.join("package.jet"),
            "name: \"receipt-entry-priority\"\nversion: \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(project.join("src").join("run.jet"), "fn run() {}\n").unwrap();

        let argv = vec!["check".to_string()];
        let store = ReceiptStore::new(&receipt_root);
        let initial_inputs = input_paths_for("check", &argv, &project);
        assert!(initial_inputs
            .iter()
            .any(|path| path.ends_with("src/run.jet")));
        let claim = store.claim("check", &argv, &initial_inputs).unwrap();
        store.write(&claim, &argv, 0, b"first", b"").unwrap();
        store.remember_context("check", &argv, &claim).unwrap();

        fs::write(project.join("run.jet"), "fn run() {}\n").unwrap();
        assert!(project_check_input_paths(&project)
            .unwrap()
            .iter()
            .any(|path| path.ends_with("run.jet")));
        let current_inputs = input_paths_for("check", &argv, &project);
        let current_claim = store.claim("check", &argv, &current_inputs).unwrap();
        assert_ne!(claim.key, current_claim.key);
        assert!(store
            .lookup_context("check", &argv, &project)
            .unwrap()
            .is_none());
        let _ = fs::remove_dir_all(project);
    }

    #[test]
    fn project_check_receipt_rejects_changed_generated_input() {
        let project = std::env::temp_dir().join(format!(
            "jet-receipt-generated-input-{}-{}",
            std::process::id(),
            NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let receipt_root = project.join("receipts");
        let generated = project.join(".jet").join("generated");
        let _ = fs::remove_dir_all(&project);
        fs::create_dir_all(project.join("src")).unwrap();
        fs::create_dir_all(&generated).unwrap();
        fs::write(
            project.join("package.jet"),
            "name: \"receipt-generated-input\"\nversion: \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(project.join("src").join("run.jet"), "fn run() {}\n").unwrap();
        fs::write(generated.join("inputs.jet"), "generated-v1\n").unwrap();

        let argv = vec!["check".to_string()];
        let store = ReceiptStore::new(&receipt_root);
        let initial_inputs = input_paths_for("check", &argv, &project);
        assert!(initial_inputs
            .iter()
            .any(|path| path == &generated.join("inputs.jet")));
        let claim = store.claim("check", &argv, &initial_inputs).unwrap();
        store.write(&claim, &argv, 0, b"first", b"").unwrap();
        store.remember_context("check", &argv, &claim).unwrap();

        fs::write(generated.join("inputs.jet"), "generated-v2\n").unwrap();
        let current_inputs = input_paths_for("check", &argv, &project);
        let current_claim = store.claim("check", &argv, &current_inputs).unwrap();
        assert_ne!(claim.key, current_claim.key);
        assert!(store
            .lookup_context("check", &argv, &project)
            .unwrap()
            .is_none());
        let _ = fs::remove_dir_all(project);
    }

    #[test]
    fn project_check_receipt_rejects_new_workspace_input() {
        let project = std::env::temp_dir().join(format!(
            "jet-receipt-workspace-input-{}-{}",
            std::process::id(),
            NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let workspace = project.join("workspace.jet");
        let _ = fs::remove_dir_all(&project);
        fs::create_dir_all(project.join("src")).unwrap();
        fs::write(
            project.join("package.jet"),
            "name: \"receipt-workspace-input\"\nversion: \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(project.join("src").join("run.jet"), "fn run() {}\n").unwrap();

        let argv = vec!["check".to_string()];
        let store = ReceiptStore::new(project.join("receipts"));
        let initial_inputs = input_paths_for("check", &argv, &project);
        assert!(!initial_inputs.iter().any(|path| path == &workspace));
        let claim = store.claim("check", &argv, &initial_inputs).unwrap();
        store.write(&claim, &argv, 0, b"first", b"").unwrap();
        store.remember_context("check", &argv, &claim).unwrap();

        fs::write(&workspace, "module workspace { members: [] }\n").unwrap();
        let current_inputs = input_paths_for("check", &argv, &project);
        assert!(current_inputs.iter().any(|path| path == &workspace));
        let current_claim = store.claim("check", &argv, &current_inputs).unwrap();
        assert_ne!(claim.key, current_claim.key);
        assert!(store
            .lookup_context("check", &argv, &project)
            .unwrap()
            .is_none());
        let _ = fs::remove_dir_all(project);
    }

    #[test]
    fn project_check_receipt_rejects_new_lock_input() {
        let project = std::env::temp_dir().join(format!(
            "jet-receipt-lock-input-{}-{}",
            std::process::id(),
            NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let lock = project.join(".jet").join("lock");
        let _ = fs::remove_dir_all(&project);
        fs::create_dir_all(project.join("src")).unwrap();
        fs::write(
            project.join("package.jet"),
            "name: \"receipt-lock-input\"\nversion: \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(project.join("src").join("run.jet"), "fn run() {}\n").unwrap();

        let argv = vec!["check".to_string()];
        let store = ReceiptStore::new(project.join("receipts"));
        let initial_inputs = input_paths_for("check", &argv, &project);
        assert!(!initial_inputs.iter().any(|path| path == &lock));
        let claim = store.claim("check", &argv, &initial_inputs).unwrap();
        store.write(&claim, &argv, 0, b"first", b"").unwrap();
        store.remember_context("check", &argv, &claim).unwrap();

        fs::create_dir_all(lock.parent().unwrap()).unwrap();
        fs::write(&lock, "lock-v1\n").unwrap();
        let current_inputs = input_paths_for("check", &argv, &project);
        assert!(current_inputs.iter().any(|path| path == &lock));
        let current_claim = store.claim("check", &argv, &current_inputs).unwrap();
        assert_ne!(claim.key, current_claim.key);
        assert!(store
            .lookup_context("check", &argv, &project)
            .unwrap()
            .is_none());
        let _ = fs::remove_dir_all(project);
    }

    #[test]
    fn project_check_receipt_rejects_changed_output_authority() {
        let project = std::env::temp_dir().join(format!(
            "jet-receipt-output-authority-{}-{}",
            std::process::id(),
            NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let package = project.join("package.jet");
        let _ = fs::remove_dir_all(&project);
        fs::create_dir_all(project.join("src")).unwrap();
        fs::write(
            &package,
            "name: \"receipt-output-authority\"\nversion: \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(project.join("src").join("run.jet"), "fn run() {}\n").unwrap();

        let argv = vec!["check".to_string()];
        let store = ReceiptStore::new(project.join("receipts"));
        let initial_inputs = input_paths_for("check", &argv, &project);
        assert!(initial_inputs.iter().any(|path| path == &package));
        let claim = store.claim("check", &argv, &initial_inputs).unwrap();
        store.write(&claim, &argv, 0, b"first", b"").unwrap();
        store.remember_context("check", &argv, &claim).unwrap();

        fs::write(
            &package,
            "name: \"project-check-output\"\noutputs: { release: .Executable{ entry: run } }\n",
        )
        .unwrap();
        let current_inputs = input_paths_for("check", &argv, &project);
        let current_claim = store.claim("check", &argv, &current_inputs).unwrap();
        assert_ne!(claim.key, current_claim.key);
        assert!(store
            .lookup_context("check", &argv, &project)
            .unwrap()
            .is_none());
        let _ = fs::remove_dir_all(project);
    }

    #[test]
    fn explicit_check_receipt_rejects_new_generated_input() {
        let project = std::env::temp_dir().join(format!(
            "jet-receipt-explicit-generated-{}-{}",
            std::process::id(),
            NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let source = project.join("src/main.jet");
        let receipt_root = project.join("receipts");
        let generated = project.join(".jet/generated/inputs.jet");
        let _ = fs::remove_dir_all(&project);
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(
            project.join("package.jet"),
            "name: \"receipt-explicit-generated\"\nversion: \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(&source, "fn run() {}\n").unwrap();

        let argv = vec!["check".to_string(), source.display().to_string()];
        let store = ReceiptStore::new(&receipt_root);
        let initial_inputs = input_paths_for("check", &argv, &project);
        let claim = store.claim("check", &argv, &initial_inputs).unwrap();
        store.write(&claim, &argv, 0, b"first", b"").unwrap();
        store.remember_context("check", &argv, &claim).unwrap();
        assert!(store
            .lookup_context("check", &argv, &project)
            .unwrap()
            .is_some());

        fs::create_dir_all(&generated).unwrap();
        fs::write(generated.join("inputs.jet"), "generated-v1\n").unwrap();
        let current_inputs = input_paths_for("check", &argv, &project);
        let current_claim = store.claim("check", &argv, &current_inputs).unwrap();
        assert_ne!(claim.key, current_claim.key);
        assert!(store
            .lookup_context("check", &argv, &project)
            .unwrap()
            .is_none());
        let _ = fs::remove_dir_all(project);
    }

    #[test]
    fn project_check_receipt_rejects_new_retired_manifest_alias() {
        let project = std::env::temp_dir().join(format!(
            "jet-receipt-retired-manifest-{}-{}",
            std::process::id(),
            NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let retired = project.join(crate::Syntax::PAYLOAD_FILE);
        let _ = fs::remove_dir_all(&project);
        fs::create_dir_all(&project).unwrap();
        fs::write(
            project.join(crate::Syntax::PACKAGE_FILE),
            "name: \"receipt-retired-manifest\"\nversion: \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            project.join(crate::Syntax::DEFAULT_ENTRY_FILE),
            "fn run() {}\n",
        )
        .unwrap();

        let argv = vec!["check".to_string()];
        let store = ReceiptStore::new(project.join("receipts"));
        let initial_inputs = input_paths_for("check", &argv, &project);
        assert!(!initial_inputs.iter().any(|path| path == &retired));
        let claim = store.claim("check", &argv, &initial_inputs).unwrap();
        store.write(&claim, &argv, 0, b"first", b"").unwrap();
        store.remember_context("check", &argv, &claim).unwrap();

        fs::write(&retired, "name: \"retired\"\n").unwrap();
        let current_inputs = input_paths_for("check", &argv, &project);
        assert!(current_inputs.iter().any(|path| path == &retired));
        let current_claim = store.claim("check", &argv, &current_inputs).unwrap();
        assert_ne!(claim.key, current_claim.key);
        assert!(store
            .lookup_context("check", &argv, &project)
            .unwrap()
            .is_none());
        let _ = fs::remove_dir_all(project);
    }

    #[test]
    fn project_check_receipt_rejects_new_malformed_workspace_authority() {
        let project = std::env::temp_dir().join(format!(
            "jet-receipt-malformed-workspace-{}-{}",
            std::process::id(),
            NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let workspace = project.join("workspace.jet");
        let _ = fs::remove_dir_all(&project);
        fs::create_dir_all(&project).unwrap();
        fs::write(
            project.join(crate::Syntax::PACKAGE_FILE),
            "name: \"receipt-malformed-workspace\"\nversion: \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            project.join(crate::Syntax::DEFAULT_ENTRY_FILE),
            "fn run() {}\n",
        )
        .unwrap();

        let argv = vec!["check".to_string()];
        let store = ReceiptStore::new(project.join("receipts"));
        let initial_inputs = input_paths_for("check", &argv, &project);
        assert!(!initial_inputs.iter().any(|path| path == &workspace));
        let claim = store.claim("check", &argv, &initial_inputs).unwrap();
        store.write(&claim, &argv, 0, b"first", b"").unwrap();
        store.remember_context("check", &argv, &claim).unwrap();

        fs::write(&workspace, "module workspace {\n").unwrap();
        let current_inputs = input_paths_for("check", &argv, &project);
        assert!(current_inputs.iter().any(|path| path == &workspace));
        let current_claim = store.claim("check", &argv, &current_inputs).unwrap();
        assert_ne!(claim.key, current_claim.key);
        assert!(store
            .lookup_context("check", &argv, &project)
            .unwrap()
            .is_none());
        let _ = fs::remove_dir_all(project);
    }

    #[test]
    fn project_check_receipt_uses_inline_package_root_for_nested_cwd() {
        let project = std::env::temp_dir().join(format!(
            "jet-receipt-inline-package-{}-{}",
            std::process::id(),
            NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&project);
        fs::create_dir_all(project.join("src")).unwrap();
        fs::write(
            project.join(crate::Syntax::DEFAULT_ENTRY_FILE),
            "package {\nname: \"receipt-inline\"\nversion: \"0.1.0\"\n}\nfn run() {}\n",
        )
        .unwrap();

        let argv = vec!["check".to_string()];
        assert_eq!(
            receipt_root_for("check", &argv, &project.join("src")),
            project.join(".jet").join("receipts")
        );
        let inputs = input_paths_for("check", &argv, &project.join("src"));
        assert!(inputs
            .iter()
            .any(|path| path == &project.join(crate::Syntax::DEFAULT_ENTRY_FILE)));
        let _ = fs::remove_dir_all(project);
    }


    #[test]
    fn receipt_debug_redacts_raw_legacy_and_unrecognized_output() {
        let receipt = Receipt {
            claim: ReceiptClaim {
                verb: "check".into(),
                key: "a".repeat(DIGEST_LEN),
                inputs: Vec::new(),
            },
            status: 17,
            stdout: b"legacy bearer secret\xff".to_vec(),
            stderr: vec![0, 1, 2, 255],
            digest: "b".repeat(DIGEST_LEN),
        };
        let debug = format!("{receipt:?}");
        assert!(debug.contains("stdout: \"<redacted>\""));
        assert!(debug.contains("stderr: \"<redacted>\""));
        assert!(!debug.contains("legacy bearer secret"));
    }

    #[test]
    fn secret_names_cover_camel_case_dotted_env_and_receipt_identity() {
        for name in [
            "--apiToken",
            "--auth.token",
            "--token2",
            "JET_API_TOKEN",
            "PERSISTENCE_SECRET",
            "digestKey",
            "replayToken",
        ] {
            assert!(is_secret_name(name), "secret name not recognized: {name}");
        }
        assert!(is_secret_environment_name("MY_APIKEY"));
        assert!(!is_secret_name("--public"));

        let first = vec![
            "check".to_string(),
            "--apiToken=camel-secret".to_string(),
            "--auth.token".to_string(),
            "dotted-secret".to_string(),
            "--persistenceToken".to_string(),
            "persistence-secret".to_string(),
            "--digest.key=digest-secret".to_string(),
            "--replayToken".to_string(),
            "replay-secret".to_string(),
            "public".to_string(),
        ];
        let second = first
            .iter()
            .map(|argument| argument.replace("secret", "rotated"))
            .collect::<Vec<_>>();
        assert_eq!(argv_identity(&first), argv_identity(&second));
        assert_eq!(
            context_identity("check", &first).unwrap(),
            context_identity("check", &second).unwrap()
        );
        for value in [
            "camel-secret",
            "dotted-secret",
            "persistence-secret",
            "digest-secret",
            "replay-secret",
        ] {
            assert!(receipt_secret_values(&first)
                .iter()
                .any(|found| found == value));
        }
    }

    #[test]
    fn replay_redacts_known_secret_values() {
        let argv = vec![
            "check".to_string(),
            "--replayToken".to_string(),
            "legacy-replay-secret".to_string(),
        ];
        let secrets = receipt_secret_values(&argv);
        let receipt = Receipt {
            claim: ReceiptClaim {
                verb: "check".into(),
                key: "a".repeat(DIGEST_LEN),
                inputs: Vec::new(),
            },
            status: 0,
            stdout: b"old legacy-replay-secret\0".to_vec(),
            stderr: b"legacy-replay-secret\n".to_vec(),
            digest: "b".repeat(DIGEST_LEN),
        };
        let (stdout, stderr) = replay_output(&receipt, &secrets);
        assert_eq!(stdout, b"old <redacted>\0");
        assert_eq!(stderr, b"<redacted>\n");
    }

    #[test]
    fn receipt_persistence_redacts_secret_values_before_digest() {
        let root = std::env::temp_dir().join(format!(
            "jet-receipt-redaction-{}-{}",
            std::process::id(),
            NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        let store = ReceiptStore::new(&root);
        let claim = ReceiptClaim {
            verb: "check".into(),
            key: "a".repeat(DIGEST_LEN),
            inputs: Vec::new(),
        };
        let argv = vec![
            "check".to_string(),
            "--apiToken=receipt-api-token".to_string(),
            "--auth.token".to_string(),
            "receipt-dotted-secret".to_string(),
            "--persistenceToken".to_string(),
            "receipt-persistence-secret".to_string(),
            "--digest.key=receipt-digest-secret".to_string(),
            "--replayToken".to_string(),
            "receipt-hostile-secret".to_string(),
        ];
        let secret_values = receipt_secret_values(&argv);
        for value in [
            "receipt-api-token",
            "receipt-dotted-secret",
            "receipt-persistence-secret",
            "receipt-digest-secret",
            "receipt-hostile-secret",
        ] {
            assert!(secret_values.iter().any(|found| found == value));
        }

        store
            .write(
                &claim,
                &argv,
                0,
                b"public receipt-api-token receipt-dotted-secret receipt-persistence-secret receipt-digest-secret receipt-hostile-secret\0",
                b"receipt-hostile-secret receipt-digest-secret receipt-persistence-secret receipt-dotted-secret receipt-api-token\n",
            )
            .unwrap();
        let receipt = store.lookup(&claim).unwrap().unwrap();
        assert_eq!(
            receipt.stdout,
            b"public <redacted> <redacted> <redacted> <redacted> <redacted>\0"
        );
        assert_eq!(
            receipt.stderr,
            b"<redacted> <redacted> <redacted> <redacted> <redacted>\n"
        );
        assert_eq!(receipt.digest, receipt_digest(&receipt));
        assert_eq!(
            redact_bytes(b"receipt-hostile-secret", &secret_values),
            b"<redacted>"
        );
        assert_eq!(redact_bytes(b"short", &["longer-secret".into()]), b"short");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn legacy_v1_receipts_with_unknown_rotated_and_file_secrets_fail_closed() {
        let root = std::env::temp_dir().join(format!(
            "jet-receipt-legacy-{}-{}",
            std::process::id(),
            NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        let file_secret = root.join("token.secret");
        fs::create_dir_all(&root).unwrap();
        fs::write(&file_secret, b"file-secret-value").unwrap();
        let file_secret_value = fs::read_to_string(&file_secret).unwrap();
        let store = ReceiptStore::new(&root);
        let claim = ReceiptClaim {
            verb: "check".into(),
            key: "a".repeat(DIGEST_LEN),
            inputs: Vec::new(),
        };
        let receipt = Receipt {
            claim: claim.clone(),
            status: 0,
            stdout: format!(
                "legacy rotated-secret {} from {}",
                file_secret_value,
                file_secret.display(),
            )
            .into_bytes(),
            stderr: b"unknown-secret".to_vec(),
            digest: String::new(),
        };
        let digest = receipt_digest(&receipt);
        let mut bytes = encode_receipt(&Receipt { digest, ..receipt }).unwrap();
        let legacy_magic = b"jet-receipt-v1\0";
        assert_eq!(legacy_magic.len(), MAGIC.len());
        bytes[..MAGIC.len()].copy_from_slice(legacy_magic);
        assert!(decode_receipt(&bytes).is_err());
        fs::create_dir_all(root.join("objects")).unwrap();
        fs::write(store.object_path(&claim.key), bytes).unwrap();

        assert!(store.lookup(&claim).unwrap().is_none());
        assert!(store.list().unwrap().is_empty());
        let _ = fs::remove_dir_all(root);
    }
}
