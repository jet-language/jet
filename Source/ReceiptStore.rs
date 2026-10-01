//! Content-addressed receipts for deterministic development acts.
//!
//! D-DEVR-TWICE1=A: `check`, `build`, `test`, `prove`, and `budget check`
//! consult one local receipt store before doing work. Receipt identity uses
//! input bytes and invocation context; filesystem timestamps never participate.
//! Result payloads use the fixed codec fields; an explicit bounded extension
//! carries named canonical typed sections without changing claim identity.

#[path = "ReceiptSections.rs"]
mod ReceiptSections;

pub use ReceiptSections::{
    decode_section_record, diff_projection as receipt_section_diff_projection,
    encode_section_record, diff_sections, normalized_sections, payload_digest, query_sections,
    schema_digest_for_type, section_record_identity, ReceiptFieldDiff, ReceiptSection,
    ReceiptSectionRecord, MAX_RECEIPT_SECTION_BYTES, MAX_RECEIPT_SECTION_BYTES_TOTAL,
    MAX_RECEIPT_SECTION_NAME_BYTES, MAX_RECEIPT_SECTION_RECORD_BYTES,
    MAX_RECEIPT_SECTION_TYPE_BYTES, MAX_RECEIPT_SECTIONS, RECEIPT_SECTION_RECORD_ID_MAGIC,
    RECEIPT_SECTION_RECORD_WIRE_MAGIC, RECEIPT_SECTION_WIRE_MAGIC,
};

use jet_foundation::MIROptimization::Acceleration::{
    AccelerationDecision, ACCELERATION_RECEIPT_SCHEMA_DIGEST, ACCELERATION_RECEIPT_SECTION_PREFIX,
    ACCELERATION_RECEIPT_TYPE_NAME,
};

use crate::RecordIndex::{
    RecordCapture, RecordIdentity, RecordIndex, RecordIndexEntry, RecordKind, RecordLink,
};
use crate::SHA256::sha256_hex;
use jet_devserver::WatchService::WatchGraph;
use jet_foundation::JSON::json_escape;
use jet_foundation::PerformanceBudget::CanonicalJson;
use jet_foundation::TestingComparison::ComparisonRecord;
use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{LazyLock, Mutex};

const OPTIONAL_HISTORY_QUEUE_CAPACITY: usize = 8;
const OPTIONAL_RECEIPT_HELPER_ARG: &str = "__jet_receipt_persist";
const OPTIONAL_RECEIPT_JOB_MAGIC: &[u8] = b"jet-optional-receipt-job-v1\0";
const OPTIONAL_RECEIPT_JOB_MAX_BYTES: u64 = MAX_RECEIPT_BYTES + 8 * 1024 * 1024;
const OPTIONAL_RECEIPT_JOB_MAX_ARGS: u64 = 4096;
const OPTIONAL_RECEIPT_JOB_MAX_PATH_BYTES: usize = 4096;
const OPTIONAL_RECEIPT_JOB_DIR: &str = ".pending";
type OptionalHistoryJob = (String, Box<dyn FnOnce() + Send + 'static>);
static OPTIONAL_HISTORY_QUEUE: LazyLock<Option<SyncSender<OptionalHistoryJob>>> =
    LazyLock::new(|| {
        let (sender, receiver): (
            SyncSender<OptionalHistoryJob>,
            mpsc::Receiver<OptionalHistoryJob>,
        ) = mpsc::sync_channel(OPTIONAL_HISTORY_QUEUE_CAPACITY);
        let worker = std::thread::Builder::new()
            .name("jet-optional-history".into())
            .spawn(move || {
                while let Ok((operation, job)) = receiver.recv() {
                    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)).is_err() {
                        optional_history_notice(
                            &operation,
                            "optional history worker failed while persisting this result",
                        );
                    }
                }
            });
        worker.ok().map(|_| sender)
    });
const RECEIPT_LINK_WIRE_MAGIC: &[u8] = b"jet-receipt-links-v1\0";
const RECEIPT_LINK_VERSION: u64 = 1;
const MAX_RECEIPT_LINKS: u64 = 100_000;
const MAGIC: &[u8] = b"jet-receipt-v2\0";
const DIGEST_LEN: usize = 64;
const MAX_FIELD: u64 = 64 * 1024 * 1024;
const MAX_RECEIPT_BYTES: u64 = MAX_FIELD * 2 + 4 * 1024 * 1024;
const RECEIPT_SECTION_DIR: &str = "sections";
const RECEIPT_SECTION_STAGE_DIR: &str = "staged";
/// Set only by the outer receipt runner after its receipt row is reserved. The
/// prove child uses this claim key for its produced `receipt` link; keeping it
/// separate from the general receipt claim environment prevents an untrusted
/// ambient value from creating a dangling record edge.
pub const JET_RECEIPT_RECORD_CLAIM_ENV: &str = "JET_RECEIPT_RECORD_CLAIM";
const RECEIPT_RECORD_ENGINE: &str = "jet-receipt-v2";
pub const JET_RECEIPT_DIR_ENV: &str = "JET_RECEIPT_DIR";
pub const JET_RECEIPT_CLAIM_ENV: &str = "JET_RECEIPT_CLAIM";
pub const JET_RECEIPT_DIGEST_ENV: &str = "JET_RECEIPT_DIGEST";
const CAPTURE_TRUNCATION_MARKER: &[u8] = b"\n<output truncated>\n";
const RECEIPT_SECRET_NAME_PARTS: &[&str] =
    &["secret", "token", "password", "passwd", "credential", "key"];
const REDACTION_MARKER: &[u8] = b"<redacted>";
static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

/// Whole-invocation receipt participants are deterministic verdict/build acts.
/// `run`/`dev` already reuse their compile actions through the tier caches,
/// and `check` and `build` answer from their typed Receipt node
/// (D-BUILD-NOCHANGE1); mutation and interactive verbs do not claim a
/// whole-invocation receipt.
pub const PARTICIPATING_VERBS: &[&str] = &["test", "prove", "budget check"];

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

#[derive(Clone, Debug)]
struct OptionalReceiptJob {
    project_root: PathBuf,
    cwd: PathBuf,
    context_key: String,
    claim: ReceiptClaim,
    argv: Vec<String>,
    status: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}



#[derive(Clone, PartialEq, Eq)]
pub struct Receipt {
    pub claim: ReceiptClaim,
    pub status: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub sections: Vec<ReceiptSection>,
    /// Versioned typed links are optional, so receipts without links retain
    /// their exact pre-index bytes.
    pub consumed: Vec<RecordLink>,
    pub produced: Vec<RecordLink>,
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
            .field("sections", &self.sections)
            .field("consumed", &self.consumed)
            .field("produced", &self.produced)
            .field("digest", &self.digest)
            .finish()
    }
}

impl Receipt {
    /// Find one named section without exposing any mutable post-publication
    /// path. A receipt object is immutable once written to the store.
    pub fn section(&self, name: &str) -> Result<Option<&ReceiptSection>, String> {
        if name.is_empty()
            || name.len() > MAX_RECEIPT_SECTION_NAME_BYTES
            || !name
                .chars()
                .all(|character| !character.is_control() && character != '/' && character != '\\')
        {
            return Err(format!("receipt section name `{name}` is invalid"));
        }
        Ok(self.sections.iter().find(|section| section.name == name))
    }

    /// Project all sections using the shared deterministic canonical object.
    pub fn sections_json(&self) -> Result<CanonicalJson, String> {
        ReceiptSections::query_sections(&self.sections, None)
    }

    /// Project selected sections using the shared deterministic canonical
    /// object. An empty list selects all sections.
    pub fn query_sections(
        &self,
        names: Option<&[String]>,
    ) -> Result<CanonicalJson, String> {
        ReceiptSections::query_sections(&self.sections, names)
    }

    /// Return deterministic per-field changes against an earlier receipt.
    pub fn diff_sections(&self, before: &Receipt) -> Result<Vec<ReceiptFieldDiff>, String> {
        ReceiptSections::diff_sections(&before.sections, &self.sections)
    }

    /// Canonical JSON array projection for receipt diff and perf compare.
    pub fn diff_sections_json(&self, before: &Receipt) -> Result<CanonicalJson, String> {
        Ok(receipt_section_diff_projection(&self.diff_sections(before)?))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccelerationReceiptRow {
    pub sequence: u64,
    pub function: String,
    pub loop_header: u32,
    pub source_start: usize,
    pub source_end: usize,
    pub decision: AccelerationDecision,
}

/// The payload carries source identity beside the canonical Foundation
/// `AccelerationDecision` object; display text is never consulted. Rows are
/// returned in numeric sequence order, independent of receipt section-name
/// sorting.
pub fn acceleration_decisions(
    receipt: &Receipt,
) -> Result<Vec<AccelerationReceiptRow>, String> {
    let mut rows = Vec::new();
    for section in &receipt.sections {
        if section.type_name != ACCELERATION_RECEIPT_TYPE_NAME
            || !section
                .name
                .starts_with(ACCELERATION_RECEIPT_SECTION_PREFIX)
        {
            continue;
        }
        if section.schema_digest != ACCELERATION_RECEIPT_SCHEMA_DIGEST {
            return Err(format!(
                "acceleration section `{}` has an unexpected schema digest",
                section.name
            ));
        }
        let sequence = section
            .name
            .strip_prefix(ACCELERATION_RECEIPT_SECTION_PREFIX)
            .and_then(|value| (!value.is_empty()).then_some(value))
            .ok_or_else(|| format!("invalid acceleration section name `{}`", section.name))?
            .parse::<u64>()
            .map_err(|_| format!("invalid acceleration section sequence `{}`", section.name))?;
        let value = section.value()?;
        let fields = receipt_acceleration_object(&value, "acceleration receipt")?;
        receipt_acceleration_require_fields(
            fields,
            "acceleration receipt",
            &["decision", "function", "loop_header", "source"],
        )?;
        let function = receipt_acceleration_string(fields, "function")?.to_string();
        let loop_header = receipt_acceleration_integer(fields, "loop_header")?
            .parse::<u32>()
            .map_err(|_| "acceleration receipt loop_header is out of range".to_string())?;
        let source = receipt_acceleration_object(
            fields
                .get("source")
                .ok_or("acceleration receipt is missing source")?,
            "acceleration receipt source",
        )?;
        receipt_acceleration_require_fields(
            source,
            "acceleration receipt source",
            &["end", "start"],
        )?;
        let source_start = receipt_acceleration_integer(source, "start")?
            .parse::<usize>()
            .map_err(|_| "acceleration receipt source start is out of range".to_string())?;
        let source_end = receipt_acceleration_integer(source, "end")?
            .parse::<usize>()
            .map_err(|_| "acceleration receipt source end is out of range".to_string())?;
        if source_start > source_end {
            return Err("acceleration receipt source range is reversed".to_string());
        }
        let decision = AccelerationDecision::from_canonical_json(
            fields
                .get("decision")
                .ok_or("acceleration receipt is missing decision")?,
        )?;
        rows.push(AccelerationReceiptRow {
            sequence,
            function,
            loop_header,
            source_start,
            source_end,
            decision,
        });
    }
    rows.sort_by_key(|row| row.sequence);
    if rows
        .windows(2)
        .any(|pair| pair[0].sequence == pair[1].sequence)
    {
        return Err("duplicate acceleration receipt sequence".to_string());
    }
    Ok(rows)
}

fn receipt_acceleration_object<'a>(
    value: &'a CanonicalJson,
    label: &str,
) -> Result<&'a std::collections::BTreeMap<String, CanonicalJson>, String> {
    match value {
        CanonicalJson::Object(fields) => Ok(fields),
        _ => Err(format!("{label} must be a canonical object")),
    }
}

