//! One deterministic, privacy-aware index over Jet record artifacts.
//!
//! The artifact codecs remain authoritative for their bytes.  This module only
//! stores references to those bytes under `.jet/records/index.jsonl`, keyed by
//! the shared `(target_inputs_sha256, tool_version, engine)` identity.  Rows
//! are versioned so the index can evolve without teaching each artifact codec
//! about the other record kinds.

use jet_foundation::DataTree::DataTree;
use jet_foundation::JSON::{json_escape, parse_json};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::thread;
use std::time::Duration;
use std::sync::atomic::{AtomicU64, Ordering};

pub const RECORD_INDEX_FILE: &str = "index.jsonl";
pub const RECORD_INDEX_VERSION: i64 = 1;
pub const RECORD_LINK_VERSION: i64 = 1;
const RECORD_INDEX_LOCK_FILE: &str = ".index.lock";
const RECORD_INDEX_LOCK_ATTEMPTS: usize = 2_000;
const RECORD_INDEX_LOCK_STALE_AFTER: Duration = Duration::from_secs(60);
pub const DEFAULT_RECORD_BUDGET_BYTES: u64 = 256 * 1024 * 1024;
const MAX_INDEX_LINE_BYTES: usize = 1024 * 1024;
const MAX_INDEX_BYTES: u64 = 256 * 1024 * 1024;
const MAX_IDENTITY_FIELD_BYTES: usize = 512;
const MAX_ARTIFACT_ID_BYTES: usize = 512;
const MAX_PATH_BYTES: usize = 4096;
const MAX_LINKS_PER_ENTRY: usize = 1024;
const MAX_LINKS_TOTAL: usize = 100_000;
static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordIndexError {
    RecordedSequenceOverflow,
}

impl fmt::Display for RecordIndexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RecordedSequenceOverflow => {
                formatter.write_str("recorded_sequence exhausted")
            }
        }
    }
}

impl std::error::Error for RecordIndexError {}

/// The one identity shared by every indexed artifact.
///
/// `target_inputs_sha256` is the digest of the target's input closure, not a
/// `tool_version` is the producer/tool version string and `engine` identifies
/// the execution engine (for example `mir-v1`).
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct RecordIdentity {
    pub target_inputs_sha256: String,
    pub tool_version: String,
    pub engine: String,
}

impl RecordIdentity {
    pub fn new(
        target_inputs_sha256: impl Into<String>,
        tool_version: impl Into<String>,
        engine: impl Into<String>,
    ) -> Result<Self, String> {
        let identity = Self {
            target_inputs_sha256: target_inputs_sha256.into(),
            tool_version: tool_version.into(),
            engine: engine.into(),
        };
        identity.validate()?;
        Ok(identity)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_digest(&self.target_inputs_sha256, "target_inputs_sha256")?;
        validate_text(&self.tool_version, "tool_version", MAX_IDENTITY_FIELD_BYTES)?;
        validate_text(&self.engine, "engine", MAX_IDENTITY_FIELD_BYTES)?;
        Ok(())
    }

    /// Stable textual key used to compare identities without relying on map
    /// iteration or a filesystem order.
    pub fn key(&self) -> String {
        format!(
            "{}\0{}\0{}",
            self.target_inputs_sha256, self.tool_version, self.engine
        )
    }

}

/// The closed set of record kinds listed by the Phase A index.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum RecordKind {
    Receipt,
    Replay,
    Proof,
    Evidence,
    Comparison,
    Trust,
}

impl RecordKind {
    pub const ALL: [Self; 6] = [
        Self::Receipt,
        Self::Replay,
        Self::Proof,
        Self::Evidence,
        Self::Comparison,
        Self::Trust,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Receipt => "receipt",
            Self::Replay => "replay",
            Self::Proof => "proof",
            Self::Evidence => "evidence",
            Self::Comparison => "comparison",
            Self::Trust => "trust",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "receipt" => Ok(Self::Receipt),
            "replay" => Ok(Self::Replay),
            "proof" => Ok(Self::Proof),
            "evidence" => Ok(Self::Evidence),
            "comparison" => Ok(Self::Comparison),
            "trust" => Ok(Self::Trust),
            _ => Err(format!("unknown record kind `{value}`")),
        }
    }

    const fn rank(self) -> u8 {
        match self {
            Self::Receipt => 0,
            Self::Replay => 1,
            Self::Proof => 2,
            Self::Evidence => 3,
            Self::Comparison => 4,
            Self::Trust => 5,
        }
    }
}

impl fmt::Display for RecordKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A typed edge to another indexed artifact.  Direction is supplied by the
/// containing entry's `consumed` or `produced` field.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct RecordLink {
    pub artifact_id: String,
    pub kind: RecordKind,
}

impl RecordLink {
    pub fn new(kind: RecordKind, artifact_id: impl Into<String>) -> Result<Self, String> {
        let link = Self {
            artifact_id: artifact_id.into(),
            kind,
        };
        link.validate()?;
        Ok(link)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_component(&self.artifact_id, "record link artifact_id", MAX_ARTIFACT_ID_BYTES)
    }

    pub fn id(&self) -> &str {
        &self.artifact_id
    }
}

/// Visibility attached to a record.  Sensitive rows remain in the durable
/// index for authenticated consumers but are omitted, including from their
/// links, for unauthenticated queries.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum RecordCapture {
    #[default]
    Safe,
    Sensitive,
}

impl RecordCapture {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Safe => "safe",
            Self::Sensitive => "sensitive",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "safe" => Ok(Self::Safe),
            "sensitive" => Ok(Self::Sensitive),
            _ => Err(format!("unknown record capture mode `{value}`")),
        }
    }

    pub const fn is_sensitive(self) -> bool {
        matches!(self, Self::Sensitive)
    }
}

/// Retention limits for disposable referenced records. The byte budget is the
/// sum of disposable artifact sizes, not the size of this small JSONL file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordBudget {
    pub max_bytes: u64,
}

impl Default for RecordBudget {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_RECORD_BUDGET_BYTES,
        }
    }
}

impl RecordBudget {
    pub fn new(max_bytes: u64) -> Result<Self, String> {
        if max_bytes == 0 {
            return Err("record budget byte limit must be greater than zero".into());
        }
        Ok(Self { max_bytes })
    }
}

/// One line in `.jet/records/index.jsonl`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordIndexEntry {
    pub identity: RecordIdentity,
    pub kind: RecordKind,
    pub artifact_id: String,
    /// Project-relative path to the authoritative artifact.  Phase A keeps
    /// references below the project's `.jet` directory.
    pub path: PathBuf,
    pub consumed: Vec<RecordLink>,
    pub produced: Vec<RecordLink>,
    pub capture: RecordCapture,
    pub size: u64,
    /// Monotonic append sequence assigned by the capture policy.
    pub recorded_sequence: u64,
    /// Saved records are retained even when an over-budget index needs eviction.
    pub saved: bool,
}

impl RecordIndexEntry {
    pub fn new(
        identity: RecordIdentity,
        kind: RecordKind,
        artifact_id: impl Into<String>,
        path: impl Into<PathBuf>,
    ) -> Result<Self, String> {
        let entry = Self {
            identity,
            kind,
            artifact_id: artifact_id.into(),
            path: path.into(),
            consumed: Vec::new(),
            produced: Vec::new(),
            capture: RecordCapture::Safe,
            size: 0,
            recorded_sequence: 0,
            saved: false,
        };
        entry.validate()?;
        Ok(entry)
    }

