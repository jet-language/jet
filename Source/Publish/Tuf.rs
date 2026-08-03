//! Local transport for Jet's sparse TUF registry shape.
//!
//! The hosted HTTP transport is deliberately outside the compiler crate. The
//! registry's git publication path still carries the same four facts: signed
//! per-package targets, a signed snapshot/checkpoint, an append-only witness
//! log, and immutable content-addressed source artifacts. This module keeps
//! that wire contract deterministic for the native git transport and for
//! offline locked resolution.

use super::Index::{self, IndexEntry};
use crate::Diagnostics::Diagnostic;
use crate::SHA256;
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const METADATA_MAGIC: &str = "jet-tuf-sparse-v1";
const LOG_MAGIC: &str = "jet-transparency-log-v1";
const CHECKPOINT_MAGIC: &str = "jet-transparency-checkpoint-v1";
const MAX_METADATA_BYTES: usize = 4 * 1024 * 1024;
const MAX_LOG_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
struct SparseMetadata {
    name: String,
    entries: Vec<IndexEntry>,
    checkpoint: String,
    public_key: String,
    signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LogRecord {
    sequence: u64,
    operation: String,
    entry: IndexEntry,
    previous: String,
    leaf: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Checkpoint {
    sequence: u64,
    root: String,
    public_key: String,
    signature: String,
}

/// Files that must be included in one registry publication commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryMetadataFiles {
    pub paths: Vec<PathBuf>,
    pub checkpoint: String,
}

/// Rebuild the signed sparse metadata and witness checkpoint from the current
/// index. The returned paths are the complete metadata/log transaction and
/// must be staged together with the package artifact and index line.
pub fn refresh_registry_metadata(
    repo: &Path,
    registry_name: &str,
) -> Result<RegistryMetadataFiles, Diagnostic> {
    let root_key = crate::Publish::read_registry_root_key(registry_name)
        .map_err(|error| metadata_diagnostic(&error))?;
    let grouped = all_index_entries(repo).map_err(|error| metadata_diagnostic(&error))?;
    if grouped.is_empty() {
        return Err(metadata_diagnostic(&io::Error::new(
            io::ErrorKind::InvalidData,
            "registry index has no package metadata to publish",
        )));
    }

    let mut records = read_log(repo).map_err(|error| metadata_diagnostic(&error))?;
    let mut known = records
        .iter()
        .map(|record| record.entry.to_jsonl())
        .collect::<BTreeSet<_>>();
    let mut current = grouped
        .values()
        .flatten()
        .cloned()
        .collect::<Vec<_>>();
    current.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.version.cmp(&right.version))
            .then(left.yanked.cmp(&right.yanked))
    });
    for entry in current {
        if known.insert(entry.to_jsonl()) {
            let operation = if entry.yanked { "yank" } else { "publish" };
            let record = next_log_record(&records, operation, entry)?;
            records.push(record);
        }
    }
    validate_log(&records).map_err(|error| metadata_diagnostic(&error))?;
    write_log(repo, &records).map_err(|error| metadata_diagnostic(&error))?;

    let checkpoint = write_checkpoint(repo, registry_name, &root_key, &records)
        .map_err(|error| metadata_diagnostic(&error))?;
    let mut paths = vec![transparency_log_path(repo), checkpoint_path(repo)];
    for (name, entries) in grouped {
        let metadata = write_sparse_metadata(repo, registry_name, &root_key, &name, &entries, &checkpoint)
            .map_err(|error| metadata_diagnostic(&error))?;
        paths.push(metadata);
    }
    paths.sort();
    paths.dedup();
    Ok(RegistryMetadataFiles { paths, checkpoint })
}