fn receipt_acceleration_string<'a>(
    fields: &'a std::collections::BTreeMap<String, CanonicalJson>,
    key: &str,
) -> Result<&'a str, String> {
    match fields.get(key) {
        Some(CanonicalJson::String(value)) => Ok(value),
        Some(_) => Err(format!("acceleration receipt field `{key}` must be a string")),
        None => Err(format!("acceleration receipt field `{key}` is missing")),
    }
}

fn receipt_acceleration_integer<'a>(
    fields: &'a std::collections::BTreeMap<String, CanonicalJson>,
    key: &str,
) -> Result<&'a str, String> {
    match fields.get(key) {
        Some(CanonicalJson::Integer(value)) => Ok(value),
        Some(_) => Err(format!("acceleration receipt field `{key}` must be an integer")),
        None => Err(format!("acceleration receipt field `{key}` is missing")),
    }
}

fn receipt_acceleration_require_fields(
    fields: &std::collections::BTreeMap<String, CanonicalJson>,
    label: &str,
    keys: &[&str],
) -> Result<(), String> {
    if fields.len() != keys.len() || !keys.iter().all(|key| fields.contains_key(*key)) {
        return Err(format!("{label} has missing or unknown fields"));
    }
    Ok(())
}

/// Decode one authenticated `jet-receipt-v2` object from bytes. Command
/// boundaries use this instead of reaching into the private codec so indexed
/// verification cannot consume a forged or stale payload.
pub(crate) fn decode_bytes(bytes: &[u8]) -> Result<Receipt, String> {
    let receipt = decode_receipt(bytes)?;
    if receipt.digest != receipt_digest(&receipt) {
        return Err("receipt digest does not authenticate its body".into());
    }
    Ok(receipt)
}

/// Read one authenticated receipt object without following a symlink.
pub fn read_path(path: &Path) -> Result<Receipt, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("could not inspect receipt `{}`: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "receipt path is not a regular file: {}",
            path.display()
        ));
    }
    let bytes = read_regular(path)
        .map_err(|error| format!("could not read receipt `{}`: {error}", path.display()))?;
    decode_bytes(&bytes)
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
    fn claim_with_attempt(
        &self,
        verb: &str,
        argv: &[String],
        input_paths: &[PathBuf],
        attempt: &[u8],
    ) -> Result<ReceiptClaim, String> {
        let mut identity = context_identity(verb, argv)?;
        frame(&mut identity, b"generated-replay-attempt-v1");
        frame(&mut identity, attempt);
        self.claim_with_identity(verb, identity, input_paths)
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
        if matches!(verb, "build" | "prove") {
            let _ = receipt_target_inputs_sha256(&receipt.claim, argv, cwd)?;
        }
        if verb == "prove" && has_generated_failure_for_target(&receipt, argv, cwd)? {
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
                        add_project_inputs(&target, verb, &mut paths);
                    }
                    paths.into_iter().collect()
                }
            }
        };
        let claim = match self.claim_with_identity(verb, identity, &input_paths) {
            Ok(claim) => claim,
            Err(_) => return Ok(None),
        };
        let generated_replay_receipt =
            verb == "prove" && claim.inputs == receipt.claim.inputs;
        if claim != receipt.claim && !generated_replay_receipt {
            if receipt_notices_visible(verb, argv) {
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
            }
            return Ok(None);
        }
        if let Some(path) = stale_receipt_input(&claim.inputs) {
            if receipt_notices_visible(verb, argv) {
                eprintln!(
                    "receipt: {verb} invalidated (input changed: `{}`)",
                    path.display()
                );
            }
            return Ok(None);
        }
        if receipt.digest != receipt_digest(&receipt) {
            if receipt_notices_visible(verb, argv) {
                eprintln!("receipt: {verb} invalidated (receipt authentication changed)");
            }
            return Ok(None);
        }
        Ok(Some(receipt))
    }


    fn remember_context_key(
        &self,
        context_key: &str,
        claim: &ReceiptClaim,
    ) -> Result<(), String> {
        if !is_digest(context_key) || claim.verb.is_empty() || !is_digest(&claim.key) {
            return Err("receipt context claim is malformed".into());
        }
        let path = self.context_path(context_key);
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
        let mut receipt = match decode_receipt(&bytes) {
            Ok(receipt) => receipt,
            Err(_) => return Ok(None),
        };
        // The immutable object authenticates itself before any append-only
        // section records are projected into the caller-visible receipt.
        if receipt.claim != *claim || receipt.digest != receipt_digest(&receipt) {
            return Ok(None);
        }
        let appended = self.published_sections(&receipt.claim.key, &receipt.digest)?;
        for section in appended {
            let same = receipt.sections.iter().any(|existing| {
                existing.name == section.name
                    && existing.type_name == section.type_name
                    && existing.schema_digest == section.schema_digest
                    && existing.payload_digest == section.payload_digest
            });
            if same {
                continue;
            }
            if receipt
                .sections
                .iter()
                .any(|existing| existing.name == section.name)
            {
                return Err(format!(
                    "receipt section `{}` conflicts with the immutable receipt",
                    section.name
                ));
            }
            receipt.sections.push(section);
        }
        receipt.sections = match normalized_sections(&receipt.sections) {
            Ok(sections) => sections,
            Err(error) => return Err(error),
        };
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
        self.write_with_sections(claim, argv, status, stdout, stderr, &[])
    }

    /// Publish one immutable receipt object and its bounded named sections.
    /// Section bytes and typed links are authenticated by the receipt digest,
    /// but neither participates in the immutable claim key.
    pub fn write_with_sections(
        &self,
        claim: &ReceiptClaim,
        argv: &[String],
        status: i32,
        stdout: &[u8],
        stderr: &[u8],
        sections: &[ReceiptSection],
    ) -> Result<bool, String> {
        self.write_with_sections_and_links(
            claim,
            argv,
            status,
            stdout,
            stderr,
            sections,
            &[],
            &[],
        )
    }

    /// Publish one immutable receipt with versioned consumed/produced links.
    /// Empty links take the pre-index wire path, preserving old bytes.
    pub fn write_with_sections_and_links(
        &self,
        claim: &ReceiptClaim,
        argv: &[String],
        status: i32,
        stdout: &[u8],
        stderr: &[u8],
        sections: &[ReceiptSection],
        consumed: &[RecordLink],
        produced: &[RecordLink],
    ) -> Result<bool, String> {
        let secret_values = receipt_secret_values(argv);
        if !is_digest(&claim.key) {
            return Err("receipt claim key is not a lowercase SHA-256 digest".into());
        }
        let sections = normalized_sections(sections)?;
        let consumed = normalized_links(consumed, "consumed")?;
        let produced = normalized_links(produced, "produced")?;
        if !inputs_current(&claim.inputs) {
            return Ok(false);
        }
        let receipt = Receipt {
            claim: claim.clone(),
            status,
            stdout: bounded_redact_bytes(stdout, &secret_values)?,
            stderr: bounded_redact_bytes(stderr, &secret_values)?,
            sections,
            consumed,
            produced,
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
        self.record_with_sections(verb, argv, input_paths, status, stdout, stderr, &[])
    }

    /// Record one command result with named typed sections.
    pub fn record_with_sections(
        &self,
        verb: &str,
        argv: &[String],
        input_paths: &[PathBuf],
        status: i32,
        stdout: &[u8],
        stderr: &[u8],
        sections: &[ReceiptSection],
    ) -> Result<Receipt, String> {
        self.record_with_sections_and_links(
            verb,
            argv,
            input_paths,
            status,
            stdout,
            stderr,
            sections,
            &[],
            &[],
        )
    }

    /// Record one command result and attach typed consumed/produced links.
    pub fn record_with_sections_and_links(
        &self,
        verb: &str,
        argv: &[String],
        input_paths: &[PathBuf],
        status: i32,
        stdout: &[u8],
        stderr: &[u8],
        sections: &[ReceiptSection],
        consumed: &[RecordLink],
        produced: &[RecordLink],
    ) -> Result<Receipt, String> {
        let claim = self.claim(verb, argv, input_paths)?;
        self.write_with_sections_and_links(
            &claim,
            argv,
            status,
            stdout,
            stderr,
            sections,
            consumed,
            produced,
        )?;
        let receipt = self
            .lookup(&claim)?
            .ok_or_else(|| "receipt was not current after publication".to_string())?;
        if let Ok(cwd) = std::env::current_dir() {
            index_published_receipt(self, &receipt, argv, &cwd)?;
        }
        Ok(receipt)
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
            let Ok(mut receipt) = decode_receipt(&bytes) else {
                continue;
            };
            if receipt.claim.key != name || receipt.digest != receipt_digest(&receipt) {
                continue;
            }
            let Ok(appended) = self.published_sections(&receipt.claim.key, &receipt.digest) else {
                continue;
            };
            receipt.sections.extend(appended);
            let Ok(sections) = normalized_sections(&receipt.sections) else {
                continue;
            };
            receipt.sections = sections;
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

    /// Query all or selected named sections through the shared projection.
    pub fn query_sections(
        &self,
        receipt: &Receipt,
        names: Option<&[String]>,
    ) -> Result<CanonicalJson, String> {
        if let Some(names) = names {
            for name in names {
                receipt.section(name)?;
            }
        }
        receipt.query_sections(names)
    }

    /// Diff named sections through the shared deterministic field projection.
    pub fn diff_sections(
        &self,
        before: &Receipt,
        after: &Receipt,
    ) -> Result<Vec<ReceiptFieldDiff>, String> {
        after.diff_sections(before)
    }

    /// Canonical JSON array projection for receipt diff and perf compare.
    pub fn diff_sections_json(
        &self,
        before: &Receipt,
        after: &Receipt,
    ) -> Result<CanonicalJson, String> {
        after.diff_sections_json(before)
    }

    /// Move child-process staged section records under the immutable parent
    /// digest once the command output has made that digest knowable.
    pub fn adopt_staged_sections(&self, claim: &ReceiptClaim) -> Result<(), String> {
        let Some(receipt) = self.lookup(claim)? else {
            return Err("cannot adopt receipt sections before parent publication".into());
        };
        let stage_dir = self.section_stage_path(&claim.key);
        let staged = read_section_records(&stage_dir, Some(&claim.key), Some(""))?;
        if staged.is_empty() {
            return Ok(());
        }

        let mut existing = receipt.sections.clone();
        let mut pending = Vec::new();
        for (_, record) in &staged {
            if let Some(section) = existing.iter().find(|section| {
                section.name == record.section.name
                    && section.type_name == record.section.type_name
                    && section.schema_digest == record.section.schema_digest
                    && section.payload_digest == record.section.payload_digest
            }) {
                let _ = section;
                continue;
            }
            if existing
                .iter()
                .any(|section| section.name == record.section.name)
                || pending
                    .iter()
                    .any(|section: &ReceiptSection| section.name == record.section.name)
            {
                return Err(format!(
                    "receipt section `{}` conflicts with an existing section",
                    record.section.name
                ));
            }
            pending.push(record.section.clone());
        }
        existing.extend(pending);
        normalized_sections(&existing)?;

        for (path, record) in staged {
            self.append_section_record(&claim.key, &receipt.digest, record.section)?;
            fs::remove_file(&path)
                .map_err(|error| format!("could not remove staged receipt section: {error}"))?;
        }
        let _ = fs::remove_dir(&stage_dir);
        Ok(())
    }

    fn published_sections(
        &self,
        claim_key: &str,
        parent_digest: &str,
    ) -> Result<Vec<ReceiptSection>, String> {
        let records = read_section_records(
            &self.section_path(claim_key),
            Some(claim_key),
            Some(parent_digest),
        )?;
        Ok(records
            .into_iter()
            .map(|(_, record)| record.section)
            .collect())
    }

    fn append_section_record(
        &self,
        claim_key: &str,
        parent_digest: &str,
        section: ReceiptSection,
    ) -> Result<bool, String> {
        let record = ReceiptSectionRecord::new(claim_key, parent_digest, section)?;
        let id = section_record_identity(&record);
        let bytes = encode_section_record(&record)?;
        publish_immutable_bytes(&self.section_path(claim_key).join(id), &bytes)
    }

    fn section_path(&self, claim_key: &str) -> PathBuf {
        self.root.join(RECEIPT_SECTION_DIR).join(claim_key)
    }

    fn section_stage_path(&self, claim_key: &str) -> PathBuf {
        self.root
            .join(RECEIPT_SECTION_STAGE_DIR)
            .join(claim_key)
    }


    pub fn object_path(&self, key: &str) -> PathBuf {
        self.root.join("objects").join(key)
    }
}
fn read_section_records(
    directory: &Path,
    expected_claim: Option<&str>,
    expected_parent: Option<&str>,
) -> Result<Vec<(PathBuf, ReceiptSectionRecord)>, String> {
    let metadata = match fs::symlink_metadata(directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(format!(
                "could not inspect receipt section directory {}: {error}",
                directory.display()
            ))
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "receipt section directory is not a regular directory: {}",
            directory.display()
        ));
    }

    let entries = fs::read_dir(directory)
        .map_err(|error| format!("could not list receipt sections: {error}"))?;
    let mut records = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("could not inspect receipt section: {error}"))?;
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if name.is_empty() || name.starts_with('.') {
            continue;
        }
        if !is_digest(name) {
            return Err(format!(
                "receipt section record has a non-digest name: {}",
                path.display()
            ));
        }
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("could not inspect receipt section: {error}"))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(format!(
                "receipt section record is not a regular file: {}",
                path.display()
            ));
        }
        let bytes = read_regular(&path)
            .map_err(|error| format!("could not read receipt section {}: {error}", path.display()))?;
        let record = decode_section_record(&bytes)?;
        if section_record_identity(&record) != name {
            return Err(format!(
                "receipt section record identity does not match its path: {}",
                path.display()
            ));
        }
        if expected_claim.is_some_and(|claim| record.claim_key != claim)
            || expected_parent.is_some_and(|parent| record.parent_digest != parent)
        {
            return Err(format!(
                "receipt section record belongs to a different receipt: {}",
                path.display()
            ));
        }
        records.push((path, record));
    }
    records.sort_by(|left, right| {
        left.0
            .file_name()
            .and_then(|name| name.to_str())
            .cmp(&right.0.file_name().and_then(|name| name.to_str()))
    });
    let sections = records
        .iter()
        .map(|(_, record)| record.section.clone())
        .collect::<Vec<_>>();
    normalized_sections(&sections)?;
    Ok(records)
}