    pub fn with_links(
        mut self,
        consumed: Vec<RecordLink>,
        produced: Vec<RecordLink>,
    ) -> Result<Self, String> {
        self.consumed = consumed;
        self.produced = produced;
        self.normalize()
    }

    pub fn with_capture(mut self, capture: RecordCapture) -> Self {
        self.capture = capture;
        self
    }

    pub fn with_size(mut self, size: u64) -> Self {
        self.size = size;
        self
    }

    pub fn with_recorded_sequence(mut self, recorded_sequence: u64) -> Self {
        self.recorded_sequence = recorded_sequence;
        self
    }

    pub fn with_saved(mut self, saved: bool) -> Self {
        self.saved = saved;
        self
    }

    pub fn is_sensitive(&self) -> bool {
        self.capture.is_sensitive()
    }

    /// Ordinary run/build/test receipts, comparisons, evidence, and replay
    /// artifacts are disposable. Proof, trust, saved, sensitive, and
    /// production-report rows stay outside this budget.
    pub fn is_disposable(&self) -> bool {
        if self.saved || self.is_sensitive() {
            return false;
        }
        let disposable_path = |prefix: &str| {
            let prefix = Path::new(prefix);
            self.path == prefix || self.path.starts_with(prefix)
        };
        match self.kind {
            RecordKind::Replay => disposable_path(".jet/replays"),
            RecordKind::Receipt | RecordKind::Comparison => disposable_path(".jet/receipts"),
            RecordKind::Evidence => disposable_path(".jet/evidence"),
            RecordKind::Proof | RecordKind::Trust => false,
        }
    }

    pub fn key(&self) -> (RecordKind, &str) {
        (self.kind, &self.artifact_id)
    }

    pub fn normalize(mut self) -> Result<Self, String> {
        self.validate()?;
        self.consumed.sort();
        self.produced.sort();
        reject_duplicate_links(&self.consumed, "consumed")?;
        reject_duplicate_links(&self.produced, "produced")?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.identity.validate()?;
        validate_component(
            &self.artifact_id,
            "record artifact_id",
            MAX_ARTIFACT_ID_BYTES,
        )?;
        validate_index_path(&self.path)?;
        validate_links(&self.consumed, "consumed")?;
        validate_links(&self.produced, "produced")?;
        Ok(())
    }

    /// Canonical JSONL row.  Link versioning is explicit and independent of
    /// the artifact's own codec version.
    pub fn to_jsonl(&self) -> Result<String, String> {
        let entry = self.clone().normalize()?;
        Ok(format!(
            "{{\"capture\":\"{}\",\"consumed\":{},\"engine\":\"{}\",\"id\":\"{}\",\"kind\":\"{}\",\"links_version\":{},\"path\":\"{}\",\"produced\":{},\"recorded_sequence\":{},\"saved\":{},\"size\":{},\"target_inputs_sha256\":\"{}\",\"tool_version\":\"{}\",\"version\":{}}}",
            json_escape(entry.capture.as_str()),
            links_json(&entry.consumed),
            json_escape(&entry.identity.engine),
            json_escape(&entry.artifact_id),
            entry.kind,
            RECORD_LINK_VERSION,
            json_escape(&entry.path.to_string_lossy()),
            links_json(&entry.produced),
            entry.recorded_sequence,
            entry.saved,
            entry.size,
            json_escape(&entry.identity.target_inputs_sha256),
            json_escape(&entry.identity.tool_version),
            RECORD_INDEX_VERSION,
        ))
    }


    pub fn parse_line(line: &str) -> Result<Self, String> {
        if line.len() > MAX_INDEX_LINE_BYTES {
            return Err("record index line exceeds the 1 MiB limit".into());
        }
        let DataTree::Object(fields) = parse_json(line)
            .map_err(|_| "record index line is not valid JSON".to_string())?
        else {
            return Err("record index line must be a JSON object".into());
        };
        const KEYS: &[&str] = &[
            "capture",
            "consumed",
            "engine",
            "id",
            "kind",
            "links_version",
            "path",
            "produced",
            "recorded_sequence",
            "saved",
            "size",
            "target_inputs_sha256",
            "tool_version",
            "version",
        ];
        if fields.iter().any(|(key, _)| !KEYS.contains(&key.as_str())) {
            return Err("record index line contains an unknown field".into());
        }
        let version = integer_field(&fields, "version")?;
        if version != RECORD_INDEX_VERSION {
            return Err(format!("unsupported record index version {version}"));
        }
        let links_version = integer_field(&fields, "links_version")?;
        if links_version != RECORD_LINK_VERSION {
            return Err(format!("unsupported record links version {links_version}"));
        }
        let identity = RecordIdentity::new(
            string_field(&fields, "target_inputs_sha256")?,
            string_field(&fields, "tool_version")?,
            string_field(&fields, "engine")?,
        )?;
        let kind = RecordKind::parse(string_field(&fields, "kind")?.as_str())?;
        let artifact_id = string_field(&fields, "id")?;
        let path = PathBuf::from(string_field(&fields, "path")?);
        let capture = RecordCapture::parse(string_field(&fields, "capture")?.as_str())?;
        let size = unsigned_field(&fields, "size")?;
        let recorded_sequence = unsigned_field(&fields, "recorded_sequence")?;
        let saved = bool_field(&fields, "saved")?;
        let consumed = links_field(&fields, "consumed")?;
        let produced = links_field(&fields, "produced")?;
        Self {
            identity,
            kind,
            artifact_id,
            path,
            consumed,
            produced,
            capture,
            size,
            recorded_sequence,
            saved,
        }
        .normalize()
    }
}

/// One project record index.  The `root` passed to `new` is the actual
/// `.jet/records` directory; use `for_project` when starting with a project
/// root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordIndex {
    root: PathBuf,
    entries: Vec<RecordIndexEntry>,
    budget: RecordBudget,
}