/// Verify one package's sparse TUF metadata, its signed checkpoint, and the
/// exact index projection. This is the only registry package view used by
/// online or locked resolution after the metadata migration.
pub fn verify_registry_package(
    repo: &Path,
    registry_name: &str,
    name: &str,
) -> Result<Vec<IndexEntry>, Diagnostic> {
    let metadata = read_sparse_metadata(repo, name).map_err(|error| metadata_diagnostic(&error))?;
    let root_key = crate::Publish::read_registry_root_key(registry_name)
        .map_err(|error| metadata_diagnostic(&error))?;
    if metadata.public_key != root_key {
        return Err(metadata_diagnostic(&io::Error::new(
            io::ErrorKind::PermissionDenied,
            "registry sparse metadata is signed by an unpinned root key",
        )));
    }
    verify_sparse_metadata(&metadata).map_err(|error| metadata_diagnostic(&error))?;
    let checkpoint = read_checkpoint(repo).map_err(|error| metadata_diagnostic(&error))?;
    verify_checkpoint(repo, registry_name, &root_key, &checkpoint)
        .map_err(|error| metadata_diagnostic(&error))?;
    if metadata.checkpoint != checkpoint.root {
        return Err(metadata_diagnostic(&io::Error::new(
            io::ErrorKind::InvalidData,
            "sparse package metadata points at a different transparency checkpoint",
        )));
    }
    let mut indexed = Index::read_entries(repo, name)
        .map_err(|error| metadata_diagnostic(&error))?;
    validate_entries(name, &mut indexed).map_err(|error| metadata_diagnostic(&error))?;
    if metadata.entries != indexed {
        return Err(metadata_diagnostic(&io::Error::new(
            io::ErrorKind::InvalidData,
            "sparse package metadata disagrees with its immutable index projection",
        )));
    }
    if !metadata
        .entries
        .iter()
        .all(|entry| checkpoint_contains_entry(repo, entry))
    {
        return Err(metadata_diagnostic(&io::Error::new(
            io::ErrorKind::InvalidData,
            "sparse package metadata has no witnessed log inclusion",
        )));
    }
    accept_checkpoint(registry_name, &checkpoint)
        .map_err(|error| metadata_diagnostic(&error))?;
    Ok(metadata.entries)
}

fn all_index_entries(repo: &Path) -> io::Result<BTreeMap<String, Vec<IndexEntry>>> {
    let root = repo.join("index");
    let metadata = match std::fs::symlink_metadata(&root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(invalid("registry index root is not a real directory"));
    }
    let mut packages = std::fs::read_dir(&root)?.collect::<Result<Vec<_>, _>>()?;
    packages.sort_by_key(|entry| entry.file_name());
    let mut grouped = BTreeMap::new();
    for package in packages {
        let path = package.path();
        let name = package.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(invalid("registry index contains a non-directory package entry"));
        }
        let entries = Index::read_entries(repo, &name)?;
        if !entries.is_empty() {
            grouped.insert(name, entries);
        }
    }
    Ok(grouped)
}

fn write_sparse_metadata(
    repo: &Path,
    registry_name: &str,
    root_key: &str,
    name: &str,
    entries: &[IndexEntry],
    checkpoint: &str,
) -> io::Result<PathBuf> {
    let (seed, _public) = crate::Publish::Sign::key_paths(registry_name);
    let public_key = root_key.to_string();
    let mut entries = entries.to_vec();
    validate_entries(name, &mut entries)?;
    let unsigned = sparse_unsigned(name, &entries, checkpoint, &public_key);
    let signature = sign_payload(&seed, &unsigned)?;
    let mut out = unsigned;
    line(&mut out, "signature", &signature)?;
    let path = sparse_metadata_path(repo, name)?;
    atomic_write(&path, out.as_bytes())?;
    Ok(path)
}