fn publish_immutable_bytes(path: &Path, bytes: &[u8]) -> Result<bool, String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("receipt section path has no parent: {}", path.display()))?;
    secure_create_dir(parent)?;
    let temp = parent.join(format!(
        ".{}.{}.{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("section"),
        std::process::id(),
        NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|error| format!("could not stage receipt section: {error}"))?;
        file.write_all(bytes)
            .map_err(|error| format!("could not write receipt section: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("could not flush receipt section: {error}"))?;
        match fs::hard_link(&temp, path) {
            Ok(()) => {
                fs::remove_file(&temp)
                    .map_err(|error| format!("could not remove staged receipt section: {error}"))?;
                sync_directory(parent)
                    .map_err(|error| format!("could not flush receipt section directory: {error}"))?;
                Ok(true)
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let existing = read_regular(path)
                    .map_err(|read_error| format!("could not inspect receipt section: {read_error}"))?;
                let _ = fs::remove_file(&temp);
                if existing == bytes {
                    Ok(false)
                } else {
                    Err("receipt section identity collision with different content".into())
                }
            }
            Err(error) => Err(format!("could not publish receipt section: {error}")),
        }
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
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
        add_project_inputs(&target, verb, &mut paths);
    } else {
        return Vec::new();
    }
    if verb == "prove" {
        let start = if target.is_dir() {
            target.as_path()
        } else {
            target.parent().unwrap_or(cwd)
        };
        for root in receipt_authority_roots(start) {
            collect_generated_failure_inputs(
                &root.join(".jet").join("records").join("generated"),
                &mut paths,
            );
        }
    }
    paths
        .into_iter()
        .filter(|path| regular_file(path))
        .collect()
}

/// Corrective actions already reported by this process. Optional failures
/// that share one fix merge into the first notice, so a run whose history
/// store is unusable prints one line, not one per history write (#3416).
static OPTIONAL_HISTORY_NOTICES: LazyLock<Mutex<BTreeSet<&'static str>>> =
    LazyLock::new(|| Mutex::new(BTreeSet::new()));

pub fn optional_history_notice(operation: &str, error: &str) {
    let lower = error.to_ascii_lowercase();
    let fix = if lower.contains("permission") || lower.contains("access denied") {
        "check `.jet` history permissions"
    } else if lower.contains("no space")
        || lower.contains("disk full")
        || lower.contains("quota")
    {
        "free `.jet` history storage or raise its quota"
    } else if lower.contains("queue") || lower.contains("worker") {
        "retry after optional history drains or repair `.jet` history storage"
    } else if lower.contains("budget") || lower.contains("exceeds") {
        "raise the history byte budget or remove disposable history"
    } else {
        "check `.jet` history storage and retry"
    };
    let first = OPTIONAL_HISTORY_NOTICES
        .lock()
        .map_or(true, |mut seen| seen.insert(fix));
    if first {
        eprintln!("history: optional persistence failed while {operation}: {error}; fix: {fix}");
    }
}

fn optional_receipt_project_root(store_root: &Path) -> Result<PathBuf, String> {
    if !store_root.is_absolute() {
        return Err("optional receipt store root must be absolute".into());
    }
    if store_root.file_name().and_then(|name| name.to_str()) != Some("receipts") {
        return Err("optional receipt store is not the canonical `.jet/receipts` root".into());
    }
    let jet_root = store_root
        .parent()
        .filter(|path| path.file_name().and_then(|name| name.to_str()) == Some(".jet"))
        .ok_or_else(|| "optional receipt store is not under `.jet`".to_string())?;
    let project_root = fs::canonicalize(
        jet_root
            .parent()
            .ok_or_else(|| "optional receipt store has no project root".to_string())?,
    )
    .map_err(|error| format!("could not canonicalize optional receipt project root: {error}"))?;
    let selected = crate::Loader::selected_project_root(&project_root)
        .map_err(|diagnostic| format!("could not validate optional receipt project root: {diagnostic:?}"))?;
    if selected != project_root {
        return Err(format!(
            "optional receipt root is not the selected project root: {}",
            project_root.display()
        ));
    }
    let jet_dir = project_root.join(".jet");
    let expected = jet_dir.join("receipts");
    let root_metadata = fs::symlink_metadata(&project_root)
        .map_err(|error| format!("could not inspect optional receipt project root: {error}"))?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err("optional receipt project root is not a real directory".into());
    }
    match fs::symlink_metadata(&jet_dir) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err("optional `.jet` directory is not a real directory".into());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("could not inspect optional `.jet` directory: {error}")),
    }
    if store_root != expected {
        return Err(format!(
            "optional receipt store must be `{}`",
            expected.display()
        ));
    }
    Ok(project_root)
}

fn optional_receipt_job_directory(store_root: &Path) -> Result<(PathBuf, PathBuf), String> {
    let project_root = optional_receipt_project_root(store_root)?;
    let receipts = project_root.join(".jet").join("receipts");
    secure_create_dir(&receipts)?;
    let pending = receipts.join(OPTIONAL_RECEIPT_JOB_DIR);
    secure_create_dir(&pending)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::symlink_metadata(&pending)
            .map_err(|error| format!("could not inspect optional receipt queue: {error}"))?;
        if metadata.permissions().mode() & 0o077 != 0 {
            fs::set_permissions(&pending, fs::Permissions::from_mode(0o700))
                .map_err(|error| format!("could not secure optional receipt queue: {error}"))?;
        }
    }
    Ok((project_root, pending))
}

fn stage_optional_receipt_job(job: &OptionalReceiptJob) -> Result<PathBuf, String> {
    let (project_root, pending) = optional_receipt_job_directory(
        &job.project_root.join(".jet").join("receipts"),
    )?;
    if project_root != job.project_root {
        return Err("optional receipt job project root changed before staging".into());
    }
    let bytes = encode_optional_receipt_job(job)?;
    let path = pending.join(format!(
        ".job-{}-{}",
        std::process::id(),
        NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| format!("could not stage optional receipt job: {error}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|error| format!("could not secure optional receipt job: {error}"))?;
        }
        // This is deliberately a disposable handoff: the trusted helper
        // validates and consumes it after the parent exits, so no fsync or
        // saved-record claim is taken on the optional path.
        file.write_all(&bytes)
            .map_err(|error| format!("could not write optional receipt job: {error}"))?;
        Ok(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&path);
        return Err(error);
    }
    Ok(path)
}
fn spawn_optional_receipt_helper(job: &OptionalReceiptJob) -> Result<(), String> {
    let path = stage_optional_receipt_job(job)?;
    let result = (|| -> Result<(), String> {
        let executable = std::env::current_exe()
            .map_err(|error| format!("could not locate the Jet executable: {error}"))
            .and_then(|path| {
                fs::canonicalize(path)
                    .map_err(|error| format!("could not canonicalize the Jet executable: {error}"))
            })?;
        let metadata = fs::symlink_metadata(&executable)
            .map_err(|error| format!("could not inspect the Jet executable: {error}"))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err("the current Jet executable is not a regular file".into());
        }
        let path = path
            .to_str()
            .ok_or_else(|| "optional receipt job path is not UTF-8".to_string())?;
        Command::new(executable)
            .current_dir(&job.cwd)
            .arg(OPTIONAL_RECEIPT_HELPER_ARG)
            .arg(path)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("could not start optional receipt helper: {error}"))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&path);
    }
    result
}

fn read_optional_receipt_job(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("could not inspect optional receipt job: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("optional receipt job is not a regular file".into());
    }
    if metadata.len() > limit {
        return Err("optional receipt job exceeds its size limit".into());
    }
    let file = OpenOptions::new()
        .read(true)
        .open(path)
        .map_err(|error| format!("could not open optional receipt job: {error}"))?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("could not read optional receipt job: {error}"))?;
    if bytes.len() as u64 > limit {
        return Err("optional receipt job exceeds its size limit".into());
    }
    Ok(bytes)
}

fn discard_optional_receipt_job(path: &Path) {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return;
    };
    let Some(pending) = path.parent() else {
        return;
    };
    let Some(receipts) = pending.parent() else {
        return;
    };
    let Some(jet_root) = receipts.parent() else {
        return;
    };
    if !path.is_absolute()
        || !file_name.starts_with(".job-")
        || pending.file_name().and_then(|name| name.to_str()) != Some(OPTIONAL_RECEIPT_JOB_DIR)
        || receipts.file_name().and_then(|name| name.to_str()) != Some("receipts")
        || jet_root.file_name().and_then(|name| name.to_str()) != Some(".jet")
    {
        return;
    }
    let Some(project_root) = jet_root.parent() else {
        return;
    };
    for directory in [project_root, jet_root, receipts, pending] {
        let Ok(metadata) = fs::symlink_metadata(directory) else {
            return;
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return;
        }
    }
    if let Err(error) = fs::remove_file(path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            optional_history_notice("removing an invalid optional receipt job", &error.to_string());
        }
    }
}

