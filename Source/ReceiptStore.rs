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
        let key = String::from_utf8(pointer_bytes)
            .map_err(|_| "receipt context is not UTF-8".to_string())?;
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
        let input_paths = match target_path(verb, argv, cwd) {
            Some(target) if target.is_dir() || verb == "budget check" => {
                input_paths_for(verb, argv, cwd)
            }
            _ => receipt
                .claim
                .inputs
                .iter()
                .map(|input| input.path.clone())
                .collect(),
        };
        let claim = match self.claim_with_identity(verb, identity, &input_paths) {
            Ok(claim) => claim,
            Err(_) => return Ok(None),
        };
        if claim != receipt.claim
            || !inputs_current(&claim.inputs)
            || receipt.digest != receipt_digest(&receipt)
        {
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

    /// Record one command result through the canonical claim/write path and
    /// return the published receipt. Output is opaque to the store; its input
    /// closure is not.
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
        self.write(&claim, status, stdout, stderr)?;
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
    let root = receipt_root(verb, argv, &cwd);
    let store = ReceiptStore::new(root);
    if let Ok(Some(receipt)) = store.lookup_context(verb, argv, &cwd) {
        replay_receipt(verb, &receipt);
        return Some(receipt.status);
    }

    let input_paths = input_paths_for(verb, argv, &cwd);
    if input_paths.is_empty() && verb != "budget check" {
        return None;
    }
    let claim = store.claim(verb, argv, &input_paths).ok()?;

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
    if store
        .write(&claim, status, &output.stdout, &output.stderr)
        .is_ok()
    {
        let _ = store.remember_context(verb, argv, &claim);
    }
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

/// Locate the project receipt store without running the act it stores.
pub fn receipt_root_for(verb: &str, argv: &[String], cwd: &Path) -> PathBuf {
    receipt_root(verb, argv, cwd)
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
    frame(&mut identity, &tool_identity());
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