fn read_sparse_metadata(repo: &Path, name: &str) -> io::Result<SparseMetadata> {
    let path = sparse_metadata_path(repo, name)?;
    let text = read_bounded(&path, MAX_METADATA_BYTES)?;
    let mut fields = BTreeMap::<String, Vec<String>>::new();
    let mut lines = text.lines();
    if lines.next() != Some(METADATA_MAGIC) {
        return Err(invalid("registry sparse metadata has an unknown format"));
    }
    for raw in lines {
        let (key, value) = raw
            .split_once('=')
            .ok_or_else(|| invalid("registry sparse metadata has a malformed line"))?;
        if !matches!(key, "name" | "entry" | "checkpoint" | "public_key" | "signature") {
            return Err(invalid("registry sparse metadata has an unknown field"));
        }
        if key != "entry" && fields.contains_key(key) {
            return Err(invalid("registry sparse metadata has a duplicate field"));
        }
        fields.entry(key.to_string()).or_default().push(value.to_string());
    }
    let encoded_name = one_field(&fields, "name")?;
    let parsed_name = decode_text(encoded_name)?;
    if parsed_name != name {
        return Err(invalid("registry sparse metadata name disagrees with its path"));
    }
    let mut entries = Vec::new();
    for encoded in fields.get("entry").cloned().unwrap_or_default() {
        let line = decode_text(&encoded)?;
        let entry = IndexEntry::parse_line(&line)
            .ok_or_else(|| invalid("registry sparse metadata contains a malformed entry"))?;
        entries.push(entry);
    }
    validate_entries(name, &mut entries)?;
    Ok(SparseMetadata {
        name: parsed_name,
        entries,
        checkpoint: decode_text(one_field(&fields, "checkpoint")?)?,
        public_key: decode_text(one_field(&fields, "public_key")?)?,
        signature: one_field(&fields, "signature")?.to_string(),
    })
}

fn verify_sparse_metadata(metadata: &SparseMetadata) -> io::Result<()> {
    let mut entries = metadata.entries.clone();
    validate_entries(&metadata.name, &mut entries)?;
    if metadata.public_key.len() != 64
        || !metadata.public_key.bytes().all(|byte| byte.is_ascii_hexdigit())
        || metadata.signature.is_empty()
    {
        return Err(invalid("registry sparse metadata has no usable signature"));
    }
    let unsigned = sparse_unsigned(
        &metadata.name,
        &entries,
        &metadata.checkpoint,
        &metadata.public_key,
    );
    verify_payload(&metadata.public_key, &unsigned, &metadata.signature)
}

fn sparse_unsigned(name: &str, entries: &[IndexEntry], checkpoint: &str, public_key: &str) -> String {
    let mut out = String::new();
    out.push_str(METADATA_MAGIC);
    out.push('\n');
    let _ = line(&mut out, "name", &encode_text(name));
    let _ = line(&mut out, "checkpoint", &encode_text(checkpoint));
    let _ = line(&mut out, "public_key", &encode_text(public_key));
    for entry in entries {
        let _ = line(&mut out, "entry", &encode_text(&entry.to_jsonl()));
    }
    out
}