fn validate_optional_receipt_job_path(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("optional receipt job path must be absolute".into());
    }
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "optional receipt job has no UTF-8 filename".to_string())?;
    if !file_name.starts_with(".job-") {
        return Err("optional receipt job filename is not in the disposable namespace".into());
    }
    let pending = path
        .parent()
        .filter(|parent| parent.file_name().and_then(|name| name.to_str()) == Some(OPTIONAL_RECEIPT_JOB_DIR))
        .ok_or_else(|| "optional receipt job is outside the disposable queue".to_string())?;
    let receipts = pending
        .parent()
        .filter(|parent| parent.file_name().and_then(|name| name.to_str()) == Some("receipts"))
        .ok_or_else(|| "optional receipt job is outside `.jet/receipts`".to_string())?;
    let jet_root = receipts
        .parent()
        .filter(|parent| parent.file_name().and_then(|name| name.to_str()) == Some(".jet"))
        .ok_or_else(|| "optional receipt job is outside `.jet`".to_string())?;
    let project_root = fs::canonicalize(
        jet_root
            .parent()
            .ok_or_else(|| "optional receipt job has no project root".to_string())?,
    )
    .map_err(|error| format!("could not canonicalize optional receipt job root: {error}"))?;
    let selected = crate::Loader::selected_project_root(&project_root)
        .map_err(|diagnostic| format!("could not validate optional receipt job root: {diagnostic:?}"))?;
    if selected != project_root {
        return Err("optional receipt job root is not the selected project root".into());
    }
    let expected = project_root
        .join(".jet")
        .join("receipts")
        .join(OPTIONAL_RECEIPT_JOB_DIR)
        .join(file_name);
    if path != expected {
        return Err("optional receipt job path is not canonical".into());
    }
    let jet_dir = project_root.join(".jet");
    let receipts_dir = jet_dir.join("receipts");
    let pending_dir = receipts_dir.join(OPTIONAL_RECEIPT_JOB_DIR);
    #[cfg(unix)]
    let owner = {
        use std::os::unix::fs::MetadataExt;
        fs::symlink_metadata(&project_root)
            .map_err(|error| format!("could not inspect optional receipt project root: {error}"))?
            .uid()
    };
    for directory in [
        project_root.as_path(),
        jet_dir.as_path(),
        receipts_dir.as_path(),
        pending_dir.as_path(),
    ] {
        let metadata = fs::symlink_metadata(directory)
            .map_err(|error| format!("could not inspect optional receipt job directory: {error}"))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err("optional receipt job directory is not a real directory".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            if metadata.uid() != owner {
                return Err("optional receipt job directory ownership does not match its project".into());
            }
            if directory == pending_dir.as_path() && metadata.permissions().mode() & 0o077 != 0 {
                return Err("optional receipt job queue permissions are too broad".into());
            }
        }
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("could not inspect optional receipt job: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("optional receipt job is not a regular file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != owner {
            return Err("optional receipt job ownership does not match its project".into());
        }
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("optional receipt job permissions are too broad".into());
        }
    }
    if metadata.len() > OPTIONAL_RECEIPT_JOB_MAX_BYTES {
        return Err("optional receipt job exceeds its size limit".into());
    }
    Ok(project_root)
}

/// Consume one staged outer-receipt job. This is only reached through the
/// private parent-spawned helper mode; malformed or hostile jobs are discarded
/// without dispatching any user command.
#[doc(hidden)]
pub fn run_optional_receipt_helper(args: &[String]) -> i32 {
    let Some(path) = args.first().map(PathBuf::from) else {
        optional_history_notice("consuming an optional receipt job", "missing staged job path");
        return 0;
    };
    if args.len() != 1 {
        optional_history_notice("consuming an optional receipt job", "unexpected helper arguments");
        discard_optional_receipt_job(&path);
        return 0;
    }
    let project_root = match validate_optional_receipt_job_path(&path) {
        Ok(root) => root,
        Err(error) => {
            optional_history_notice("consuming an optional receipt job", &error);
            discard_optional_receipt_job(&path);
            return 0;
        }
    };
    let result = (|| -> Result<(), String> {
        let bytes = read_optional_receipt_job(&path, OPTIONAL_RECEIPT_JOB_MAX_BYTES)?;
        let job = decode_optional_receipt_job(&bytes, &project_root)?;
        if job.project_root != project_root {
            return Err("optional receipt job project root does not match its canonical path".into());
        }
        let cwd = fs::canonicalize(&job.cwd)
            .map_err(|error| format!("could not canonicalize optional receipt working directory: {error}"))?;
        if cwd != job.cwd || !cwd.starts_with(&project_root) || !cwd.is_dir() {
            return Err("optional receipt working directory is not a canonical project directory".into());
        }
        let store = ReceiptStore::new(project_root.join(".jet").join("receipts"));
        persist_published_receipt(
            &store,
            &job.claim,
            &job.context_key,
            &job.argv,
            job.status,
            &job.stdout,
            &job.stderr,
            &job.cwd,
        )
    })();
    if let Err(error) = result {
        optional_history_notice("consuming an optional receipt job", &error);
    }
    if let Err(error) = fs::remove_file(&path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            optional_history_notice("removing an optional receipt job", &error.to_string());
        }
    }
    0
}
fn optional_job_frame(output: &mut Vec<u8>, value: &[u8]) -> Result<(), String> {
    if value.len() as u64 > MAX_FIELD {
        return Err("optional receipt job field exceeds the frame limit".into());
    }
    let added = 8usize
        .checked_add(value.len())
        .ok_or_else(|| "optional receipt job size overflows".to_string())?;
    let next = output
        .len()
        .checked_add(added)
        .ok_or_else(|| "optional receipt job size overflows".to_string())?;
    if next as u64 > OPTIONAL_RECEIPT_JOB_MAX_BYTES {
        return Err("optional receipt job exceeds its size limit".into());
    }
    frame(output, value);
    Ok(())
}

fn optional_job_text(value: &[u8], field: &str, max_bytes: usize) -> Result<String, String> {
    if value.is_empty()
        || value.len() > max_bytes
        || value.iter().any(|byte| *byte == 0 || byte.is_ascii_control())
    {
        return Err(format!("optional receipt job {field} is empty, too large, or unsafe"));
    }
    String::from_utf8(value.to_vec())
        .map_err(|_| format!("optional receipt job {field} is not UTF-8"))
}

fn optional_job_u64(bytes: &[u8], cursor: &mut usize, field: &str) -> Result<u64, String> {
    let value = take_frame(bytes, cursor)?;
    if value.len() != 8 {
        return Err(format!("optional receipt job {field} is malformed"));
    }
    Ok(u64::from_be_bytes(
        value
            .as_slice()
            .try_into()
            .map_err(|_| format!("optional receipt job {field} is malformed"))?,
    ))
}

fn redacted_optional_receipt_argv(argv: &[String]) -> Vec<String> {
    let mut redacted = Vec::with_capacity(argv.len());
    let mut redact_next = false;
    for argument in argv {
        if redact_next {
            redacted.push(String::from_utf8_lossy(REDACTION_MARKER).into_owned());
            redact_next = false;
            continue;
        }
        if let Some((name, _)) = argument.split_once('=') {
            if is_secret_name(name) {
                redacted.push(format!(
                    "{name}={}",
                    String::from_utf8_lossy(REDACTION_MARKER)
                ));
                continue;
            }
        }
        redact_next = is_secret_name(argument);
        redacted.push(argument.clone());
    }
    redacted
}

fn encode_optional_receipt_job(job: &OptionalReceiptJob) -> Result<Vec<u8>, String> {
    let root = job
        .project_root
        .to_str()
        .ok_or_else(|| "optional receipt job project root is not UTF-8".to_string())?;
    let cwd = job
        .cwd
        .to_str()
        .ok_or_else(|| "optional receipt job working directory is not UTF-8".to_string())?;
    if !job.project_root.is_absolute() || !job.cwd.is_absolute() {
        return Err("optional receipt job paths must be absolute".into());
    }
    if !job.cwd.starts_with(&job.project_root) {
        return Err("optional receipt job working directory is outside its project root".into());
    }
    if !is_digest(&job.context_key) || !is_digest(&job.claim.key) || job.claim.verb.is_empty() {
        return Err("optional receipt job claim is malformed".into());
    }
    if participating_verb(&job.argv) != Some(job.claim.verb.as_str())
        || !cacheable_invocation(&job.claim.verb, &job.argv)
    {
        return Err("optional receipt job command is not a cacheable Jet act".into());
    }
    if job.claim.inputs.len() as u64 > 100_000 {
        return Err("optional receipt job has too many inputs".into());
    }
    if job.argv.len() as u64 > OPTIONAL_RECEIPT_JOB_MAX_ARGS {
        return Err("optional receipt job has too many arguments".into());
    }
    let mut output = OPTIONAL_RECEIPT_JOB_MAGIC.to_vec();
    optional_job_frame(&mut output, root.as_bytes())?;
    optional_job_frame(&mut output, cwd.as_bytes())?;
    optional_job_frame(&mut output, job.context_key.as_bytes())?;
    optional_job_frame(&mut output, job.claim.verb.as_bytes())?;
    optional_job_frame(&mut output, job.claim.key.as_bytes())?;
    optional_job_frame(&mut output, &job.status.to_be_bytes())?;
    optional_job_frame(&mut output, &(job.claim.inputs.len() as u64).to_be_bytes())?;
    for input in &job.claim.inputs {
        let path = input
            .path
            .to_str()
            .ok_or_else(|| "optional receipt job input path is not UTF-8".to_string())?;
        if path.len() > OPTIONAL_RECEIPT_JOB_MAX_PATH_BYTES
            || !input.path.is_absolute()
            || !input.path.starts_with(&job.project_root)
        {
            return Err("optional receipt job input path is outside its project root".into());
        }
        optional_job_frame(&mut output, path.as_bytes())?;
        if !is_digest(&input.digest) {
            return Err("optional receipt job input digest is malformed".into());
        }
        optional_job_frame(&mut output, input.digest.as_bytes())?;
    }
    optional_job_frame(&mut output, &(job.argv.len() as u64).to_be_bytes())?;
    for argument in &job.argv {
        if argument.is_empty()
            || argument.len() > OPTIONAL_RECEIPT_JOB_MAX_PATH_BYTES
            || argument.bytes().any(|byte| byte == 0 || byte.is_ascii_control())
        {
            return Err("optional receipt job argument is empty, too large, or unsafe".into());
        }
        optional_job_frame(&mut output, argument.as_bytes())?;
    }
    optional_job_frame(&mut output, &job.stdout)?;
    optional_job_frame(&mut output, &job.stderr)?;
    Ok(output)
}