impl RecordIndex {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            entries: Vec::new(),
            budget: RecordBudget::default(),
        }
    }

    pub fn for_project(project_root: impl Into<PathBuf>) -> Self {
        let project_root = project_root.into();
        let project_root = fs::canonicalize(&project_root).unwrap_or(project_root);
        Self::new(project_root.join(".jet").join("records"))
    }

    /// Alias for callers that use `project` as the constructor name.
    pub fn project(project_root: impl Into<PathBuf>) -> Self {
        Self::for_project(project_root)
    }

    pub fn load(root: impl Into<PathBuf>) -> Result<Self, String> {
        Self::load_with_budget(root, RecordBudget::default())
    }

    pub fn load_for_project(project_root: impl Into<PathBuf>) -> Result<Self, String> {
        Self::load(Self::for_project(project_root).root)
    }

    pub fn load_with_budget(
        root: impl Into<PathBuf>,
        budget: RecordBudget,
    ) -> Result<Self, String> {
        let root = root.into();
        let path = root.join(RECORD_INDEX_FILE);
        let bytes = match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(format!("record index is not a regular file: {}", path.display()));
                }
                if metadata.len() > MAX_INDEX_BYTES {
                    return Err("record index exceeds the 256 MiB limit".into());
                }
                fs::read(&path).map_err(|error| {
                    format!("could not read record index {}: {error}", path.display())
                })?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => {
                return Err(format!(
                    "could not inspect record index {}: {error}",
                    path.display()
                ))
            }
        };
        let text = String::from_utf8(bytes)
            .map_err(|_| "record index is not valid UTF-8".to_string())?;
        let mut entries = Vec::new();
        for (line_number, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            if line.len() > MAX_INDEX_LINE_BYTES {
                return Err(format!(
                    "record index line {} exceeds the 1 MiB limit",
                    line_number + 1
                ));
            }
            let entry = RecordIndexEntry::parse_line(line).map_err(|error| {
                format!("record index line {} is invalid: {error}", line_number + 1)
            })?;
            entries.push(entry);
        }
        let index = Self {
            root,
            entries,
            budget,
        };
        index.validate_all()?;
        Ok(index)
    }

    pub fn reload(&mut self) -> Result<(), String> {
        let loaded = Self::load_with_budget(self.root.clone(), self.budget)?;
        *self = loaded;
        Ok(())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn index_path(&self) -> PathBuf {
        self.root.join(RECORD_INDEX_FILE)
    }

    pub fn budget(&self) -> RecordBudget {
        self.budget
    }

    /// Return the next strictly increasing sequence for a newly recorded row.
    /// Empty stores begin at one; exhaustion is reported instead of wrapping.
    pub fn next_recorded_sequence(&self) -> Result<u64, RecordIndexError> {
        self.entries
            .iter()
            .map(|entry| entry.recorded_sequence)
            .max()
            .map_or(Ok(1), |maximum| {
                maximum
                    .checked_add(1)
                    .ok_or(RecordIndexError::RecordedSequenceOverflow)
            })
    }

    pub fn set_budget(&mut self, budget: RecordBudget) -> Result<(), String> {
        let mut candidate = self.clone();
        candidate.budget = budget;
        candidate.apply_eviction()?;
        candidate.validate_all()?;
        self.budget = candidate.budget;
        self.entries = candidate.entries;
        Ok(())
    }

    /// Return safe rows in deterministic `(kind,id,identity,path)` order.
    pub fn entries(&self) -> Vec<RecordIndexEntry> {
        self.query(false)
    }

    /// Return rows visible to the caller.  Authentication is explicit so an
    /// unauthenticated query cannot infer sensitive presence from a count or a
    /// link.
    pub fn query(&self, authenticated: bool) -> Vec<RecordIndexEntry> {
        let mut rows = self
            .entries
            .iter()
            .filter(|entry| authenticated || !entry.is_sensitive())
            .map(|entry| self.visible_entry(entry, authenticated))
            .collect::<Vec<_>>();
        rows.sort_by(compare_entries);
        rows
    }

    pub fn list(&self, authenticated: bool) -> Vec<RecordIndexEntry> {
        self.query(authenticated)
    }

    pub fn entries_authenticated(&self) -> Vec<RecordIndexEntry> {
        self.query(true)
    }

    pub fn query_identity(
        &self,
        identity: &RecordIdentity,
        authenticated: bool,
    ) -> Vec<RecordIndexEntry> {
        self.query(authenticated)
            .into_iter()
            .filter(|entry| &entry.identity == identity)
            .collect()
    }

    pub fn query_kind(&self, kind: RecordKind, authenticated: bool) -> Vec<RecordIndexEntry> {
        self.query(authenticated)
            .into_iter()
            .filter(|entry| entry.kind == kind)
            .collect()
    }

    pub fn find(
        &self,
        kind: RecordKind,
        artifact_id: &str,
        authenticated: bool,
    ) -> Option<RecordIndexEntry> {
        self.query(authenticated)
            .into_iter()
            .find(|entry| entry.kind == kind && entry.artifact_id == artifact_id)
    }

    /// Add or replace one `(kind, artifact_id)` row in memory. A byte-for-byte
    /// equivalent duplicate is idempotent; a differing duplicate is rejected.
    /// Only unsaved, non-sensitive ordinary receipts and replay artifacts are
    /// disposable.
    pub fn update(&mut self, entry: RecordIndexEntry) -> Result<(), String> {
        let entry = entry.normalize()?;
        let mut candidate = self.clone();
        if let Some(position) = candidate
            .entries
            .iter()
            .position(|existing| existing.key() == entry.key())
        {
            if candidate.entries[position] == entry {
                return Ok(());
            }
            return Err(format!(
                "record `{}/{}` conflicts with an existing identity",
                entry.kind, entry.artifact_id
            ));
        }
        let entry_kind = entry.kind;
        let entry_artifact_id = entry.artifact_id.clone();
        candidate.entries.push(entry);
        candidate.apply_eviction()?;
        candidate.validate_all()?;
        if !candidate
            .entries
            .iter()
            .any(|existing| existing.kind == entry_kind && existing.artifact_id == entry_artifact_id)
        {
            return Err(format!(
                "disposable record `{}/{}` exceeds the byte budget and was not retained",
                entry_kind, entry_artifact_id
            ));
        }
        self.entries = candidate.entries;
        Ok(())
    }

    /// Replace one existing row without changing its `(kind, artifact_id)` key.
    /// This is reserved for state transitions such as saving or unsaving a
    /// replay; callers must preserve the row's identity and sequence.
    pub fn replace(&mut self, entry: RecordIndexEntry) -> Result<(), String> {
        let entry = entry.normalize()?;
        let mut candidate = self.clone();
        let Some(position) = candidate
            .entries
            .iter()
            .position(|existing| existing.key() == entry.key())
        else {
            return Err(format!(
                "record `{}/{}` does not exist",
                entry.kind, entry.artifact_id
            ));
        };
        if candidate.entries[position].identity != entry.identity {
            return Err(format!(
                "record `{}/{}` conflicts with an existing identity",
                entry.kind, entry.artifact_id
            ));
        }
        let entry_kind = entry.kind;
        let entry_artifact_id = entry.artifact_id.clone();
        candidate.entries[position] = entry;
        candidate.apply_eviction()?;
        candidate.validate_all()?;
        if !candidate
            .entries
            .iter()
            .any(|existing| existing.kind == entry_kind && existing.artifact_id == entry_artifact_id)
        {
            return Err(format!(
                "disposable record `{}/{}` exceeds the byte budget and was not retained",
                entry_kind, entry_artifact_id
            ));
        }
        self.entries = candidate.entries;
        Ok(())
    }

    /// Mark one indexed replay as saved at its immutable project-relative path.
    /// The row identity and recorded sequence remain stable.
    pub fn save_replay(
        &mut self,
        artifact_id: &str,
        identity: &RecordIdentity,
        path: impl Into<PathBuf>,
        size: u64,
    ) -> Result<RecordIndexEntry, String> {
        let Some(existing) = self.find(RecordKind::Replay, artifact_id, true) else {
            return Err(format!("replay `{artifact_id}` is not indexed"));
        };
        if &existing.identity != identity {
            return Err(format!(
                "replay `{artifact_id}` conflicts with the current target identity"
            ));
        }
        let mut saved = existing.clone();
        saved.path = path.into();
        saved.size = size;
        saved.saved = true;
        self.replace(saved.clone())?;
        Ok(saved)
    }

    /// Return saved replay claims for one target in stable capture order.
    pub fn saved_replays(&self, identity: &RecordIdentity) -> Vec<RecordIndexEntry> {
        let mut rows = self
            .entries
            .iter()
            .filter(|entry| {
                entry.kind == RecordKind::Replay && entry.saved && &entry.identity == identity
            })
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|left, right| {
            left.recorded_sequence
                .cmp(&right.recorded_sequence)
                .then(left.artifact_id.cmp(&right.artifact_id))
                .then(left.path.cmp(&right.path))
        });
        rows
    }

    /// Remove one exact replay row.  Referential integrity is checked by the
    /// normal candidate validation before the mutation is published.
    pub fn remove_replay(&mut self, artifact_id: &str) -> Result<RecordIndexEntry, String> {
        let mut candidate = self.clone();
        let Some(position) = candidate
            .entries
            .iter()
            .position(|entry| entry.kind == RecordKind::Replay && entry.artifact_id == artifact_id)
        else {
            return Err(format!("replay `{artifact_id}` is not indexed"));
        };
        let removed = candidate.entries.remove(position);
        candidate.validate_all()?;
        self.entries = candidate.entries;
        Ok(removed)
    }

    pub fn append(&mut self, entry: RecordIndexEntry) -> Result<(), String> {
        self.update(entry)
    }
    pub fn update_and_store(&mut self, entry: RecordIndexEntry) -> Result<(), String> {
        self.publish_and_store(entry, false)
    }

    /// Atomically publish `entry`, superseding any row with the same
    /// `(kind, artifact_id)` key whatever its identity. Reserved for artifacts
    /// rewritten in place at a stable path, such as production receipts: the
    /// same bytes can be reproduced after an input the artifact does not
    /// record (an imported module) changed, and the latest producer owns the
    /// row.
    pub fn supersede_and_store(&mut self, entry: RecordIndexEntry) -> Result<(), String> {
        self.publish_and_store(entry, true)
    }

    fn publish_and_store(&mut self, entry: RecordIndexEntry, supersede: bool) -> Result<(), String> {
        let _lock = acquire_index_lock(&self.root)?;
        let mut candidate = if self.index_path().is_file() {
            Self::load_with_budget(self.root.clone(), self.budget)?
        } else {
            self.clone()
        };
        if supersede {
            candidate
                .entries
                .retain(|existing| existing.key() != entry.key());
        }
        let entry = if candidate
            .entries
            .iter()
            .all(|existing| existing.key() != entry.key())
            && entry.recorded_sequence
                <= candidate
                    .entries
                    .iter()
                    .map(|existing| existing.recorded_sequence)
                    .max()
                    .unwrap_or(0)
        {
            entry.with_recorded_sequence(
                candidate
                    .next_recorded_sequence()
                    .map_err(|error| format!("could not allocate record sequence: {error}"))?,
            )
        } else {
            entry
        };
        candidate.update(entry)?;
        candidate.store_unlocked()?;
        *self = candidate;
        Ok(())
    }

    /// Atomically upsert a batch against the latest on-disk index.
    ///
    /// Receipt publication updates a receipt and its produced comparison rows
    /// as one history transition.  Re-read under the lock so another process
    /// cannot be overwritten by a stale in-memory snapshot.
    pub(crate) fn upsert_many_and_store(
        &mut self,
        entries: impl IntoIterator<Item = RecordIndexEntry>,
    ) -> Result<(), String> {
        let _lock = acquire_index_lock(&self.root)?;
        let mut candidate = if self.index_path().is_file() {
            Self::load_with_budget(self.root.clone(), self.budget)?
        } else {
            self.clone()
        };
        for entry in entries {
            if candidate
                .entries
                .iter()
                .any(|existing| existing.key() == entry.key())
            {
                candidate.replace(entry)?;
                continue;
            }
            let entry = if entry.recorded_sequence
                <= candidate
                    .entries
                    .iter()
                    .map(|existing| existing.recorded_sequence)
                    .max()
                    .unwrap_or(0)
            {
                entry.with_recorded_sequence(
                    candidate
                        .next_recorded_sequence()
                        .map_err(|error| format!("could not allocate record sequence: {error}"))?,
                )
            } else {
                entry
            };
            candidate.update(entry)?;
        }
        candidate.store_unlocked()?;
        *self = candidate;
        Ok(())
    }

    pub fn save_replay_and_store(
        &mut self,
        artifact_id: &str,
        identity: &RecordIdentity,
        path: impl Into<PathBuf>,
        size: u64,
    ) -> Result<RecordIndexEntry, String> {
        let _lock = acquire_index_lock(&self.root)?;
        let mut candidate = if self.index_path().is_file() {
            Self::load_with_budget(self.root.clone(), self.budget)?
        } else {
            self.clone()
        };
        let saved = candidate.save_replay(artifact_id, identity, path, size)?;
        candidate.store_unlocked()?;
        *self = candidate;
        Ok(saved)
    }

    pub fn replace_and_store(&mut self, entry: RecordIndexEntry) -> Result<(), String> {
        let _lock = acquire_index_lock(&self.root)?;
        let mut candidate = if self.index_path().is_file() {
            Self::load_with_budget(self.root.clone(), self.budget)?
        } else {
            self.clone()
        };
        candidate.replace(entry)?;
        candidate.store_unlocked()?;
        *self = candidate;
        Ok(())
    }

    pub fn remove_replay_and_store(
        &mut self,
        artifact_id: &str,
    ) -> Result<RecordIndexEntry, String> {
        let _lock = acquire_index_lock(&self.root)?;
        let mut candidate = if self.index_path().is_file() {
            Self::load_with_budget(self.root.clone(), self.budget)?
        } else {
            self.clone()
        };
        let removed = candidate.remove_replay(artifact_id)?;
        candidate.store_unlocked()?;
        *self = candidate;
        Ok(removed)
    }

    /// Atomically publish the current validated rows as one JSONL file.
    pub fn store(&self) -> Result<(), String> {
        let _lock = acquire_index_lock(&self.root)?;
        self.store_unlocked()
    }

    fn store_unlocked(&self) -> Result<(), String> {
        self.validate_all()?;
        let index_path = self.index_path();
        let previous = match fs::symlink_metadata(&index_path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err(format!("record index is unsafe: {}", index_path.display()));
            }
            Ok(_) => Self::load_with_budget(self.root.clone(), self.budget)?.entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => {
                return Err(format!(
                    "could not inspect record index {}: {error}",
                    index_path.display()
                ));
            }
        };
        let mut bytes = Vec::new();
        for entry in &self.entries {
            let line = entry.to_jsonl()?;
            bytes.extend_from_slice(line.as_bytes());
            bytes.push(b'\n');
            if bytes.len() as u64 > MAX_INDEX_BYTES {
                return Err("record index exceeds the 256 MiB limit".into());
            }
        }
        secure_create_dir(&self.root)?;
        let path = self.index_path();
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(format!("record index is unsafe: {}", path.display()));
            }
        }
        let temp = self.root.join(format!(
            ".{}.{}.{}",
            RECORD_INDEX_FILE,
            std::process::id(),
            NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
                .map_err(|error| format!("could not stage record index: {error}"))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                file.set_permissions(fs::Permissions::from_mode(0o600))
                    .map_err(|error| format!("could not secure record index: {error}"))?;
            }
            file.write_all(&bytes)
                .map_err(|error| format!("could not write record index: {error}"))?;
            file.sync_all()
                .map_err(|error| format!("could not flush record index: {error}"))?;
            fs::rename(&temp, &path)
                .map_err(|error| format!("could not publish record index: {error}"))?;
            sync_directory(&self.root)
                .map_err(|error| format!("could not flush record index directory: {error}"))?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
            return result;
        }
        for entry in previous {
            if !entry.is_disposable()
                || self.entries.iter().any(|current| current.path == entry.path)
            {
                continue;
            }
            self.remove_disposable_artifact(&entry)?;
        }
        Ok(())
    }

    fn validate_all(&self) -> Result<(), String> {
        let mut keys = BTreeSet::new();
        let mut by_key = BTreeMap::new();
        let mut total_disposable_size = 0u64;
        let mut total_links = 0usize;
        let mut previous_sequence = None;
        for entry in &self.entries {
            entry.validate()?;
            if let Some(previous) = previous_sequence {
                if entry.recorded_sequence <= previous {
                    return Err(
                        "recorded_sequence values must be unique and strictly increasing".into(),
                    );
                }
            }
            previous_sequence = Some(entry.recorded_sequence);
            if !keys.insert(entry.key()) {
                return Err(format!(
                    "duplicate record identity `{}/{}`",
                    entry.kind, entry.artifact_id
                ));
            }
            if entry.is_disposable() {
                total_disposable_size = total_disposable_size
                    .checked_add(entry.size)
                    .ok_or_else(|| "record budget byte count overflows".to_string())?;
            }
            total_links = total_links
                .checked_add(entry.consumed.len() + entry.produced.len())
                .ok_or_else(|| "record link count overflows".to_string())?;
            by_key.insert((entry.kind, entry.artifact_id.as_str()), entry);
        }
        if total_disposable_size > self.budget.max_bytes {
            return Err(format!(
                "disposable record size {} exceeds budget {}",
                total_disposable_size, self.budget.max_bytes
            ));
        }
        if total_links > MAX_LINKS_TOTAL {
            return Err("record index has too many links".into());
        }
        for entry in &self.entries {
            for link in entry.consumed.iter().chain(entry.produced.iter()) {
                if !by_key.contains_key(&(link.kind, link.artifact_id.as_str())) {
                    return Err(format!(
                        "record `{}/{}` links to missing `{}/{}`",
                        entry.kind, entry.artifact_id, link.kind, link.artifact_id
                    ));
                }
            }
        }
        reject_cycles(&self.entries)
    }

    fn apply_eviction(&mut self) -> Result<(), String> {
        loop {
            let total_disposable_size = self.entries.iter().try_fold(0u64, |total, entry| {
                if !entry.is_disposable() {
                    return Ok(total);
                }
                total
                    .checked_add(entry.size)
                    .ok_or_else(|| "record budget byte count overflows".to_string())
            })?;
            if total_disposable_size <= self.budget.max_bytes {
                return Ok(());
            }
            let Some(group) = self
                .eviction_groups()
                .into_iter()
                .min_by(|left, right| {
                    let left_key = left
                        .iter()
                        .map(|index| {
                            (
                                self.entries[*index].recorded_sequence,
                                self.entries[*index].artifact_id.as_str(),
                            )
                        })
                        .min();
                    let right_key = right
                        .iter()
                        .map(|index| {
                            (
                                self.entries[*index].recorded_sequence,
                                self.entries[*index].artifact_id.as_str(),
                            )
                        })
                        .min();
                    left_key.cmp(&right_key)
                })
            else {
                return Err(
                    "disposable history budget exceeded; no unreferenced FIFO record group can be evicted"
                        .into(),
                );
            };
            for position in group.into_iter().rev() {
                self.entries.remove(position);
            }
        }
    }

    /// Return connected disposable groups whose members are not referenced by
    /// any protected or otherwise non-group record. Evicting one group cannot
    /// leave a dangling link.
    fn eviction_groups(&self) -> Vec<Vec<usize>> {
        let disposable = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| entry.is_disposable().then_some(index))
            .collect::<Vec<_>>();
        let mut visited = BTreeSet::new();
        let mut groups = Vec::new();
        for start in disposable.iter().copied() {
            if !visited.insert(start) {
                continue;
            }
            let mut group = BTreeSet::from([start]);
            loop {
                let mut changed = false;
                for candidate in disposable.iter().copied() {
                    if group.contains(&candidate) {
                        continue;
                    }
                    if group.iter().any(|member| {
                        entries_linked(&self.entries[*member], &self.entries[candidate])
                    }) {
                        group.insert(candidate);
                        visited.insert(candidate);
                        changed = true;
                    }
                }
                if !changed {
                    break;
                }
            }
            let externally_referenced = self.entries.iter().enumerate().any(|(index, entry)| {
                !group.contains(&index)
                    && group.iter().any(|target| entry_links_to(entry, &self.entries[*target]))
            });
            if !externally_referenced {
                groups.push(group.into_iter().collect());
            }
        }
        groups
    }

    fn remove_disposable_artifact(&self, entry: &RecordIndexEntry) -> Result<(), String> {
        if self.entries.iter().any(|other| other.path == entry.path) {
            return Ok(());
        }
        let Some(jet_root) = self.root.parent() else {
            return Ok(());
        };
        let Some(project_root) = jet_root.parent() else {
            return Ok(());
        };
        let path = project_root.join(&entry.path);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => Err(
                format!("disposable artifact is not a regular file: {}", path.display()),
            ),
            Ok(_) => match fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(format!(
                    "could not evict disposable artifact `{}`: {error}",
                    path.display()
                )),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!(
                "could not inspect disposable artifact `{}`: {error}",
                path.display()
            )),
        }
    }


    fn visible_entry(&self, entry: &RecordIndexEntry, authenticated: bool) -> RecordIndexEntry {
        if authenticated {
            return entry.clone();
        }
        let mut visible = entry.clone();
        visible.consumed.retain(|link| self.link_visible(link));
        visible.produced.retain(|link| self.link_visible(link));
        visible
    }

    fn link_visible(&self, link: &RecordLink) -> bool {
        self.entries
            .iter()
            .find(|entry| entry.kind == link.kind && entry.artifact_id == link.artifact_id)
            .is_some_and(|entry| !entry.is_sensitive())
    }
}