fn read_log(repo: &Path) -> io::Result<Vec<LogRecord>> {
    let path = transparency_log_path(repo);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => {
            if text.len() > MAX_LOG_BYTES {
                return Err(invalid("registry transparency log is too large"));
            }
            text
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut lines = text.lines();
    if lines.next() != Some(LOG_MAGIC) {
        return Err(invalid("registry transparency log has an unknown format"));
    }
    let mut records = Vec::new();
    for raw in lines {
        let fields = raw.split('\t').collect::<Vec<_>>();
        if fields.len() != 5 {
            return Err(invalid("registry transparency log has a malformed record"));
        }
        let sequence = fields[0]
            .parse()
            .map_err(|_| invalid("registry transparency log sequence is invalid"))?;
        if fields[1] != "publish" && fields[1] != "yank" && fields[1] != "migrate" {
            return Err(invalid("registry transparency log operation is invalid"));
        }
        let entry_line = decode_text(fields[2])?;
        let entry = IndexEntry::parse_line(&entry_line)
            .ok_or_else(|| invalid("registry transparency log entry is malformed"))?;
        records.push(LogRecord {
            sequence,
            operation: fields[1].to_string(),
            entry,
            previous: fields[3].to_string(),
            leaf: fields[4].to_string(),
        });
    }
    validate_log(&records)?;
    Ok(records)
}

fn next_log_record(records: &[LogRecord], operation: &str, entry: IndexEntry) -> io::Result<LogRecord> {
    let sequence = records.last().map(|record| record.sequence + 1).unwrap_or(1);
    let previous = records
        .last()
        .map(|record| record.leaf.clone())
        .unwrap_or_else(empty_log_root);
    let canonical = log_record_unsigned(sequence, operation, &entry, &previous);
    let leaf = format!("sha256-{}", SHA256::sha256_hex(canonical.as_bytes()));
    Ok(LogRecord {
        sequence,
        operation: operation.to_string(),
        entry,
        previous,
        leaf,
    })
}

fn validate_log(records: &[LogRecord]) -> io::Result<()> {
    let mut previous = empty_log_root();
    for (index, record) in records.iter().enumerate() {
        if record.sequence != index as u64 + 1 || record.previous != previous {
            return Err(invalid("registry transparency log chain is not contiguous"));
        }
        let canonical = log_record_unsigned(
            record.sequence,
            &record.operation,
            &record.entry,
            &record.previous,
        );
        let expected = format!("sha256-{}", SHA256::sha256_hex(canonical.as_bytes()));
        if record.leaf != expected {
            return Err(invalid("registry transparency log leaf hash is invalid"));
        }
        previous = record.leaf.clone();
    }
    Ok(())
}

fn write_log(repo: &Path, records: &[LogRecord]) -> io::Result<()> {
    let mut out = String::new();
    out.push_str(LOG_MAGIC);
    out.push('\n');
    for record in records {
        out.push_str(&record.sequence.to_string());
        out.push('\t');
        out.push_str(&record.operation);
        out.push('\t');
        out.push_str(&encode_text(&record.entry.to_jsonl()));
        out.push('\t');
        out.push_str(&record.previous);
        out.push('\t');
        out.push_str(&record.leaf);
        out.push('\n');
    }
    atomic_write(&transparency_log_path(repo), out.as_bytes())
}

fn write_checkpoint(
    repo: &Path,
    registry_name: &str,
    root_key: &str,
    records: &[LogRecord],
) -> io::Result<String> {
    let root = records
        .last()
        .map(|record| record.leaf.clone())
        .unwrap_or_else(empty_log_root);
    let sequence = records.last().map(|record| record.sequence).unwrap_or(0);
    let (seed, _) = crate::Publish::Sign::key_paths(registry_name);
    let public_key = root_key.to_string();
    let unsigned = checkpoint_unsigned(sequence, &root, &public_key);
    let signature = sign_payload(&seed, &unsigned)?;
    let mut out = unsigned;
    line(&mut out, "signature", &signature)?;
    atomic_write(&checkpoint_path(repo), out.as_bytes())?;
    Ok(root)
}

fn read_checkpoint(repo: &Path) -> io::Result<Checkpoint> {
    let text = read_bounded(&checkpoint_path(repo), MAX_METADATA_BYTES)?;
    let mut fields = BTreeMap::<String, String>::new();
    let mut lines = text.lines();
    if lines.next() != Some(CHECKPOINT_MAGIC) {
        return Err(invalid("registry checkpoint has an unknown format"));
    }
    for raw in lines {
        let (key, value) = raw
            .split_once('=')
            .ok_or_else(|| invalid("registry checkpoint has a malformed line"))?;
        if !matches!(key, "sequence" | "root" | "public_key" | "signature")
            || fields.insert(key.to_string(), value.to_string()).is_some()
        {
            return Err(invalid("registry checkpoint has an unknown or duplicate field"));
        }
    }
    Ok(Checkpoint {
        sequence: required_field(&fields, "sequence")?
            .parse()
            .map_err(|_| invalid("registry checkpoint sequence is invalid"))?,
        root: required_field(&fields, "root")?.to_string(),
        public_key: decode_text(required_field(&fields, "public_key")?)?,
        signature: required_field(&fields, "signature")?.to_string(),
    })
}

fn verify_checkpoint(
    repo: &Path,
    _registry_name: &str,
    root_key: &str,
    checkpoint: &Checkpoint,
) -> io::Result<()> {
    let records = read_log(repo)?;
    let expected_root = records
        .last()
        .map(|record| record.leaf.clone())
        .unwrap_or_else(empty_log_root);
    let expected_sequence = records.last().map(|record| record.sequence).unwrap_or(0);
    if checkpoint.root != expected_root || checkpoint.sequence != expected_sequence {
        return Err(invalid("registry checkpoint disagrees with its transparency log"));
    }
    if checkpoint.public_key != root_key {
        return Err(invalid("registry checkpoint has an invalid public key"));
    }
    verify_payload(
        &checkpoint.public_key,
        &checkpoint_unsigned(checkpoint.sequence, &checkpoint.root, &checkpoint.public_key),
        &checkpoint.signature,
    )
}

fn checkpoint_contains_entry(repo: &Path, expected: &IndexEntry) -> bool {
    read_log(repo)
        .map(|records| records.iter().any(|record| record.entry == *expected))
        .unwrap_or(false)
}

fn accept_checkpoint(registry_name: &str, checkpoint: &Checkpoint) -> io::Result<()> {
    let path = crate::Publish::registry_checkpoint_path(registry_name);
    if let Ok(metadata) = std::fs::symlink_metadata(&path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(invalid("accepted registry checkpoint is not a regular file"));
        }
        let text = read_bounded(&path, 1024)?;
        let mut lines = text.lines();
        let sequence = lines
            .next()
            .ok_or_else(|| invalid("accepted registry checkpoint is malformed"))?
            .parse::<u64>()
            .map_err(|_| invalid("accepted registry checkpoint sequence is invalid"))?;
        let root = lines
            .next()
            .ok_or_else(|| invalid("accepted registry checkpoint is missing its root"))?;
        if sequence > checkpoint.sequence || (sequence == checkpoint.sequence && root != checkpoint.root)
        {
            return Err(invalid("registry checkpoint rolled back or forked"));
        }
        if sequence == checkpoint.sequence {
            return Ok(());
        }
    }
    let parent = path
        .parent()
        .ok_or_else(|| invalid("accepted registry checkpoint has no parent"))?;
    if let Ok(metadata) = std::fs::symlink_metadata(parent) {
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(invalid("accepted registry checkpoint parent is not a directory"));
        }
    } else {
        std::fs::create_dir_all(parent)?;
    }
    atomic_write(
        &path,
        format!("{}\n{}\n", checkpoint.sequence, checkpoint.root).as_bytes(),
    )
}