fn decode_optional_receipt_job(
    bytes: &[u8],
    project_root: &Path,
) -> Result<OptionalReceiptJob, String> {
    if bytes.len() as u64 > OPTIONAL_RECEIPT_JOB_MAX_BYTES
        || !bytes.starts_with(OPTIONAL_RECEIPT_JOB_MAGIC)
    {
        return Err("optional receipt job schema or size is invalid".into());
    }
    let mut cursor = OPTIONAL_RECEIPT_JOB_MAGIC.len();
    let root = optional_job_text(
        &take_frame(bytes, &mut cursor)?,
        "project root",
        OPTIONAL_RECEIPT_JOB_MAX_PATH_BYTES,
    )?;
    let expected_root = project_root
        .to_str()
        .ok_or_else(|| "canonical optional receipt project root is not UTF-8".to_string())?;
    if root != expected_root {
        return Err("optional receipt job project root does not match its path".into());
    }
    let cwd = optional_job_text(
        &take_frame(bytes, &mut cursor)?,
        "working directory",
        OPTIONAL_RECEIPT_JOB_MAX_PATH_BYTES,
    )?;
    let cwd = PathBuf::from(cwd);
    if !cwd.is_absolute() || !cwd.starts_with(project_root) {
        return Err("optional receipt job working directory is outside its project root".into());
    }
    let context_key = optional_job_text(
        &take_frame(bytes, &mut cursor)?,
        "context key",
        DIGEST_LEN,
    )?;
    if !is_digest(&context_key) {
        return Err("optional receipt job context key is malformed".into());
    }
    let verb = optional_job_text(
        &take_frame(bytes, &mut cursor)?,
        "verb",
        OPTIONAL_RECEIPT_JOB_MAX_PATH_BYTES,
    )?;
    let key = optional_job_text(
        &take_frame(bytes, &mut cursor)?,
        "claim key",
        DIGEST_LEN,
    )?;
    let status_bytes = take_frame(bytes, &mut cursor)?;
    if status_bytes.len() != 4 {
        return Err("optional receipt job status is malformed".into());
    }
    let status = i32::from_be_bytes(
        status_bytes
            .as_slice()
            .try_into()
            .map_err(|_| "optional receipt job status is malformed".to_string())?,
    );
    let input_count = optional_job_u64(bytes, &mut cursor, "input count")?;
    if input_count > 100_000 {
        return Err("optional receipt job has too many inputs".into());
    }
    let mut inputs = Vec::with_capacity(input_count as usize);
    for _ in 0..input_count {
        let path = optional_job_text(
            &take_frame(bytes, &mut cursor)?,
            "input path",
            OPTIONAL_RECEIPT_JOB_MAX_PATH_BYTES,
        )?;
        let path = PathBuf::from(path);
        if !path.is_absolute() || !path.starts_with(project_root) {
            return Err("optional receipt job input path is outside its project root".into());
        }
        let digest = optional_job_text(
            &take_frame(bytes, &mut cursor)?,
            "input digest",
            DIGEST_LEN,
        )?;
        if !is_digest(&digest) {
            return Err("optional receipt job input digest is malformed".into());
        }
        inputs.push(ReceiptInput { path, digest });
    }
    let argument_count = optional_job_u64(bytes, &mut cursor, "argument count")?;
    if argument_count > OPTIONAL_RECEIPT_JOB_MAX_ARGS {
        return Err("optional receipt job has too many arguments".into());
    }
    let mut argv = Vec::with_capacity(argument_count as usize);
    for _ in 0..argument_count {
        argv.push(optional_job_text(
            &take_frame(bytes, &mut cursor)?,
            "argument",
            OPTIONAL_RECEIPT_JOB_MAX_PATH_BYTES,
        )?);
    }
    let stdout = take_frame(bytes, &mut cursor)?;
    let stderr = take_frame(bytes, &mut cursor)?;
    if cursor != bytes.len() {
        return Err("optional receipt job has trailing bytes".into());
    }
    let claim = ReceiptClaim {
        verb,
        key,
        inputs,
    };
    if !is_digest(&claim.key)
        || participating_verb(&argv) != Some(claim.verb.as_str())
        || !cacheable_invocation(&claim.verb, &argv)
    {
        return Err("optional receipt job claim does not match its cacheable act".into());
    }
    Ok(OptionalReceiptJob {
        project_root: project_root.to_path_buf(),
        cwd,
        context_key,
        claim,
        argv,
        status,
        stdout,
        stderr,
    })
}

fn persist_published_receipt(
    store: &ReceiptStore,
    published_claim: &ReceiptClaim,
    context_key: &str,
    argv: &[String],
    status: i32,
    stdout: &[u8],
    stderr: &[u8],
    cwd: &Path,
) -> Result<(), String> {
    let published = store.write(published_claim, argv, status, stdout, stderr)?;
    if !published {
        return Ok(());
    }
    store.adopt_staged_sections(published_claim)?;
    store.remember_context_key(context_key, published_claim)?;
    if let Some(receipt) = store.lookup(published_claim)? {
        index_published_receipt(store, &receipt, argv, cwd)?;
    }
    Ok(())
}

/// Queue one ordinary history write without delaying the command's result.
/// The queue is intentionally small: a full or unavailable worker drops this
/// optional job and reports the cause instead of creating an unbounded thread
/// backlog.
pub fn enqueue_optional_history(
    operation: &str,
    job: impl FnOnce() + Send + 'static,
) {
    let Some(sender) = OPTIONAL_HISTORY_QUEUE.as_ref() else {
        optional_history_notice(operation, "optional history worker is unavailable");
        return;
    };
    match sender.try_send((operation.to_string(), Box::new(job))) {
        Ok(()) => {}
        Err(TrySendError::Full((operation, _)))
        | Err(TrySendError::Disconnected((operation, _))) => {
            optional_history_notice(&operation, "optional history queue is full or unavailable");
        }
    }
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
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(error) => {
            optional_history_notice("resolving receipt working directory", &error.to_string());
            return None;
        }
    };
    let root = receipt_root(verb, argv, &cwd);
    let store = ReceiptStore::new(root);
    match store.lookup_context(verb, argv, &cwd) {
        Ok(Some(receipt)) => {
            let store_root = store.root.clone();
            let receipt_for_index = receipt.clone();
            let argv_for_index = argv.to_vec();
            let cwd_for_index = cwd.clone();
            enqueue_optional_history("indexing a cached receipt", move || {
                let store = ReceiptStore::new(store_root);
                if let Err(error) = index_published_receipt(
                    &store,
                    &receipt_for_index,
                    &argv_for_index,
                    &cwd_for_index,
                ) {
                    optional_history_notice("indexing a cached receipt", &error);
                }
            });
            let secret_values = receipt_secret_values(argv);
            replay_receipt(verb, argv, &receipt, &secret_values);
            return Some(receipt.status);
        }
        Ok(None) => {}
        Err(error) => {
            optional_history_notice("reading a cached receipt", &error);
            return None;
        }
    }

    let input_paths = if verb == "check" && !has_explicit_target(verb, argv) {
        project_check_input_paths(&cwd).unwrap_or_else(|| input_paths_for(verb, argv, &cwd))
    } else {
        input_paths_for(verb, argv, &cwd)
    };
    if input_paths.is_empty() && verb != "budget check" {
        return None;
    }
    let mut claim = match store.claim(verb, argv, &input_paths) {
        Ok(claim) => claim,
        Err(error) => {
            optional_history_notice("claiming a receipt", &error);
            return None;
        }
    };
    if verb == "prove" {
        let target_inputs_sha256 = match receipt_target_inputs_sha256(&claim, argv, &cwd) {
            Ok(identity) => identity,
            Err(error) => {
                optional_history_notice("resolving receipt identity", &error);
                return None;
            }
        };
        let filtered_paths = claim
            .inputs
            .iter()
            .filter(|input| {
                !is_generated_failure_input(&input.path)
                    || generated_failure_matches_target(&input.path, &target_inputs_sha256)
            })
            .map(|input| input.path.clone())
            .collect::<Vec<_>>();
        let has_generated_failure = claim.inputs.iter().any(|input| {
            generated_failure_matches_target(&input.path, &target_inputs_sha256)
        });
        if has_generated_failure {
            let attempt = format!(
                "{}:{}:{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|duration| duration.as_nanos())
                    .unwrap_or_default(),
                NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
            );
            claim = match store.claim_with_attempt(verb, argv, &filtered_paths, attempt.as_bytes()) {
                Ok(claim) => claim,
                Err(error) => {
                    optional_history_notice("claiming a generated replay receipt", &error);
                    return None;
                }
            };
        } else if filtered_paths.len() != claim.inputs.len() {
            claim = match store.claim(verb, argv, &filtered_paths) {
                Ok(claim) => claim,
                Err(error) => {
                    optional_history_notice("claiming a filtered receipt", &error);
                    return None;
                }
            };
        }
    }
    let proof_receipt_link = if verb == "prove" {
        match reserve_receipt_record(&store, &claim, argv, &cwd) {
            Ok(link) => link,
            Err(error) => {
                optional_history_notice("reserving a receipt record", &error);
                None
            }
        }
    } else {
        None
    };

    let executable = match std::env::current_exe() {
        Ok(executable) => executable,
        Err(error) => {
            optional_history_notice("resolving the Jet executable for receipt capture", &error.to_string());
            return None;
        }
    };
    let mut command = std::process::Command::new(executable);
    command
        .args(argv)
        .current_dir(&cwd)
        .env("JET_RECEIPT_BYPASS", "1")
        .env(JET_RECEIPT_DIR_ENV, &store.root)
        .env(JET_RECEIPT_CLAIM_ENV, &claim.key)
        // The parent digest is not knowable until child output is captured.
        // Prelude attach records are staged and rebound after publication.
        .env(JET_RECEIPT_DIGEST_ENV, "");
    if proof_receipt_link.is_some() {
        command.env(JET_RECEIPT_RECORD_CLAIM_ENV, &claim.key);
    } else {
        command.env_remove(JET_RECEIPT_RECORD_CLAIM_ENV);
    }
    let mut child = match command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            optional_history_notice("starting receipt capture", &error.to_string());
            return None;
        }
    };
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        optional_history_notice("capturing receipt stdout", "receipt child did not expose stdout");
        return None;
    };
    let Some(stderr) = child.stderr.take() else {
        let _ = child.kill();
        optional_history_notice("capturing receipt stderr", "receipt child did not expose stderr");
        return None;
    };
    let stdout_reader = std::thread::spawn(move || capture_stream(stdout, std::io::stdout()));
    let stderr_reader = std::thread::spawn(move || capture_stream(stderr, std::io::stderr()));
    let status = match child.wait() {
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => {
            optional_history_notice("waiting for receipt capture", &error.to_string());
            return None;
        }
    };
    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    let published_claim = if verb == "prove" {
        let target_inputs_sha256 = match receipt_target_inputs_sha256(&claim, argv, &cwd) {
            Ok(identity) => identity,
            Err(error) => {
                optional_history_notice("resolving published receipt identity", &error);
                return None;
            }
        };
        let current_paths = input_paths_for(verb, argv, &cwd)
            .into_iter()
            .filter(|path| {
                !is_generated_failure_input(path)
                    || generated_failure_matches_target(path, &target_inputs_sha256)
            })
            .collect::<Vec<_>>();
        let current_claim = match store.claim(verb, argv, &current_paths) {
            Ok(claim) => claim,
            Err(error) => {
                optional_history_notice("claiming the published receipt", &error);
                return None;
            }
        };
        ReceiptClaim {
            inputs: current_claim.inputs,
            ..claim.clone()
        }
    } else {
        claim.clone()
    };
    let secret_values = receipt_secret_values(argv);
    let stdout = match bounded_redact_bytes(&stdout, &secret_values) {
        Ok(stdout) => stdout,
        Err(error) => {
            optional_history_notice("staging optional receipt job", &error);
            return Some(status);
        }
    };
    let receipt_stderr = canonicalize_receipt_stderr(verb, &stderr);
    let receipt_stderr = match bounded_redact_bytes(&receipt_stderr, &secret_values) {
        Ok(stderr) => stderr,
        Err(error) => {
            optional_history_notice("staging optional receipt job", &error);
            return Some(status);
        }
    };
    let context_key = match store.context_key(verb, argv) {
        Ok(context_key) => context_key,
        Err(error) => {
            optional_history_notice("staging optional receipt job", &error);
            return Some(status);
        }
    };
    let project_root = match receipt_project_root_for_store(&store, &cwd) {
        Some(project_root) => project_root,
        None => {
            optional_history_notice(
                "staging optional receipt job",
                "receipt store is not rooted at the canonical `.jet/receipts` path",
            );
            return Some(status);
        }
    };
    let helper_cwd = match fs::canonicalize(&cwd) {
        Ok(helper_cwd) if helper_cwd.starts_with(&project_root) => helper_cwd,
        Ok(_) => {
            optional_history_notice(
                "staging optional receipt job",
                "receipt working directory is outside its canonical project root",
            );
            return Some(status);
        }
        Err(error) => {
            optional_history_notice(
                "staging optional receipt job",
                &format!("could not canonicalize receipt working directory: {error}"),
            );
            return Some(status);
        }
    };
    let job = OptionalReceiptJob {
        project_root,
        cwd: helper_cwd,
        context_key,
        claim: published_claim,
        argv: redacted_optional_receipt_argv(argv),
        status,
        stdout,
        stderr: receipt_stderr,
    };
    if let Err(error) = spawn_optional_receipt_helper(&job) {
        optional_history_notice("staging optional receipt job", &error);
    }
    Some(status)
}
fn receipt_record_path(store: &ReceiptStore, claim: &ReceiptClaim, cwd: &Path) -> Option<PathBuf> {
    let object = store.object_path(&claim.key);
    let object = if object.is_absolute() {
        object
    } else {
        cwd.join(object)
    };
    let project_root = receipt_project_root_for_store(store, cwd)?;
    object
        .strip_prefix(&project_root)
        .ok()
        .map(Path::to_path_buf)
        .filter(|path| !path.as_os_str().is_empty())
        .filter(|path| path.starts_with(Path::new(".jet")))
}