fn entries_linked(left: &RecordIndexEntry, right: &RecordIndexEntry) -> bool {
    entry_links_to(left, right) || entry_links_to(right, left)
}

fn entry_links_to(entry: &RecordIndexEntry, target: &RecordIndexEntry) -> bool {
    entry
        .consumed
        .iter()
        .chain(entry.produced.iter())
        .any(|link| link.kind == target.kind && link.artifact_id == target.artifact_id)
}

fn compare_entries(left: &RecordIndexEntry, right: &RecordIndexEntry) -> std::cmp::Ordering {
    left.kind
        .rank()
        .cmp(&right.kind.rank())
        .then(left.artifact_id.cmp(&right.artifact_id))
        .then(left.identity.cmp(&right.identity))
        .then(left.path.cmp(&right.path))
}

fn validate_digest(value: &str, field: &str) -> Result<(), String> {
    let bare = value.strip_prefix("sha256-").unwrap_or(value);
    if bare.len() != 64
        || !bare
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(format!("{field} must be a lowercase SHA-256 digest"));
    }
    Ok(())
}

fn validate_text(value: &str, field: &str, max_bytes: usize) -> Result<(), String> {
    if value.is_empty() || value.len() > max_bytes || value.chars().any(char::is_control) {
        return Err(format!("{field} is empty, too large, or contains control characters"));
    }
    Ok(())
}