fn log_record_unsigned(sequence: u64, operation: &str, entry: &IndexEntry, previous: &str) -> String {
    format!(
        "{LOG_MAGIC}-record\nsequence={sequence}\noperation={operation}\nentry={}\nprevious={previous}\n",
        encode_text(&entry.to_jsonl())
    )
}

fn checkpoint_unsigned(sequence: u64, root: &str, public_key: &str) -> String {
    let mut out = String::new();
    out.push_str(CHECKPOINT_MAGIC);
    out.push('\n');
    let _ = line(&mut out, "sequence", &sequence.to_string());
    let _ = line(&mut out, "root", root);
    let _ = line(&mut out, "public_key", &encode_text(public_key));
    out
}

fn sign_payload(seed: &Path, payload: &str) -> io::Result<String> {
    if !seed.is_file() {
        return Err(invalid(&format!(
            "registry metadata signing key `{}` is unavailable",
            seed.display()
        )));
    }
    let message = format!("sha256-{}", SHA256::sha256_hex(payload.as_bytes()));
    crate::Publish::Sign::sign(seed, &message)
        .map_err(|diagnostic| invalid(&diagnostic.what))
}

fn verify_payload(public_key: &str, payload: &str, signature: &str) -> io::Result<()> {
    let message = format!("sha256-{}", SHA256::sha256_hex(payload.as_bytes()));
    let verified = crate::Publish::Sign::verify(public_key, &message, signature)
        .map_err(|diagnostic| invalid(&diagnostic.what))?;
    if verified {
        Ok(())
    } else {
        Err(invalid("registry metadata signature does not verify"))
    }
}

fn validate_entries(name: &str, entries: &mut Vec<IndexEntry>) -> io::Result<()> {
    if name.is_empty() {
        return Err(invalid("registry sparse metadata has an empty package name"));
    }
    entries.sort_by(|left, right| left.version.cmp(&right.version));
    for entry in entries.iter() {
        if entry.name != name {
            return Err(invalid("registry sparse metadata contains another package"));
        }
    }
    if entries.windows(2).any(|pair| pair[0].version == pair[1].version) {
        return Err(invalid("registry sparse metadata contains a duplicate version"));
    }
    Ok(())
}