fn receipt_project_root_for_store(store: &ReceiptStore, cwd: &Path) -> Option<PathBuf> {
    let object_root = store.root.parent()?;
    if object_root.file_name().and_then(|name| name.to_str()) != Some(".jet") {
        return None;
    }
    let root = object_root.parent().unwrap_or(cwd);
    fs::canonicalize(root).ok().or_else(|| Some(root.to_path_buf()))
}

/// The one target identity contract shared by proof artifacts, receipt index
/// rows, and build verification.  The authority closure is resolved strictly:
/// malformed authority is an error, never a reason to hash a weaker input set.
#[derive(Clone, Debug)]
pub struct CanonicalTargetIdentity {
    pub members: Vec<(String, String)>,
    pub input_sha256: String,
    pub authority_root: PathBuf,
}

fn canonical_authority_roots(
    target: &Path,
) -> Result<(BTreeSet<PathBuf>, PathBuf), String> {
    let metadata = fs::symlink_metadata(target).map_err(|error| {
        format!(
            "couldn't inspect canonical target `{}`: {error}",
            target.display()
        )
    })?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "canonical target `{}` must not be a symlink",
            target.display()
        ));
    }
    let start = if metadata.is_dir() {
        target
    } else {
        target.parent().unwrap_or_else(|| Path::new("."))
    };
    let workspace_root = crate::Loader::find_workspace_root_checked(start)
        .map_err(|diagnostic| {
            format!(
                "couldn't resolve workspace authority for `{}`: {diagnostic:?}",
                target.display()
            )
        })?;
    let package_root = crate::Loader::find_package_root_checked(start)
        .map_err(|diagnostic| {
            format!(
                "couldn't resolve package authority for `{}`: {diagnostic:?}",
                target.display()
            )
        })?;
    let mut roots = BTreeSet::new();
    if let Some(root) = package_root.as_ref() {
        roots.insert(root.clone());
    }
    if let Some(root) = workspace_root.as_ref() {
        roots.insert(root.clone());
    }
    // Evidence, replay records, and proofs live in the one workspace-root
    // `.jet/`, never in a per-package one.
    let authority_root = crate::Loader::selected_project_root(start).map_err(|diagnostic| {
        format!(
            "couldn't resolve the workspace root for `{}`: {diagnostic:?}",
            target.display()
        )
    })?;
    Ok((roots, authority_root))
}

pub fn canonical_target_identity(
    target: &Path,
    members: &[(String, String)],
) -> Result<CanonicalTargetIdentity, String> {
    let (roots, authority_root) = canonical_authority_roots(target)?;

    let mut canonical = Vec::new();
    for (path, digest) in members {
        canonical_identity_insert(&mut canonical, path, digest)?;
    }
    for root in roots {
        for name in [
            crate::Syntax::PACKAGE_FILE,
            crate::Syntax::PAYLOAD_FILE,
            crate::Syntax::WORKSPACE_FILE,
            "build.jet",
        ] {
            canonical_identity_file(&root.join(name), &mut canonical)?;
        }
        canonical_identity_file(
            &root.join(crate::Syntax::UNIFIED_LOCK_FILE),
            &mut canonical,
        )?;
        canonical_identity_tree(
            &root.join(".jet").join("generated"),
            &mut canonical,
        )?;
    }
    canonical.sort_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
    let mut identity = Vec::new();
    for (path, digest) in &canonical {
        identity.extend_from_slice(
            format!(
                "{{\"path\":\"{}\",\"sha256\":\"{}\"}}\n",
                json_escape(path),
                json_escape(digest)
            )
            .as_bytes(),
        );
    }
    Ok(CanonicalTargetIdentity {
        members: canonical,
        input_sha256: sha256_hex(&identity),
        authority_root,
    })
}

fn canonical_identity_path(path: &Path) -> String {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let path = if path.is_absolute() {
        path.strip_prefix(&cwd).unwrap_or(path)
    } else {
        path
    };
    path.to_string_lossy().replace('\\', "/")
}

fn canonical_identity_insert(
    members: &mut Vec<(String, String)>,
    path: &str,
    digest: &str,
) -> Result<(), String> {
    let path = canonical_identity_path(Path::new(path));
    if is_generated_failure_identity_path(&path) {
        return Ok(());
    }
    if let Some((_, existing)) = members.iter().find(|(member, _)| member == &path) {
        if existing != digest {
            return Err(format!(
                "canonical target identity has conflicting digests for `{path}`"
            ));
        }
        return Ok(());
    }
    members.push((path, digest.to_string()));
    Ok(())
}

fn canonical_identity_file(
    path: &Path,
    members: &mut Vec<(String, String)>,
) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "couldn't inspect canonical authority input `{}`: {error}",
                path.display()
            ))
        }
    };
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "canonical authority input `{}` must not be a symlink",
            path.display()
        ));
    }
    if !metadata.is_file() {
        return Ok(());
    }
    let bytes = fs::read(path).map_err(|error| {
        format!(
            "couldn't read canonical authority input `{}`: {error}",
            path.display()
        )
    })?;
    let digest = sha256_hex(&bytes);
    let path = canonical_identity_path(path);
    canonical_identity_insert(members, &path, &digest)
}

fn canonical_identity_tree(
    root: &Path,
    members: &mut Vec<(String, String)>,
) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "couldn't inspect canonical generated inputs `{}`: {error}",
                root.display()
            ))
        }
    };
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "canonical generated inputs `{}` must not contain a symlink",
            root.display()
        ));
    }
    if metadata.is_file() {
        return canonical_identity_file(root, members);
    }
    if !metadata.is_dir() {
        return Ok(());
    }
    let entries = fs::read_dir(root).map_err(|error| {
        format!(
            "couldn't read canonical generated inputs `{}`: {error}",
            root.display()
        )
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "couldn't inspect canonical generated inputs `{}`: {error}",
                root.display()
            )
        })?;
        canonical_identity_tree(&entry.path(), members)?;
    }
    Ok(())
}

fn is_generated_failure_identity_path(path: &str) -> bool {
    path.split('/').collect::<Vec<_>>().windows(2).any(|parts| {
        parts[0] == ".jet" && parts[1] == "generated"
    })
}

fn receipt_target_inputs_sha256(
    claim: &ReceiptClaim,
    argv: &[String],
    cwd: &Path,
) -> Result<String, String> {
    let target = target_path(&claim.verb, argv, cwd)
        .ok_or_else(|| format!("couldn't resolve `{}` target for receipt identity", claim.verb))?;
    let members = claim
        .inputs
        .iter()
        .map(|input| {
            (
                input.path.to_string_lossy().to_string(),
                input.digest.clone(),
            )
        })
        .collect::<Vec<_>>();
    canonical_target_identity(&target, &members).map(|identity| identity.input_sha256)
}

fn receipt_record_identity(
    claim: &ReceiptClaim,
    argv: &[String],
    cwd: &Path,
) -> Result<RecordIdentity, String> {
    RecordIdentity::new(
        receipt_target_inputs_sha256(claim, argv, cwd)?,
        env!("CARGO_PKG_VERSION"),
        RECEIPT_RECORD_ENGINE,
    )
}

fn receipt_record_link(claim: &ReceiptClaim) -> Result<RecordLink, String> {
    RecordLink::new(RecordKind::Receipt, claim.key.clone())
}

fn reserve_receipt_record(
    store: &ReceiptStore,
    claim: &ReceiptClaim,
    argv: &[String],
    cwd: &Path,
) -> Result<Option<RecordLink>, String> {
    let Some(path) = receipt_record_path(store, claim, cwd) else {
        return Ok(None);
    };
    let identity = receipt_record_identity(claim, argv, cwd)?;
    let link = receipt_record_link(claim)?;
    let project_root = receipt_project_root_for_store(store, cwd)
        .ok_or_else(|| "receipt store is not under the canonical project `.jet` root".to_string())?;
    let mut index = RecordIndex::load_for_project(project_root)?;
    if let Some(existing) = index.find(RecordKind::Receipt, &claim.key, true) {
        if existing.identity != identity {
            return Err(format!(
                "receipt `{}` conflicts with its indexed identity",
                claim.key
            ));
        }
        return Ok(Some(link));
    }
    let sequence = index.next_recorded_sequence().map_err(|error| error.to_string())?;
    let entry = RecordIndexEntry::new(identity, RecordKind::Receipt, claim.key.clone(), path)?
        .with_capture(RecordCapture::Safe)
        .with_size(0)
        .with_recorded_sequence(sequence);
    index.update_and_store(entry)?;
    Ok(Some(link))
}
fn index_published_receipt(
    store: &ReceiptStore,
    receipt: &Receipt,
    argv: &[String],
    cwd: &Path,
) -> Result<(), String> {
    let Some(path) = receipt_record_path(store, &receipt.claim, cwd) else {
        return Ok(());
    };
    let object = store.object_path(&receipt.claim.key);
    let metadata = fs::symlink_metadata(&object).map_err(|error| {
        format!(
            "could not inspect receipt object `{}`: {error}",
            object.display()
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "receipt object is not a regular file: {}",
            object.display()
        ));
    }
    let size = metadata.len();
    let identity = receipt_record_identity(&receipt.claim, argv, cwd)?;
    let comparisons = indexed_comparison_sections(receipt)?;
    let project_root = receipt_project_root_for_store(store, cwd)
        .ok_or_else(|| "receipt store is not under the canonical project `.jet` root".to_string())?;
    let mut index = RecordIndex::load_for_project(project_root)?;
    let mut comparison_links = Vec::with_capacity(comparisons.len());
    let mut entries = Vec::with_capacity(comparisons.len() + 1);
    for (artifact_id, consumed, comparison_size) in comparisons {
        let existing = index.find(RecordKind::Comparison, &artifact_id, true);
        let sequence = match existing.as_ref() {
            Some(entry) => entry.recorded_sequence,
            None => index
                .next_recorded_sequence()
                .map_err(|error| error.to_string())?,
        };
        let comparison_identity = RecordIdentity::new(
            identity.target_inputs_sha256.clone(),
            identity.tool_version.clone(),
            "jet-comparison-v1",
        )?;
        entries.push(
            RecordIndexEntry::new(
                comparison_identity,
                RecordKind::Comparison,
                artifact_id.clone(),
                path.clone(),
            )?
            .with_links(consumed, Vec::new())?
            .with_capture(RecordCapture::Safe)
            .with_size(comparison_size)
            .with_recorded_sequence(sequence)
            .with_saved(existing.as_ref().is_some_and(|entry| entry.saved)),
        );
        comparison_links.push(RecordLink::new(RecordKind::Comparison, artifact_id)?);
    }
    let mut produced = receipt.produced.clone();
    for link in comparison_links {
        if !produced.contains(&link) {
            produced.push(link);
        }
    }
    let existing = index.find(RecordKind::Receipt, &receipt.claim.key, true);
    let sequence = match existing.as_ref() {
        Some(entry) => entry.recorded_sequence,
        None => index
            .next_recorded_sequence()
            .map_err(|error| error.to_string())?,
    };
    let saved = existing.as_ref().is_some_and(|entry| entry.saved);
    entries.push(
        RecordIndexEntry::new(
            identity,
            RecordKind::Receipt,
            receipt.claim.key.clone(),
            path,
        )?
        .with_links(receipt.consumed.clone(), produced)?
        .with_capture(RecordCapture::Safe)
        .with_size(size)
        .with_recorded_sequence(sequence)
        .with_saved(saved),
    );
    index.upsert_many_and_store(entries)?;
    Ok(())
}