fn validate_component(value: &str, field: &str, max_bytes: usize) -> Result<(), String> {
    validate_text(value, field, max_bytes)?;
    if value == "." || value == ".." || value.contains('/') || value.contains('\\') {
        return Err(format!("{field} contains an unsafe path component"));
    }
    Ok(())
}

fn validate_index_path(path: &Path) -> Result<(), String> {
    let text = path
        .to_str()
        .ok_or_else(|| "record index path is not UTF-8".to_string())?;
    if text.is_empty() || text.len() > MAX_PATH_BYTES || text.contains('\0') || text.contains('\\') {
        return Err("record index path is empty, too large, or uses unsafe separators".into());
    }
    if path.is_absolute() {
        return Err("record index path must be project-relative".into());
    }
    let components = path.components().collect::<Vec<_>>();
    if components.len() < 3 || components[0] != Component::Normal(OsStr::new(".jet")) {
        return Err("record index path must stay under `.jet/<artifact-kind>`".into());
    }
    if components.iter().any(|component| {
        matches!(component, Component::CurDir | Component::ParentDir | Component::RootDir | Component::Prefix(_))
    }) {
        return Err("record index path contains an unsafe component".into());
    }
    Ok(())
}

fn validate_links(links: &[RecordLink], field: &str) -> Result<(), String> {
    if links.len() > MAX_LINKS_PER_ENTRY {
        return Err(format!("record {field} has too many links"));
    }
    for link in links {
        link.validate()?;
    }
    Ok(())
}