fn sparse_metadata_path(repo: &Path, name: &str) -> io::Result<PathBuf> {
    let index = Index::index_entry_path(repo, name)?;
    Ok(repo.join("metadata").join(format!("{}.json", index.file_stem().and_then(|v| v.to_str()).unwrap_or(name))))
}

pub fn registry_package_metadata_path(repo: &Path, name: &str) -> io::Result<PathBuf> {
    sparse_metadata_path(repo, name)
}

fn transparency_log_path(repo: &Path) -> PathBuf {
    repo.join("transparency").join("log")
}

fn checkpoint_path(repo: &Path) -> PathBuf {
    repo.join("transparency").join("checkpoint")
}

fn read_bounded(path: &Path, limit: usize) -> io::Result<String> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(invalid("registry metadata path is not a regular file"));
    }
    if metadata.len() as usize > limit {
        return Err(invalid("registry metadata exceeds its size limit"));
    }
    std::fs::read_to_string(path)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Ok(metadata) = std::fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(invalid("registry metadata destination is not a regular file"));
        }
    }
    let parent = path
        .parent()
        .ok_or_else(|| invalid("registry metadata path has no parent"))?;
    if let Ok(metadata) = std::fs::symlink_metadata(parent) {
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(invalid("registry metadata parent is not a real directory"));
        }
    } else {
        std::fs::create_dir_all(parent)?;
        let metadata = std::fs::symlink_metadata(parent)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(invalid("registry metadata parent is not a real directory"));
        }
    }
    let partial = parent.join(format!(
        ".{}.partial-{}-{}",
        path.file_name().and_then(|value| value.to_str()).unwrap_or("metadata"),
        std::process::id(),
        unique_suffix()
    ));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial)?;
        use std::io::Write;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&partial, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    result
}

fn line(output: &mut String, key: &str, value: &str) -> io::Result<()> {
    if key.is_empty()
        || key.bytes().any(|byte| byte == b'=' || byte == b'\n' || byte == b'\r')
        || value.bytes().any(|byte| byte == b'\n' || byte == b'\r')
    {
        return Err(invalid("registry metadata contains an unsafe field"));
    }
    output.push_str(key);
    output.push('=');
    output.push_str(value);
    output.push('\n');
    Ok(())
}

fn one_field<'a>(fields: &'a BTreeMap<String, Vec<String>>, key: &str) -> io::Result<&'a str> {
    fields
        .get(key)
        .and_then(|values| (values.len() == 1).then_some(values[0].as_str()))
        .ok_or_else(|| invalid(&format!("registry sparse metadata is missing `{key}`")))
}

fn required_field<'a>(fields: &'a BTreeMap<String, String>, key: &str) -> io::Result<&'a str> {
    fields
        .get(key)
        .map(String::as_str)
        .ok_or_else(|| invalid(&format!("registry checkpoint is missing `{key}`")))
}

fn encode_text(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn decode_text(value: &str) -> io::Result<String> {
    if value.len() % 2 != 0 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid("registry metadata contains invalid hex text"));
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().chunks_exact(2) {
        let high = (pair[0] as char)
            .to_digit(16)
            .ok_or_else(|| invalid("registry metadata contains invalid hex text"))?;
        let low = (pair[1] as char)
            .to_digit(16)
            .ok_or_else(|| invalid("registry metadata contains invalid hex text"))?;
        bytes.push(((high << 4) | low) as u8);
    }
    String::from_utf8(bytes).map_err(|_| invalid("registry metadata text is not UTF-8"))
}

fn empty_log_root() -> String {
    format!("sha256-{}", SHA256::sha256_hex(b"jet-transparency-empty-v1"))
}

fn unique_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{}-{nanos}", std::process::id())
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

fn metadata_diagnostic(error: &io::Error) -> Diagnostic {
    super::Advisory::e2607("registry TUF metadata", &error.to_string())
}