fn indexed_comparison_sections(
    receipt: &Receipt,
) -> Result<Vec<(String, Vec<RecordLink>, u64)>, String> {
    let consumed = receipt
        .consumed
        .iter()
        .filter(|link| link.kind == RecordKind::Evidence)
        .cloned()
        .collect::<Vec<_>>();
    let mut comparisons = Vec::new();
    for section in &receipt.sections {
        if section.name != "comparison" || section.type_name != "ComparisonRecord" {
            continue;
        }
        if consumed.is_empty() {
            return Err(
                "comparison receipt must link at least one canonical evidence record".into(),
            );
        }
        let payload = section.value()?;
        let comparison_size = u64::try_from(payload.bytes().len())
            .map_err(|_| "comparison payload is too large".to_string())?;
        let record = ComparisonRecord::from_json(&payload)?;
        let artifact_id = record.artifact_id()?;
        if section.payload_digest != artifact_id {
            return Err("comparison section payload digest is not canonical".into());
        }
        comparisons.push((artifact_id, consumed.clone(), comparison_size));
    }
    Ok(comparisons)
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
        Some("test") => Some("test"),
        Some("prove") => Some("prove"),
        Some("budget") if argv.get(1).map(String::as_str) == Some("check") => {
            Some("budget check")
        }
        _ => None,
    }
}

fn cacheable_invocation(verb: &str, argv: &[String]) -> bool {
    if verb == "prove"
        && argv.iter().any(|arg| {
            matches!(
                arg.as_str(),
                "--capture" | "--capture-sensitive" | "--replay" | "--save" | "--unsave"
            ) || arg.starts_with("--capture=")
                || arg.starts_with("--capture-sensitive=")
                || arg.starts_with("--replay=")
                || arg.starts_with("--save=")
                || arg.starts_with("--unsave=")
        })
    {
        return false;
    }
    true
}

fn receipt_root(verb: &str, argv: &[String], cwd: &Path) -> PathBuf {
    let target = target_path(verb, argv, cwd);
    let start = target.as_deref().unwrap_or(cwd);
    let start = if start.is_absolute() {
        start.to_path_buf()
    } else {
        cwd.join(start)
    };
    let start = if start.is_dir() {
        start
    } else {
        start.parent()
            .map(Path::to_path_buf)
            .unwrap_or(start)
    };
    receipt_project_root(&start).join(".jet").join("receipts")
}

fn receipt_project_root(start: &Path) -> PathBuf {
    crate::Loader::selected_project_root(start).unwrap_or_else(|_| {
        fs::canonicalize(start)
            .ok()
            .unwrap_or_else(|| start.to_path_buf())
    })
}

/// The authority tree a bare project action reads: the declared workspace,
/// else the owning package, else the start directory itself. This is not
/// `receipt_project_root`: that root only locates the receipt store and
/// widens to the enclosing repository, and fingerprinting a whole repository
/// (running the loader on every source in it) made a package nested in a
/// large checkout appear to hang before the action even started.
fn receipt_authority_root(start: &Path) -> PathBuf {
    if let Ok(Some(root)) = crate::Loader::find_workspace_root_checked(start) {
        return root;
    }
    if let Ok(Some(root)) = crate::Loader::find_package_root_checked(start) {
        return root;
    }
    fs::canonicalize(start).unwrap_or_else(|_| start.to_path_buf())
}

fn receipt_authority_roots(start: &Path) -> BTreeSet<PathBuf> {
    let mut roots = BTreeSet::new();
    roots.insert(receipt_authority_root(start));

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
                | "--lens"
                | "--save"
                | "--unsave"
                | "--replay"
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
        let root = receipt_authority_root(cwd);
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
                | "--lens"
                | "--save"
                | "--unsave"
                | "--replay"
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
    Some(
        paths
            .into_iter()
            .filter(|path| regular_file(path))
            .collect(),
    )
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
        let is_generated_input =
            path.components()
                .collect::<Vec<_>>()
                .windows(2)
                .any(|components| {
                    components[0].as_os_str() == ".jet" && components[1].as_os_str() == "generated"
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
fn collect_generated_failure_inputs(root: &Path, out: &mut BTreeSet<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            collect_generated_failure_inputs(&path, out);
        } else if metadata.is_file() {
            out.insert(path);
        }
    }
}

fn is_generated_failure_input(path: &Path) -> bool {
    path.components()
        .collect::<Vec<_>>()
        .windows(3)
        .any(|components| {
            components[0].as_os_str() == ".jet"
                && components[1].as_os_str() == "records"
                && components[2].as_os_str() == "generated"
        })
}

fn generated_failure_matches_target(path: &Path, target_inputs_sha256: &str) -> bool {
    is_generated_failure_input(path)
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                name.starts_with(target_inputs_sha256)
                    && name.as_bytes().get(target_inputs_sha256.len()) == Some(&b'-')
            })
}

fn has_generated_failure_for_target(
    receipt: &Receipt,
    argv: &[String],
    cwd: &Path,
) -> Result<bool, String> {
    let target_inputs_sha256 = receipt_target_inputs_sha256(&receipt.claim, argv, cwd)?;
    let target = target_path("prove", argv, cwd)
        .ok_or_else(|| "couldn't resolve prove target for generated replay".to_string())?;
    let (roots, _) = canonical_authority_roots(&target)?;
    let mut paths = BTreeSet::new();
    for root in roots {
        collect_generated_failure_inputs(
            &root.join(".jet").join("records").join("generated"),
            &mut paths,
        );
    }
    Ok(paths
        .into_iter()
        .any(|path| generated_failure_matches_target(&path, &target_inputs_sha256)))
}


fn add_project_inputs(entry: &Path, verb: &str, out: &mut BTreeSet<PathBuf>) {
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
            "build.jet",
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
        if verb == "check" {
            for candidate in check_entry_candidates(&root) {
                if regular_file(&candidate) {
                    out.insert(candidate);
                }
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
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => frame(identity, b"missing"),
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
        if old.iter().all(|candidate| candidate.path != input.path) {
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
    // Validate again at the wire boundary so callers cannot construct an
    // unauthenticated receipt by bypassing `write_with_sections`.
    normalized_sections(&receipt.sections)?;
    normalized_links(&receipt.consumed, "consumed")?;
    normalized_links(&receipt.produced, "produced")?;
    let mut out = encode_receipt_body(receipt);
    frame(&mut out, receipt.digest.as_bytes());
    if out.len() as u64 > MAX_RECEIPT_BYTES {
        return Err(format!(
            "receipt is too large (maximum {} bytes)",
            MAX_RECEIPT_BYTES
        ));
    }
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
    if !receipt.sections.is_empty() {
        let mut sections = receipt.sections.clone();
        sections.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then(left.type_name.cmp(&right.type_name))
                .then(left.bytes.cmp(&right.bytes))
        });
        frame(&mut out, RECEIPT_SECTION_WIRE_MAGIC);
        frame(&mut out, &(sections.len() as u64).to_be_bytes());
        for section in sections {
            frame(&mut out, section.name.as_bytes());
            frame(&mut out, section.type_name.as_bytes());
            frame(&mut out, &section.bytes);
        }
    }
    if !receipt.consumed.is_empty() || !receipt.produced.is_empty() {
        frame(&mut out, RECEIPT_LINK_WIRE_MAGIC);
        frame(&mut out, &RECEIPT_LINK_VERSION.to_be_bytes());
        encode_links(&mut out, &receipt.consumed);
        encode_links(&mut out, &receipt.produced);
    }
    out
}

fn decode_receipt(bytes: &[u8]) -> Result<Receipt, String> {
    if bytes.len() as u64 > MAX_RECEIPT_BYTES {
        return Err("receipt is too large".into());
    }
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
    let extension_or_digest = take_frame(bytes, &mut cursor)?;
    let mut sections = Vec::new();
    let mut consumed = Vec::new();
    let mut produced = Vec::new();
    let mut digest_bytes = extension_or_digest;
    if digest_bytes.as_slice() == RECEIPT_SECTION_WIRE_MAGIC {
        let section_count = take_frame(bytes, &mut cursor)?;
        if section_count.len() != 8 {
            return Err("receipt section count is malformed".into());
        }
        let section_count =
            u64::from_be_bytes(section_count.try_into().expect("checked section count length"));
        if section_count > MAX_RECEIPT_SECTIONS as u64 {
            return Err(format!(
                "receipt has too many sections (maximum {})",
                MAX_RECEIPT_SECTIONS
            ));
        }
        sections = Vec::with_capacity(section_count as usize);
        for _ in 0..section_count {
            let name = String::from_utf8(take_frame(bytes, &mut cursor)?)
                .map_err(|_| "receipt section name is not UTF-8".to_string())?;
            let type_name = String::from_utf8(take_frame(bytes, &mut cursor)?)
                .map_err(|_| "receipt section type name is not UTF-8".to_string())?;
            let value = take_frame(bytes, &mut cursor)?;
            sections.push(ReceiptSection::new(name, type_name, value).map_err(|error| {
                format!("receipt section schema is invalid: {error}")
            })?);
        }
        sections = normalized_sections(&sections)?;
        digest_bytes = take_frame(bytes, &mut cursor)?;
    }
    if digest_bytes.as_slice() == RECEIPT_LINK_WIRE_MAGIC {
        let version = take_frame(bytes, &mut cursor)?;
        if version.len() != 8 {
            return Err("receipt links version is malformed".into());
        }
        let version = u64::from_be_bytes(version.try_into().expect("checked link version length"));
        if version != RECEIPT_LINK_VERSION {
            return Err(format!("unsupported receipt links version {version}"));
        }
        consumed = decode_links(bytes, &mut cursor, "consumed")?;
        produced = decode_links(bytes, &mut cursor, "produced")?;
        digest_bytes = take_frame(bytes, &mut cursor)?;
    }
    let digest = String::from_utf8(digest_bytes)
        .map_err(|_| "receipt digest is not UTF-8".to_string())?;
    if cursor != bytes.len() || !is_digest(&key) || !is_digest(&digest) {
        return Err("receipt has trailing bytes or malformed digest".into());
    }
    Ok(Receipt {
        claim: ReceiptClaim { verb, key, inputs },
        status,
        stdout,
        stderr,
        sections,
        consumed,
        produced,
        digest,
    })
}

fn normalized_links(links: &[RecordLink], label: &str) -> Result<Vec<RecordLink>, String> {
    if links.len() > MAX_RECEIPT_LINKS as usize {
        return Err(format!(
            "receipt has too many {label} links (maximum {MAX_RECEIPT_LINKS})"
        ));
    }
    let mut normalized = links.to_vec();
    for link in &normalized {
        link.validate()
            .map_err(|error| format!("receipt {label} link is invalid: {error}"))?;
    }
    normalized.sort_by(|left, right| left.kind.cmp(&right.kind).then(left.artifact_id.cmp(&right.artifact_id)));
    if normalized.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(format!("receipt {label} links contain a duplicate"));
    }
    Ok(normalized)
}

fn encode_links(out: &mut Vec<u8>, links: &[RecordLink]) {
    let mut links = links.to_vec();
    links.sort_by(|left, right| left.kind.cmp(&right.kind).then(left.artifact_id.cmp(&right.artifact_id)));
    frame(out, &(links.len() as u64).to_be_bytes());
    for link in links {
        frame(out, link.kind.as_str().as_bytes());
        frame(out, link.artifact_id.as_bytes());
    }
}