fn reject_duplicate_links(links: &[RecordLink], field: &str) -> Result<(), String> {
    for pair in links.windows(2) {
        if pair[0] == pair[1] {
            return Err(format!("record {field} contains duplicate link"));
        }
    }
    Ok(())
}

fn reject_cycles(entries: &[RecordIndexEntry]) -> Result<(), String> {
    let mut graph: BTreeMap<(RecordKind, &str), Vec<(RecordKind, &str)>> = BTreeMap::new();
    for entry in entries {
        let key = (entry.kind, entry.artifact_id.as_str());
        let edges = graph.entry(key).or_default();
        edges.extend(
            entry
                .consumed
                .iter()
                .chain(entry.produced.iter())
                .map(|link| (link.kind, link.artifact_id.as_str())),
        );
    }
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for key in graph.keys().copied() {
        if visit_cycle(key, &graph, &mut visiting, &mut visited) {
            return Err(format!("record links contain a cycle at `{}/{}`", key.0, key.1));
        }
    }
    Ok(())
}

fn visit_cycle<'a>(
    key: (RecordKind, &'a str),
    graph: &BTreeMap<(RecordKind, &'a str), Vec<(RecordKind, &'a str)>>,
    visiting: &mut BTreeSet<(RecordKind, &'a str)>,
    visited: &mut BTreeSet<(RecordKind, &'a str)>,
) -> bool {
    if visiting.contains(&key) {
        return true;
    }
    if !visited.insert(key) {
        return false;
    }
    visiting.insert(key);
    if let Some(edges) = graph.get(&key) {
        for edge in edges {
            if visit_cycle(*edge, graph, visiting, visited) {
                return true;
            }
        }
    }
    visiting.remove(&key);
    false
}

fn links_json(links: &[RecordLink]) -> String {
    let mut links = links.to_vec();
    links.sort();
    let values = links
        .iter()
        .map(|link| {
            format!(
                "{{\"artifact_id\":\"{}\",\"kind\":\"{}\"}}",
                json_escape(&link.artifact_id),
                link.kind
            )
        })
        .collect::<Vec<_>>();
    format!("[{}]", values.join(","))
}

fn field<'a>(fields: &'a [(String, DataTree)], name: &str) -> Option<&'a DataTree> {
    fields
        .iter()
        .find_map(|(key, value)| (key == name).then_some(value))
}

fn string_field(fields: &[(String, DataTree)], name: &str) -> Result<String, String> {
    match field(fields, name) {
        Some(DataTree::Text(value) | DataTree::TypedText(value)) => Ok(value.clone()),
        Some(_) => Err(format!("record index field `{name}` must be a string")),
        None => Err(format!("record index is missing `{name}`")),
    }
}

fn integer_field(fields: &[(String, DataTree)], name: &str) -> Result<i64, String> {
    match field(fields, name) {
        Some(DataTree::Int(value)) => Ok(*value),
        Some(DataTree::Number(value)) => value
            .parse::<i64>()
            .map_err(|_| format!("record index field `{name}` must be an integer")),
        Some(_) => Err(format!("record index field `{name}` must be an integer")),
        None => Err(format!("record index is missing `{name}`")),
    }
}

fn unsigned_field(fields: &[(String, DataTree)], name: &str) -> Result<u64, String> {
    let value = integer_field(fields, name)?;
    u64::try_from(value).map_err(|_| format!("record index field `{name}` must be non-negative"))
}

fn bool_field(fields: &[(String, DataTree)], name: &str) -> Result<bool, String> {
    match field(fields, name) {
        Some(DataTree::Bool(value)) => Ok(*value),
        Some(_) => Err(format!("record index field `{name}` must be a boolean")),
        None => Err(format!("record index is missing `{name}`")),
    }
}

fn links_field(
    fields: &[(String, DataTree)],
    name: &str,
) -> Result<Vec<RecordLink>, String> {
    let Some(DataTree::Array(values)) = field(fields, name) else {
        return Err(format!("record index field `{name}` must be an array"));
    };
    if values.len() > MAX_LINKS_PER_ENTRY {
        return Err(format!("record index field `{name}` has too many links"));
    }
    let mut links = Vec::with_capacity(values.len());
    for value in values {
        let DataTree::Object(fields) = value else {
            return Err(format!("record index {name} link must be an object"));

        };
        if fields
            .iter()
            .any(|(key, _)| key != "artifact_id" && key != "kind")
        {
            return Err(format!("record index {name} link contains an unknown field"));
        }
        let kind = RecordKind::parse(string_field(fields, "kind")?.as_str())?;
        links.push(RecordLink::new(kind, string_field(fields, "artifact_id")?)?);
    }
    let mut normalized = links;
    normalized.sort();
    reject_duplicate_links(&normalized, name)?;
    Ok(normalized)
}

struct IndexWriteLock {
    path: PathBuf,
}

impl Drop for IndexWriteLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn acquire_index_lock(root: &Path) -> Result<IndexWriteLock, String> {
    secure_create_dir(root)?;
    let path = root.join(RECORD_INDEX_LOCK_FILE);
    for _ in 0..RECORD_INDEX_LOCK_ATTEMPTS {
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Err(error) = file.set_permissions(fs::Permissions::from_mode(0o600)) {
                        let _ = fs::remove_file(&path);
                        return Err(format!("could not secure record index lock: {error}"));
                    }
                }
                if let Err(error) = file.write_all(std::process::id().to_string().as_bytes()) {
                    let _ = fs::remove_file(&path);
                    return Err(format!("could not write record index lock: {error}"));
                }
                return Ok(IndexWriteLock { path });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if let Ok(metadata) = fs::symlink_metadata(&path) {
                    if !metadata.file_type().is_symlink()
                        && metadata.is_file()
                        && metadata
                            .modified()
                            .ok()
                            .and_then(|modified| modified.elapsed().ok())
                            .is_some_and(|age| age > RECORD_INDEX_LOCK_STALE_AFTER)
                    {
                        let _ = fs::remove_file(&path);
                    }
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => {
                return Err(format!("could not create record index lock: {error}"));
            }
        }
    }
    Err("timed out waiting for record index lock".into())
}