fn decode_links(
    bytes: &[u8],
    cursor: &mut usize,
    label: &str,
) -> Result<Vec<RecordLink>, String> {
    let count = take_frame(bytes, cursor)?;
    if count.len() != 8 {
        return Err(format!("receipt {label} link count is malformed"));
    }
    let count = u64::from_be_bytes(count.try_into().expect("checked link count length"));
    if count > MAX_RECEIPT_LINKS {
        return Err(format!(
            "receipt has too many {label} links (maximum {MAX_RECEIPT_LINKS})"
        ));
    }
    let mut links = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let kind = String::from_utf8(take_frame(bytes, cursor)?)
            .map_err(|_| format!("receipt {label} link kind is not UTF-8"))?;
        let kind = RecordKind::parse(&kind)
            .map_err(|error| format!("receipt {label} link kind is invalid: {error}"))?;
        let artifact_id = String::from_utf8(take_frame(bytes, cursor)?)
            .map_err(|_| format!("receipt {label} link artifact_id is not UTF-8"))?;
        links.push(
            RecordLink::new(kind, artifact_id)
                .map_err(|error| format!("receipt {label} link is invalid: {error}"))?,
        );
    }
    normalized_links(&links, label)
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
    environment_identity_from(std::env::vars().collect())
}

fn environment_identity_from(mut env: Vec<(String, String)>) -> Vec<u8> {
    // Nix creates a fresh build root for every shell, and systemd mints a
    // fresh `INVOCATION_ID` for every unit or scope (`systemd-run --scope`).
    // Both are process plumbing, not build inputs, so they cannot claim a
    // different receipt for the same invocation.
    env.retain(|(key, _)| {
        !matches!(
            key.as_str(),
            "INVOCATION_ID"
                | "JET_RECEIPT_BYPASS"
                | "NIX_BUILD_TOP"
                | "TEMP"
                | "TEMPDIR"
                | "TMP"
                | "TMPDIR"
        )
    });
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

fn replay_receipt(command: &str, argv: &[String], receipt: &Receipt, secret_values: &[String]) {
    let (stdout, stderr) = replay_output(receipt, secret_values);
    write_bytes(std::io::stdout(), &stdout);
    write_bytes(std::io::stderr(), &stderr);
    if receipt_notices_visible(command, argv) {
        let short = &receipt.claim.key[..12];
        let _ = writeln!(std::io::stderr(), "ok: {command} current (receipt {short})");
    }
}

/// A clean `jet check` prints one line (#3721): its receipt replay and
/// invalidation notices are detail that only `--verbose` shows.
fn receipt_notices_visible(verb: &str, argv: &[String]) -> bool {
    verb != "check" || jet_cli::CLI::OutputFlags::parse(argv).is_ok_and(|flags| flags.verbose)
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
            sections: Vec::new(),
            consumed: Vec::new(),
            produced: Vec::new(),
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
        let context_key = store.context_key("check", &argv).unwrap();
        store.remember_context_key(&context_key, &claim).unwrap();

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
        let context_key = store.context_key("check", &argv).unwrap();
        store.remember_context_key(&context_key, &claim).unwrap();

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
        let context_key = store.context_key("check", &argv).unwrap();
        store.remember_context_key(&context_key, &claim).unwrap();

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
        let context_key = store.context_key("check", &argv).unwrap();
        store.remember_context_key(&context_key, &claim).unwrap();

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
        let context_key = store.context_key("check", &argv).unwrap();
        store.remember_context_key(&context_key, &claim).unwrap();

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
        let context_key = store.context_key("check", &argv).unwrap();
        store.remember_context_key(&context_key, &claim).unwrap();
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
        let context_key = store.context_key("check", &argv).unwrap();
        store.remember_context_key(&context_key, &claim).unwrap();

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
        let context_key = store.context_key("check", &argv).unwrap();
        store.remember_context_key(&context_key, &claim).unwrap();

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
            sections: Vec::new(),
            consumed: Vec::new(),
            produced: Vec::new(),
            digest: "b".repeat(DIGEST_LEN),
        };
        let debug = format!("{receipt:?}");
        assert!(debug.contains("stdout: \"<redacted>\""));
        assert!(debug.contains("stderr: \"<redacted>\""));
        assert!(!debug.contains("legacy bearer secret"));
    }

    #[test]
    fn receipt_context_ignores_nix_shell_temp_paths_and_systemd_invocation() {
        let first = vec![
            ("INVOCATION_ID".into(), "7863347a1378412ea643bdcc4c1a8b6d".into()),
            ("NIX_BUILD_TOP".into(), "/tmp/nix-shell.first".into()),
            ("TEMP".into(), "/tmp/nix-shell.first".into()),
            ("TEMPDIR".into(), "/tmp/nix-shell.first".into()),
            ("TMP".into(), "/tmp/nix-shell.first".into()),
            ("TMPDIR".into(), "/tmp/nix-shell.first".into()),
            ("JET_RECEIPT_TEST_STABLE".into(), "same".into()),
        ];
        let second = vec![
            ("INVOCATION_ID".into(), "55d41a44be5646d9a261b25e341ea5bb".into()),
            ("NIX_BUILD_TOP".into(), "/tmp/nix-shell.second".into()),
            ("TEMP".into(), "/tmp/nix-shell.second".into()),
            ("TEMPDIR".into(), "/tmp/nix-shell.second".into()),
            ("TMP".into(), "/tmp/nix-shell.second".into()),
            ("TMPDIR".into(), "/tmp/nix-shell.second".into()),
            ("JET_RECEIPT_TEST_STABLE".into(), "same".into()),
        ];
        assert_eq!(
            environment_identity_from(first),
            environment_identity_from(second)
        );
        let changed = vec![("JET_RECEIPT_TEST_STABLE".into(), "different".into())];
        assert_ne!(
            environment_identity_from(vec![("JET_RECEIPT_TEST_STABLE".into(), "same".into())]),
            environment_identity_from(changed)
        );
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
            sections: Vec::new(),
            consumed: Vec::new(),
            produced: Vec::new(),
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
            stderr: Vec::new(),
            sections: Vec::new(),
            consumed: Vec::new(),
            produced: Vec::new(),
            digest: String::new(),
        };
        let digest = receipt_digest(&receipt);
        let mut bytes = encode_receipt(&Receipt { digest, ..receipt }).unwrap();
        let legacy_magic =
            include_bytes!("../tests/fixtures/build-metadata-compat/receipt-v1.magic");
        assert!(legacy_magic.len() >= MAGIC.len());
        bytes[..MAGIC.len()].copy_from_slice(&legacy_magic[..MAGIC.len()]);
        assert!(decode_receipt(&bytes).is_err());
        fs::create_dir_all(root.join("objects")).unwrap();
        fs::write(store.object_path(&claim.key), bytes).unwrap();

        assert!(store.lookup(&claim).unwrap().is_none());
        assert!(store.list().unwrap().is_empty());
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn optional_history_uses_canonical_jobs_and_fails_closed_for_blocked_destinations() {
        let make_project = |label: &str| {
            let project = std::env::temp_dir().join(format!(
                "jet-receipt-optional-{label}-{}-{}",
                std::process::id(),
                NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&project);
            fs::create_dir_all(&project).unwrap();
            fs::write(
                project.join("package.jet"),
                "name: \"optional-history\"\nversion: \"0.1.0\"\n",
            )
            .unwrap();
            fs::write(project.join("main.jet"), "fn run() {}\n").unwrap();
            fs::canonicalize(project).unwrap()
        };

        let project = make_project("success");
        let store = ReceiptStore::new(project.join(".jet").join("receipts"));
        let source = project.join("main.jet");
        let argv = vec!["check".to_string(), source.display().to_string()];
        let claim = store.claim("check", &argv, std::slice::from_ref(&source)).unwrap();
        let job = OptionalReceiptJob {
            project_root: project.clone(),
            cwd: project.clone(),
            context_key: store.context_key("check", &argv).unwrap(),
            claim: claim.clone(),
            argv: argv.clone(),
            status: 0,
            stdout: b"optional-output".to_vec(),
            stderr: Vec::new(),
        };
        let staged = stage_optional_receipt_job(&job).unwrap();
        assert_eq!(
            staged.parent().and_then(|path| path.file_name()).and_then(|name| name.to_str()),
            Some(OPTIONAL_RECEIPT_JOB_DIR)
        );
        assert_eq!(
            run_optional_receipt_helper(&[staged.display().to_string()]),
            0
        );
        assert!(!staged.exists(), "the helper consumes its disposable job");
        assert!(store.lookup(&claim).unwrap().is_some());
        let indexed = RecordIndex::load_for_project(&project).unwrap();
        assert!(indexed
            .find(RecordKind::Receipt, &claim.key, true)
            .is_some());

        let required = make_project("required-blocked");
        let required_root = required.join(".jet").join("receipts");
        fs::create_dir_all(&required_root).unwrap();
        let blocked_objects = required_root.join("objects");
        fs::write(&blocked_objects, b"required-destination").unwrap();
        let required_store = ReceiptStore::new(&required_root);
        let required_source = required.join("main.jet");
        let required_argv = vec![
            "check".to_string(),
            required_source.display().to_string(),
        ];
        let required_claim = required_store
            .claim(
                "check",
                &required_argv,
                std::slice::from_ref(&required_source),
            )
            .unwrap();
        let error = required_store
            .write(&required_claim, &required_argv, 0, b"output", b"")
            .expect_err("a non-directory object destination must fail closed");
        assert!(error.contains("unsafe"));
        assert_eq!(fs::read(&blocked_objects).unwrap(), b"required-destination");

        let optional = make_project("optional-blocked");
        let optional_root = optional.join(".jet").join("receipts");
        fs::create_dir_all(&optional_root).unwrap();
        let blocked_pending = optional_root.join(OPTIONAL_RECEIPT_JOB_DIR);
        fs::write(&blocked_pending, b"optional-destination").unwrap();
        let optional_store = ReceiptStore::new(&optional_root);
        let optional_source = optional.join("main.jet");
        let optional_argv = vec!["check".to_string(), optional_source.display().to_string()];
        let optional_claim = optional_store
            .claim(
                "check",
                &optional_argv,
                std::slice::from_ref(&optional_source),
            )
            .unwrap();
        let optional_job = OptionalReceiptJob {
            project_root: optional.clone(),
            cwd: optional.clone(),
            context_key: optional_store
                .context_key("check", &optional_argv)
                .unwrap(),
            claim: optional_claim,
            argv: optional_argv,
            status: 0,
            stdout: Vec::new(),
            stderr: Vec::new(),
        };
        assert!(stage_optional_receipt_job(&optional_job).is_err());
        assert_eq!(fs::read(&blocked_pending).unwrap(), b"optional-destination");

        let cleanup = make_project("cleanup");
        let cleanup_pending = cleanup.join(".jet").join("receipts").join(OPTIONAL_RECEIPT_JOB_DIR);
        fs::create_dir_all(&cleanup_pending).unwrap();
        let invalid = cleanup_pending.join(".job-invalid");
        fs::write(&invalid, b"not-a-job").unwrap();
        assert_eq!(
            run_optional_receipt_helper(&[invalid.display().to_string()]),
            0
        );
        assert!(!invalid.exists(), "invalid jobs in the disposable namespace are removed");
        let outside = cleanup.join(".job-invalid");
        fs::write(&outside, b"not-a-job").unwrap();
        assert_eq!(
            run_optional_receipt_helper(&[outside.display().to_string()]),
            0
        );
        assert!(
            outside.exists(),
            "cleanup never deletes a file outside the canonical disposable namespace"
        );

        let _ = fs::remove_dir_all(project);
        let _ = fs::remove_dir_all(required);
        let _ = fs::remove_dir_all(optional);
        let _ = fs::remove_dir_all(cleanup);
    }
}