fn secure_create_dir(path: &Path) -> Result<(), String> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir => current.push(Path::new("/")),
            Component::Normal(name) => {
                current.push(name);
                match fs::symlink_metadata(&current) {
                    Ok(metadata) if metadata.file_type().is_symlink() => {
                        return Err(format!("record index directory is a symlink: {}", current.display()))
                    }
                    Ok(metadata) if !metadata.is_dir() => {
                        return Err(format!("record index directory is not a directory: {}", current.display()))
                    }
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        fs::create_dir(&current).map_err(|error| {
                            format!("could not create record index directory: {error}")
                        })?;
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt;
                            fs::set_permissions(&current, fs::Permissions::from_mode(0o700))
                                .map_err(|error| {
                                    format!("could not secure record index directory: {error}")
                                })?;
                        }
                    }
                    Err(error) => {
                        return Err(format!("could not inspect record index directory: {error}"))
                    }
                }
            }
            Component::CurDir => {}
            Component::ParentDir | Component::Prefix(_) => {
                return Err("record index directory contains an unsafe component".into())
            }
        }
    }
    Ok(())
}

fn sync_directory(path: &Path) -> std::io::Result<()> {
    fs::File::open(path)?.sync_all()
}

#[allow(dead_code)]
fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!("record index path is not a regular file: {}", path.display()));
    }
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    std::io::Read::by_ref(&mut file)
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > limit {
        return Err(format!("record index file exceeds {limit} bytes"));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Barrier};

    static TEST_ID: AtomicU64 = AtomicU64::new(0);

    fn temp_project(label: &str) -> (PathBuf, PathBuf) {
        let project = std::env::temp_dir().join(format!(
            "jet-record-index-{label}-{}-{}",
            std::process::id(),
            TEST_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let records = project.join(".jet").join("records");
        fs::create_dir_all(&records).unwrap();
        (project, records)
    }

    fn identity() -> RecordIdentity {
        RecordIdentity::new("a".repeat(64), "test-toolchain", "test-engine").unwrap()
    }

    fn entry(
        kind: RecordKind,
        artifact_id: &str,
        path: &str,
        size: u64,
        recorded_sequence: u64,
        saved: bool,
    ) -> RecordIndexEntry {
        RecordIndexEntry::new(identity(), kind, artifact_id, path)
            .unwrap()
            .with_size(size)
            .with_recorded_sequence(recorded_sequence)
            .with_saved(saved)
    }

    fn artifact(project: &Path, relative: &str, bytes: usize) -> PathBuf {
        let path = project.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, vec![b'x'; bytes]).unwrap();
        path
    }

    #[test]
    fn restart_eviction_removes_the_old_disposable_artifact() {
        let (project, records) = temp_project("restart");
        let old_path = artifact(&project, ".jet/receipts/old", 5);
        let new_path = artifact(&project, ".jet/receipts/new", 5);
        let budget = RecordBudget::new(5).unwrap();

        let mut index = RecordIndex::new(records.clone());
        index.set_budget(budget).unwrap();
        index
            .update(entry(
                RecordKind::Receipt,
                "old",
                ".jet/receipts/old",
                5,
                1,
                false,
            ))
            .unwrap();
        index.store().unwrap();

        let mut restarted = RecordIndex::load_with_budget(records.clone(), budget).unwrap();
        restarted
            .update_and_store(entry(
                RecordKind::Receipt,
                "new",
                ".jet/receipts/new",
                5,
                2,
                false,
            ))
            .unwrap();

        assert!(!old_path.exists());
        assert!(new_path.is_file());
        assert!(RecordIndex::load_with_budget(records, budget)
            .unwrap()
            .find(RecordKind::Receipt, "old", true)
            .is_none());
        let _ = fs::remove_dir_all(project);
    }

    #[test]
    fn saved_history_survives_disposable_eviction_after_restart() {
        let (project, records) = temp_project("saved");
        let saved_path = artifact(&project, ".jet/replays/saved", 100);
        let old_path = artifact(&project, ".jet/receipts/old", 4);
        let new_path = artifact(&project, ".jet/receipts/new", 4);
        let budget = RecordBudget::new(4).unwrap();

        let mut index = RecordIndex::new(records.clone());
        index.set_budget(budget).unwrap();
        index
            .update(entry(
                RecordKind::Replay,
                "saved",
                ".jet/replays/saved",
                100,
                1,
                true,
            ))
            .unwrap();
        index
            .update(entry(
                RecordKind::Receipt,
                "old",
                ".jet/receipts/old",
                4,
                2,
                false,
            ))
            .unwrap();
        index.store().unwrap();

        let mut restarted = RecordIndex::load_with_budget(records.clone(), budget).unwrap();
        restarted
            .update_and_store(entry(
                RecordKind::Receipt,
                "new",
                ".jet/receipts/new",
                4,
                3,
                false,
            ))
            .unwrap();

        let loaded = RecordIndex::load_with_budget(records, budget).unwrap();
        assert!(loaded.find(RecordKind::Replay, "saved", true).is_some());
        assert!(saved_path.is_file());
        assert!(!old_path.exists());
        assert!(new_path.is_file());
        let _ = fs::remove_dir_all(project);
    }

    #[test]
    fn concurrent_update_and_store_preserves_each_row() {
        let (project, records) = temp_project("concurrent");
        let workers = 4usize;
        let barrier = Arc::new(Barrier::new(workers));
        let mut handles = Vec::with_capacity(workers);
        for worker in 0..workers {
            let records = records.clone();
            let barrier = Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                let mut index = RecordIndex::new(records);
                index
                    .update_and_store(entry(
                        RecordKind::Receipt,
                        &format!("receipt-{worker}"),
                        &format!(".jet/receipts/receipt-{worker}"),
                        1,
                        worker as u64 + 1,
                        false,
                    ))
                    .unwrap();
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }

        let loaded = RecordIndex::load(records).unwrap();
        assert_eq!(loaded.query(true).len(), workers);
        for worker in 0..workers {
            assert!(loaded
                .find(RecordKind::Receipt, &format!("receipt-{worker}"), true)
                .is_some());
        }
        let _ = fs::remove_dir_all(project);
    }

    #[test]
    fn oversized_disposable_update_does_not_corrupt_the_previous_index() {
        let (project, records) = temp_project("oversized");
        let old_path = artifact(&project, ".jet/receipts/old", 1);
        let new_path = artifact(&project, ".jet/receipts/too-large", 2);
        let budget = RecordBudget::new(1).unwrap();

        let mut index = RecordIndex::new(records.clone());
        index.set_budget(budget).unwrap();
        index
            .update_and_store(entry(
                RecordKind::Receipt,
                "old",
                ".jet/receipts/old",
                1,
                1,
                false,
            ))
            .unwrap();

        let mut oversized = RecordIndex::new(records.clone());
        oversized.set_budget(budget).unwrap();
        let error = oversized
            .update_and_store(entry(
                RecordKind::Receipt,
                "too-large",
                ".jet/receipts/too-large",
                2,
                2,
                false,
            ))
            .expect_err("an oversized disposable record must be rejected");
        assert!(error.contains("exceeds the byte budget"));

        let loaded = RecordIndex::load_with_budget(records, budget).unwrap();
        assert!(loaded.find(RecordKind::Receipt, "old", true).is_some());
        assert!(old_path.is_file());
        assert!(new_path.is_file());
        let _ = fs::remove_dir_all(project);
    }
    #[test]
    fn default_budget_retains_many_run_build_test_and_replay_rows_without_count_cap() {
        let (project, records) = temp_project("default-budget");
        let mut index = RecordIndex::new(records.clone());
        assert_eq!(
            index.budget().max_bytes,
            256 * 1024 * 1024,
            "the project budget is one 256 MiB byte limit"
        );

        let mut sequence = 1_u64;
        for (kind, label, prefix, count) in [
            (RecordKind::Receipt, "run", ".jet/receipts", 80_usize),
            (RecordKind::Receipt, "build", ".jet/receipts", 80_usize),
            (RecordKind::Receipt, "test", ".jet/receipts", 80_usize),
            (RecordKind::Replay, "replay", ".jet/replays", 80_usize),
        ] {
            for offset in 0..count {
                index
                    .update(entry(
                        kind,
                        &format!("{label}-{offset}"),
                        &format!("{prefix}/{label}-{offset}"),
                        1,
                        sequence,
                        false,
                    ))
                    .unwrap();
                sequence += 1;
            }
        }

        assert_eq!(
            index.entries_authenticated().len(),
            320,
            "ordinary history has no record-count eviction policy"
        );
        index.store().unwrap();
        let loaded = RecordIndex::load(records).unwrap();
        assert_eq!(loaded.entries_authenticated().len(), 320);
        let _ = fs::remove_dir_all(project);
    }

    #[test]
    fn protected_sensitive_saved_and_linked_rows_survive_selective_eviction_and_failed_publish() {
        let (project, records) = temp_project("protected");
        let linked_path = artifact(&project, ".jet/receipts/linked", 1);
        let old_path = artifact(&project, ".jet/receipts/old", 1);
        let sensitive_path = artifact(&project, ".jet/receipts/sensitive", 8);
        let saved_path = artifact(&project, ".jet/replays/saved", 32);
        let proof_path = artifact(&project, ".jet/proofs/audit", 32);
        let trust_path = artifact(&project, ".jet/trust/audit", 32);
        let evidence_path = artifact(&project, ".jet/evidence/mandatory", 32);
        let new_path = artifact(&project, ".jet/receipts/new", 1);
        let oversized_path = artifact(&project, ".jet/receipts/oversized", 3);
        let budget = RecordBudget::new(2).unwrap();

        let linked = entry(
            RecordKind::Receipt,
            "linked",
            ".jet/receipts/linked",
            1,
            1,
            false,
        );
        let old = entry(
            RecordKind::Receipt,
            "old",
            ".jet/receipts/old",
            1,
            2,
            false,
        );
        let sensitive = entry(
            RecordKind::Receipt,
            "sensitive",
            ".jet/receipts/sensitive",
            8,
            3,
            false,
        )
        .with_capture(RecordCapture::Sensitive);
        let saved = entry(
            RecordKind::Replay,
            "saved",
            ".jet/replays/saved",
            32,
            4,
            true,
        );
        let proof = entry(
            RecordKind::Proof,
            "audit-proof",
            ".jet/proofs/audit",
            32,
            5,
            false,
        )
        .with_links(
            vec![RecordLink::new(RecordKind::Receipt, "linked").unwrap()],
            Vec::new(),
        )
        .unwrap();
        let trust = entry(
            RecordKind::Trust,
            "audit-trust",
            ".jet/trust/audit",
            32,
            6,
            false,
        )
        .with_links(
            Vec::new(),
            vec![RecordLink::new(RecordKind::Receipt, "sensitive").unwrap()],
        )
        .unwrap();
        let evidence = entry(
            RecordKind::Evidence,
            "mandatory",
            ".jet/evidence/mandatory",
            32,
            7,
            false,
        )
        .with_capture(RecordCapture::Sensitive);

        let mut index = RecordIndex::new(records.clone());
        index.set_budget(budget).unwrap();
        for row in [linked, old, sensitive, saved, proof, trust, evidence] {
            index.update(row).unwrap();
        }
        index.store().unwrap();

        let mut restarted = RecordIndex::load_with_budget(records.clone(), budget).unwrap();
        restarted
            .update_and_store(entry(
                RecordKind::Receipt,
                "new",
                ".jet/receipts/new",
                1,
                8,
                false,
            ))
            .unwrap();

        let loaded = RecordIndex::load_with_budget(records.clone(), budget).unwrap();
        assert!(!old_path.exists(), "only the unreferenced FIFO row is evicted");
        assert!(new_path.is_file());
        assert!(linked_path.is_file(), "linked rows cannot be evicted");
        assert!(saved_path.is_file(), "saved replay bytes are protected");
        assert!(sensitive_path.is_file(), "sensitive bytes are protected");
        assert!(proof_path.is_file(), "proof/audit bytes are protected");
        assert!(trust_path.is_file(), "trust/audit bytes are protected");
        assert!(evidence_path.is_file(), "mandatory evidence bytes are protected");
        assert!(loaded.find(RecordKind::Receipt, "old", true).is_none());
        assert!(loaded.find(RecordKind::Receipt, "new", true).is_some());
        assert!(loaded.find(RecordKind::Receipt, "sensitive", false).is_none());
        assert!(loaded.find(RecordKind::Receipt, "sensitive", true).is_some());
        let visible_trust = loaded
            .find(RecordKind::Trust, "audit-trust", false)
            .unwrap();
        assert!(
            visible_trust.produced.is_empty(),
            "unauthenticated queries hide sensitive references"
        );
        let authenticated_trust = loaded
            .find(RecordKind::Trust, "audit-trust", true)
            .unwrap();
        assert_eq!(authenticated_trust.produced.len(), 1);

        // A failed publication must not replace the last valid index or remove
        // protected data. This uses the same bounded candidate path as a
        // disk-full/oversized write failure without depending on host quotas.
        let mut failed = RecordIndex::load_with_budget(records.clone(), budget).unwrap();
        let error = failed
            .update_and_store(entry(
                RecordKind::Receipt,
                "oversized",
                ".jet/receipts/oversized",
                3,
                9,
                false,
            ))
            .expect_err("an oversized disposable row must fail before publication");
        assert!(error.contains("exceeds"));
        let after_failure = RecordIndex::load_with_budget(records.clone(), budget).unwrap();
        assert!(after_failure.find(RecordKind::Receipt, "new", true).is_some());
        assert!(after_failure.find(RecordKind::Replay, "saved", true).is_some());
        assert!(after_failure.find(RecordKind::Proof, "audit-proof", true).is_some());
        assert!(saved_path.is_file());
        assert!(proof_path.is_file());
        assert!(oversized_path.is_file(), "the failed candidate never owns this path");

        // An interrupted/stale staging file is ignored; the published JSONL
        // remains the source of truth.
        fs::write(records.join(".index.jsonl.interrupted"), b"partial").unwrap();
        let after_interruption = RecordIndex::load_with_budget(records, budget).unwrap();
        assert!(after_interruption
            .find(RecordKind::Trust, "audit-trust", true)
            .is_some());
        let _ = fs::remove_dir_all(project);
    }
}
