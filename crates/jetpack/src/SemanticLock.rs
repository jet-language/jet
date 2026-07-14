//! Explainable semantic lock records and merge support (D-WD4 / E4-JP13).
//!
//! One live lock path: semantic records, transitive input graph, source maps,
//! and overlay invalidation facts share `.jet/lock` with machine package /
//! toolchain sections. Human rationale never enters the machine identity key.
//! Every atomic write revalidates graph satisfiability, signatures, domain
//! consistency, source authority, and offline completeness first.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum LockRecordKind {
    Package,
    SourceRef,
    WorkspaceMember,
    AdapterOutput,
    SourceBuild,
    CacheObject,
    Toolchain,
    SecretReference,
    ServicePackage,
    ImageInput,
    FleetInput,
    JetosActivationClosure,
    ReplacementOverlay,
    PackageOverlay,
    /// Selected typed variant domain (E4-JP15 / D-JPK-VARIANT1).
    Variant,
    Future(String),
}

impl LockRecordKind {
    pub fn as_str(&self) -> &str {
        match self {
            LockRecordKind::Package => "package",
            LockRecordKind::SourceRef => "source-ref",
            LockRecordKind::WorkspaceMember => "workspace-member",
            LockRecordKind::AdapterOutput => "adapter-output",
            LockRecordKind::SourceBuild => "source-build",
            LockRecordKind::CacheObject => "cache-object",
            LockRecordKind::Toolchain => "toolchain",
            LockRecordKind::SecretReference => "secret-reference",
            LockRecordKind::ServicePackage => "service-package",
            LockRecordKind::ImageInput => "image-input",
            LockRecordKind::FleetInput => "fleet-input",
            LockRecordKind::JetosActivationClosure => "jetos-activation-closure",
            LockRecordKind::ReplacementOverlay => "replacement-overlay",
            LockRecordKind::PackageOverlay => "package-overlay",
            LockRecordKind::Variant => "variant",
            LockRecordKind::Future(s) => s.as_str(),
        }
    }

    pub fn parse(raw: &str) -> LockRecordKind {
        match raw {
            "package" => LockRecordKind::Package,
            "source-ref" => LockRecordKind::SourceRef,
            "workspace-member" => LockRecordKind::WorkspaceMember,
            "adapter-output" => LockRecordKind::AdapterOutput,
            "source-build" => LockRecordKind::SourceBuild,
            "cache-object" => LockRecordKind::CacheObject,
            "toolchain" => LockRecordKind::Toolchain,
            "secret-reference" => LockRecordKind::SecretReference,
            "service-package" => LockRecordKind::ServicePackage,
            "image-input" => LockRecordKind::ImageInput,
            "fleet-input" => LockRecordKind::FleetInput,
            "jetos-activation-closure" => LockRecordKind::JetosActivationClosure,
            "replacement-overlay" => LockRecordKind::ReplacementOverlay,
            "package-overlay" => LockRecordKind::PackageOverlay,
            "variant" => LockRecordKind::Variant,
            other => LockRecordKind::Future(other.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockIdentity {
    pub kind: LockRecordKind,
    pub key: String,
    pub exact: String,
    pub hash: String,
    pub platform: String,
}

impl LockIdentity {
    pub fn semantic_key(&self) -> String {
        format!("{}:{}", self.kind.as_str(), self.key)
    }

    pub fn machine_identity(&self) -> String {
        format!(
            "{}:{}:{}:{}:{}",
            self.kind.as_str(),
            self.key,
            self.exact,
            self.hash,
            self.platform
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LockRationale {
    pub owner_package: String,
    pub reason: String,
    pub source_ref: String,
    pub provider: String,
    pub channel_input: String,
    pub exact_output: String,
    pub policy_fingerprint: String,
    pub recipe_id: String,
    pub adapter_id: String,
    pub signature: String,
    pub cache_provenance: String,
    pub update_command: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticRecord {
    pub identity: LockIdentity,
    pub rationales: Vec<LockRationale>,
    pub future_fields: BTreeMap<String, String>,
}

impl SemanticRecord {
    pub fn new(identity: LockIdentity, rationale: LockRationale) -> SemanticRecord {
        SemanticRecord {
            identity,
            rationales: vec![rationale],
            future_fields: BTreeMap::new(),
        }
    }
}

/// One flake-style lock input with optional `follows` edge (E4-JP13).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LockInput {
    pub name: String,
    pub url: String,
    pub follows: String,
}

/// Package-pattern → allowed source authorities (dependency-confusion guard).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceMapEntry {
    pub pattern: String,
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SemanticLockFile {
    pub records: Vec<SemanticRecord>,
    pub inputs: Vec<LockInput>,
    pub source_maps: Vec<SourceMapEntry>,
}

impl SemanticLockFile {
    pub fn with_records(records: Vec<SemanticRecord>) -> SemanticLockFile {
        SemanticLockFile {
            records,
            ..Default::default()
        }
    }
}

pub fn write(lock: &SemanticLockFile) -> String {
    let mut records = lock.records.clone();
    records.sort_by(|a, b| a.identity.semantic_key().cmp(&b.identity.semantic_key()));
    let mut inputs = lock.inputs.clone();
    inputs.sort();
    let mut source_maps = lock.source_maps.clone();
    source_maps.sort();
    let mut out = String::from("semantic-lock-version = 1\n");
    for input in inputs {
        out.push_str("\n[[lock_input]]\n");
        out.push_str(&line("name", &input.name));
        out.push_str(&line("url", &input.url));
        out.push_str(&line("follows", &input.follows));
    }
    for map in source_maps {
        out.push_str("\n[[source_map]]\n");
        out.push_str(&line("pattern", &map.pattern));
        out.push_str(&line("sources", &format!("[{}]", map.sources.iter().map(|s| format!("\"{s}\"")).collect::<Vec<_>>().join(", "))));
    }
    for rec in records {
        out.push_str("\n[[semantic_record]]\n");
        out.push_str(&line("kind", rec.identity.kind.as_str()));
        out.push_str(&line("key", &rec.identity.key));
        out.push_str(&line("exact", &rec.identity.exact));
        out.push_str(&line("hash", &rec.identity.hash));
        out.push_str(&line("platform", &rec.identity.platform));
        for (k, v) in rec.future_fields {
            out.push_str(&line(&k, &v));
        }
        for rationale in rec.rationales {
            out.push_str("[[semantic_record.rationale]]\n");
            out.push_str(&line("owner-package", &rationale.owner_package));
            out.push_str(&line("reason", &rationale.reason));
            out.push_str(&line("source-ref", &rationale.source_ref));
            out.push_str(&line("provider", &rationale.provider));
            out.push_str(&line("channel-input", &rationale.channel_input));
            out.push_str(&line("exact-output", &rationale.exact_output));
            out.push_str(&line("policy-fingerprint", &rationale.policy_fingerprint));
            out.push_str(&line("recipe-id", &rationale.recipe_id));
            out.push_str(&line("adapter-id", &rationale.adapter_id));
            out.push_str(&line("signature", &rationale.signature));
            out.push_str(&line("cache-provenance", &rationale.cache_provenance));
            out.push_str(&line("update-command", &rationale.update_command));
        }
    }
    out
}

fn line(key: &str, value: &str) -> String {
    format!(
        "{key} = \"{}\"\n",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    )
}

pub fn parse(raw: &str) -> SemanticLockFile {
    let mut records = Vec::new();
    let mut inputs = Vec::new();
    let mut source_maps = Vec::new();
    let mut current: Option<PartialRecord> = None;
    let mut current_rationale: Option<LockRationale> = None;
    let mut current_input: Option<PartialInput> = None;
    let mut current_source_map: Option<PartialSourceMap> = None;

    for raw_line in raw.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match line {
            "[[semantic_record]]" => {
                flush_rationale(&mut current, &mut current_rationale);
                flush_record(&mut records, &mut current);
                flush_input(&mut inputs, &mut current_input);
                flush_source_map(&mut source_maps, &mut current_source_map);
                current = Some(PartialRecord::default());
                continue;
            }
            "[[semantic_record.rationale]]" => {
                flush_rationale(&mut current, &mut current_rationale);
                current_rationale = Some(LockRationale::default());
                continue;
            }
            "[[lock_input]]" => {
                flush_rationale(&mut current, &mut current_rationale);
                flush_record(&mut records, &mut current);
                flush_input(&mut inputs, &mut current_input);
                flush_source_map(&mut source_maps, &mut current_source_map);
                current_input = Some(PartialInput::default());
                continue;
            }
            "[[source_map]]" => {
                flush_rationale(&mut current, &mut current_rationale);
                flush_record(&mut records, &mut current);
                flush_input(&mut inputs, &mut current_input);
                flush_source_map(&mut source_maps, &mut current_source_map);
                current_source_map = Some(PartialSourceMap::default());
                continue;
            }
            _ => {}
        }
        let Some((key, val)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let val = val
            .trim()
            .trim_matches('"')
            .replace("\\\"", "\"")
            .replace("\\\\", "\\");
        if let Some(rationale) = &mut current_rationale {
            set_rationale(rationale, key, val);
        } else if let Some(input) = &mut current_input {
            input.set(key, val);
        } else if let Some(map) = &mut current_source_map {
            map.set(key, val);
        } else if let Some(record) = &mut current {
            record.set(key, val);
        }
    }
    flush_rationale(&mut current, &mut current_rationale);
    flush_record(&mut records, &mut current);
    flush_input(&mut inputs, &mut current_input);
    flush_source_map(&mut source_maps, &mut current_source_map);
    SemanticLockFile {
        records,
        inputs,
        source_maps,
    }
}

fn flush_rationale(
    current: &mut Option<PartialRecord>,
    current_rationale: &mut Option<LockRationale>,
) {
    if let (Some(record), Some(rationale)) = (current, current_rationale.take()) {
        record.rationales.push(rationale);
    }
}

fn flush_record(records: &mut Vec<SemanticRecord>, current: &mut Option<PartialRecord>) {
    if let Some(record) = current.take().and_then(PartialRecord::finish) {
        records.push(record);
    }
}

fn flush_input(inputs: &mut Vec<LockInput>, current: &mut Option<PartialInput>) {
    if let Some(input) = current.take().and_then(PartialInput::finish) {
        inputs.push(input);
    }
}

fn flush_source_map(maps: &mut Vec<SourceMapEntry>, current: &mut Option<PartialSourceMap>) {
    if let Some(map) = current.take().and_then(PartialSourceMap::finish) {
        maps.push(map);
    }
}

fn set_rationale(r: &mut LockRationale, key: &str, value: String) {
    match key {
        "owner-package" => r.owner_package = value,
        "reason" => r.reason = value,
        "source-ref" => r.source_ref = value,
        "provider" => r.provider = value,
        "channel-input" => r.channel_input = value,
        "exact-output" => r.exact_output = value,
        "policy-fingerprint" => r.policy_fingerprint = value,
        "recipe-id" => r.recipe_id = value,
        "adapter-id" => r.adapter_id = value,
        "signature" => r.signature = value,
        "cache-provenance" => r.cache_provenance = value,
        "update-command" => r.update_command = value,
        _ => {}
    }
}

#[derive(Default)]
struct PartialInput {
    name: Option<String>,
    url: Option<String>,
    follows: String,
}

impl PartialInput {
    fn set(&mut self, key: &str, value: String) {
        match key {
            "name" => self.name = Some(value),
            "url" => self.url = Some(value),
            "follows" => self.follows = value,
            _ => {}
        }
    }

    fn finish(self) -> Option<LockInput> {
        Some(LockInput {
            name: self.name?,
            url: self.url.unwrap_or_default(),
            follows: self.follows,
        })
    }
}

#[derive(Default)]
struct PartialSourceMap {
    pattern: Option<String>,
    sources: Vec<String>,
}

impl PartialSourceMap {
    fn set(&mut self, key: &str, value: String) {
        match key {
            "pattern" => self.pattern = Some(value),
            "sources" => self.sources = parse_string_list(&value),
            "source" => self.sources.push(value),
            _ => {}
        }
    }

    fn finish(self) -> Option<SourceMapEntry> {
        Some(SourceMapEntry {
            pattern: self.pattern?,
            sources: self.sources,
        })
    }
}

fn parse_string_list(raw: &str) -> Vec<String> {
    let raw = raw.trim().trim_start_matches('[').trim_end_matches(']');
    if raw.is_empty() {
        return Vec::new();
    }
    raw.split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[derive(Default)]
struct PartialRecord {
    kind: Option<LockRecordKind>,
    key: Option<String>,
    exact: Option<String>,
    hash: Option<String>,
    platform: Option<String>,
    rationales: Vec<LockRationale>,
    future_fields: BTreeMap<String, String>,
}

impl PartialRecord {
    fn set(&mut self, key: &str, value: String) {
        match key {
            "kind" => self.kind = Some(LockRecordKind::parse(&value)),
            "key" => self.key = Some(value),
            "exact" => self.exact = Some(value),
            "hash" => self.hash = Some(value),
            "platform" => self.platform = Some(value),
            other if other != "semantic-lock-version" => {
                self.future_fields.insert(other.to_string(), value);
            }
            _ => {}
        }
    }

    fn finish(self) -> Option<SemanticRecord> {
        Some(SemanticRecord {
            identity: LockIdentity {
                kind: self.kind?,
                key: self.key?,
                exact: self.exact.unwrap_or_default(),
                hash: self.hash.unwrap_or_default(),
                platform: self.platform.unwrap_or_default(),
            },
            rationales: self.rationales,
            future_fields: self.future_fields,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockConflict {
    pub semantic_key: String,
    pub owner_package: String,
    pub left_identity: String,
    pub right_identity: String,
    pub left_reason: String,
    pub right_reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeOutcome {
    pub merged: SemanticLockFile,
    pub conflicts: Vec<LockConflict>,
}

pub fn merge(
    base: &SemanticLockFile,
    left: &SemanticLockFile,
    right: &SemanticLockFile,
) -> MergeOutcome {
    let base_by_key = records_by_key(base);
    let left_by_key = records_by_key(left);
    let right_by_key = records_by_key(right);
    let mut keys = BTreeSet::new();
    keys.extend(left_by_key.keys().cloned());
    keys.extend(right_by_key.keys().cloned());

    let mut by_key = BTreeMap::new();
    let mut conflicts = Vec::new();
    for key in keys {
        match (left_by_key.get(&key), right_by_key.get(&key)) {
            (Some(left_rec), None) => {
                by_key.insert(key, (*left_rec).clone());
            }
            (None, Some(right_rec)) => {
                by_key.insert(key, (*right_rec).clone());
            }
            (Some(left_rec), Some(right_rec)) => {
                if left_rec.identity.machine_identity() == right_rec.identity.machine_identity() {
                    let mut merged = (*left_rec).clone();
                    merge_rationales(&mut merged, &right_rec.rationales);
                    by_key.insert(key, merged);
                    continue;
                }
                let base_rec = base_by_key.get(&key).copied();
                if base_rec.is_some_and(|base_rec| {
                    left_rec.identity.machine_identity() == base_rec.identity.machine_identity()
                }) {
                    by_key.insert(key, (*right_rec).clone());
                    continue;
                }
                if base_rec.is_some_and(|base_rec| {
                    right_rec.identity.machine_identity() == base_rec.identity.machine_identity()
                }) {
                    by_key.insert(key, (*left_rec).clone());
                    continue;
                }
                if let Some(conflict) = conflict_for(left_rec, right_rec) {
                    conflicts.push(conflict);
                } else {
                    let mut merged = (*left_rec).clone();
                    merge_rationales(&mut merged, &right_rec.rationales);
                    by_key.insert(key, merged);
                }
            }
            (None, None) => {}
        }
    }
    MergeOutcome {
        merged: SemanticLockFile {
            records: by_key.into_values().collect(),
            inputs: merge_inputs(base, left, right),
            source_maps: merge_source_maps(base, left, right),
        },
        conflicts,
    }
}

fn merge_inputs(
    base: &SemanticLockFile,
    left: &SemanticLockFile,
    right: &SemanticLockFile,
) -> Vec<LockInput> {
    let mut by_name: BTreeMap<String, LockInput> = BTreeMap::new();
    for input in base
        .inputs
        .iter()
        .chain(left.inputs.iter())
        .chain(right.inputs.iter())
    {
        by_name.insert(input.name.clone(), input.clone());
    }
    // Prefer right over left over base for same name when urls differ.
    for input in left.inputs.iter().chain(right.inputs.iter()) {
        by_name.insert(input.name.clone(), input.clone());
    }
    by_name.into_values().collect()
}

fn merge_source_maps(
    base: &SemanticLockFile,
    left: &SemanticLockFile,
    right: &SemanticLockFile,
) -> Vec<SourceMapEntry> {
    let mut by_pattern: BTreeMap<String, SourceMapEntry> = BTreeMap::new();
    for map in base
        .source_maps
        .iter()
        .chain(left.source_maps.iter())
        .chain(right.source_maps.iter())
    {
        by_pattern.insert(map.pattern.clone(), map.clone());
    }
    for map in left.source_maps.iter().chain(right.source_maps.iter()) {
        by_pattern.insert(map.pattern.clone(), map.clone());
    }
    by_pattern.into_values().collect()
}

fn records_by_key(lock: &SemanticLockFile) -> BTreeMap<String, &SemanticRecord> {
    lock.records
        .iter()
        .map(|record| (record.identity.semantic_key(), record))
        .collect()
}

fn merge_rationales(existing: &mut SemanticRecord, incoming: &[LockRationale]) {
    let mut seen: BTreeSet<(String, String)> = existing
        .rationales
        .iter()
        .map(|r| (r.owner_package.clone(), r.reason.clone()))
        .collect();
    for r in incoming {
        if seen.insert((r.owner_package.clone(), r.reason.clone())) {
            existing.rationales.push(r.clone());
        }
    }
}

fn conflict_for(left: &SemanticRecord, right: &SemanticRecord) -> Option<LockConflict> {
    for l in &left.rationales {
        for r in &right.rationales {
            if l.owner_package == r.owner_package {
                return Some(LockConflict {
                    semantic_key: left.identity.semantic_key(),
                    owner_package: l.owner_package.clone(),
                    left_identity: left.identity.machine_identity(),
                    right_identity: right.identity.machine_identity(),
                    left_reason: l.reason.clone(),
                    right_reason: r.reason.clone(),
                });
            }
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplainFact {
    pub semantic_key: String,
    pub owners: Vec<String>,
    pub provider: String,
    pub platform: String,
    pub exact_artifact: String,
    pub policy_fingerprint: String,
    pub update_command: String,
    pub offline_satisfied: bool,
}

pub fn explain(lock: &SemanticLockFile, key: &str) -> Option<ExplainFact> {
    let rec = lock
        .records
        .iter()
        .find(|r| r.identity.semantic_key() == key)?;
    let mut owners: Vec<String> = rec
        .rationales
        .iter()
        .map(|r| r.owner_package.clone())
        .filter(|s| !s.is_empty())
        .collect();
    owners.sort();
    owners.dedup();
    let first = rec.rationales.first().cloned().unwrap_or_default();
    Some(ExplainFact {
        semantic_key: key.to_string(),
        owners,
        provider: first.provider,
        platform: rec.identity.platform.clone(),
        exact_artifact: rec.identity.exact.clone(),
        policy_fingerprint: first.policy_fingerprint,
        update_command: first.update_command,
        offline_satisfied: !rec.identity.hash.is_empty(),
    })
}

/// Record a selected typed variant into the semantic lock (E4-JP15).
/// `platform` holds the canonical `PackageVariant::identity_key()`.
pub fn record_selected_variant(
    lock: &mut SemanticLockFile,
    package: &str,
    variant_identity: &str,
    hash: &str,
    reason: &str,
) {
    let identity = LockIdentity {
        kind: LockRecordKind::Variant,
        key: package.to_string(),
        exact: variant_identity.to_string(),
        hash: hash.to_string(),
        platform: variant_identity.to_string(),
    };
    let rationale = LockRationale {
        owner_package: package.to_string(),
        reason: reason.to_string(),
        source_ref: String::new(),
        provider: "native".to_string(),
        channel_input: String::new(),
        exact_output: variant_identity.to_string(),
        policy_fingerprint: String::new(),
        recipe_id: String::new(),
        adapter_id: String::new(),
        signature: String::new(),
        cache_provenance: String::new(),
        update_command: String::new(),
    };
    // Replace existing variant record for the same package key.
    lock.records
        .retain(|r| !(r.identity.kind == LockRecordKind::Variant && r.identity.key == package));
    lock.records
        .push(SemanticRecord::new(identity, rationale));
}

/// Identity keys of every locked variant domain (universal lock coverage).
pub fn locked_variant_domains(lock: &SemanticLockFile) -> BTreeSet<String> {
    lock.records
        .iter()
        .filter(|r| r.identity.kind == LockRecordKind::Variant)
        .map(|r| r.identity.exact.clone())
        .collect()
}

/// Record a workspace catalog selection into the semantic lock (E4-JP13).
pub fn record_catalog_selection(
    lock: &mut SemanticLockFile,
    owner: &str,
    logical_name: &str,
    provider_ref: &str,
    hash: &str,
    platform: &str,
    rationale: &str,
) {
    let identity = LockIdentity {
        kind: LockRecordKind::Package,
        key: logical_name.to_string(),
        exact: provider_ref.to_string(),
        hash: hash.to_string(),
        platform: platform.to_string(),
    };
    let rationale = LockRationale {
        owner_package: owner.to_string(),
        reason: rationale.to_string(),
        source_ref: format!("catalog:{logical_name}"),
        provider: provider_ref
            .split(':')
            .next()
            .unwrap_or("catalog")
            .to_string(),
        channel_input: String::new(),
        exact_output: provider_ref.to_string(),
        policy_fingerprint: String::new(),
        recipe_id: String::new(),
        adapter_id: "workspace.catalog".to_string(),
        signature: String::new(),
        cache_provenance: String::new(),
        update_command: format!("jet update {logical_name}"),
    };
    lock.records
        .retain(|r| !(r.identity.kind == LockRecordKind::Package && r.identity.key == logical_name));
    lock.records
        .push(SemanticRecord::new(identity, rationale));
}

// ── E4-JP13 live path, selective update, revalidation ───────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationIssue {
    InputCycle(String),
    MissingFollows { input: String, follows: String },
    MissingHash { semantic_key: String },
    MissingSignature { semantic_key: String },
    DomainConflict { key: String, left: String, right: String },
    SourceAuthority { package: String, source: String, allowed: Vec<String> },
    OfflineIncomplete { semantic_key: String },
}

impl ValidationIssue {
    pub fn message(&self) -> String {
        match self {
            ValidationIssue::InputCycle(name) => {
                format!("lock input `{name}` participates in a follows cycle")
            }
            ValidationIssue::MissingFollows { input, follows } => {
                format!("lock input `{input}` follows unknown input `{follows}`")
            }
            ValidationIssue::MissingHash { semantic_key } => {
                format!("`{semantic_key}` has empty content hash — offline realize cannot run")
            }
            ValidationIssue::MissingSignature { semantic_key } => {
                format!("`{semantic_key}` requires a signature but the slot is empty")
            }
            ValidationIssue::DomainConflict { key, left, right } => {
                format!("platform domain conflict for `{key}`: `{left}` vs `{right}`")
            }
            ValidationIssue::SourceAuthority {
                package,
                source,
                allowed,
            } => format!(
                "package `{package}` source `{source}` is not in source map [{}]",
                allowed.join(", ")
            ),
            ValidationIssue::OfflineIncomplete { semantic_key } => {
                format!("`{semantic_key}` is not offline-complete")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockCommitError {
    pub issues: Vec<ValidationIssue>,
    pub io: Option<String>,
}

impl LockCommitError {
    pub fn message(&self) -> String {
        if let Some(io) = &self.io {
            return io.clone();
        }
        self.issues
            .iter()
            .map(ValidationIssue::message)
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// Revalidate before any atomic write (E4-JP13).
pub fn revalidate(lock: &SemanticLockFile) -> Result<(), Vec<ValidationIssue>> {
    let mut issues = Vec::new();
    issues.extend(validate_input_graph(&lock.inputs));
    issues.extend(validate_records(lock));
    issues.extend(validate_source_maps(lock));
    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

fn validate_input_graph(inputs: &[LockInput]) -> Vec<ValidationIssue> {
    let names: BTreeSet<String> = inputs.iter().map(|i| i.name.clone()).collect();
    let mut issues = Vec::new();
    for input in inputs {
        if !input.follows.is_empty() && !names.contains(&input.follows) {
            issues.push(ValidationIssue::MissingFollows {
                input: input.name.clone(),
                follows: input.follows.clone(),
            });
        }
    }
    let mut adj: BTreeMap<String, String> = BTreeMap::new();
    for input in inputs {
        if !input.follows.is_empty() && names.contains(&input.follows) {
            adj.insert(input.name.clone(), input.follows.clone());
        }
    }
    let mut reported = BTreeSet::new();
    for name in &names {
        if let Some(cycle_node) = find_cycle_from(name, &adj) {
            if reported.insert(cycle_node.clone()) {
                issues.push(ValidationIssue::InputCycle(cycle_node));
            }
        }
    }
    issues
}

fn find_cycle_from(start: &str, adj: &BTreeMap<String, String>) -> Option<String> {
    let mut slow = start.to_string();
    let mut fast = start.to_string();
    loop {
        let Some(next_slow) = adj.get(&slow) else {
            return None;
        };
        slow = next_slow.clone();
        let Some(next_fast) = adj.get(&fast).and_then(|n| adj.get(n)) else {
            return None;
        };
        fast = next_fast.clone();
        if slow == fast {
            return Some(slow);
        }
    }
}

fn validate_records(lock: &SemanticLockFile) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    let mut seen_platform: BTreeMap<String, String> = BTreeMap::new();
    for rec in &lock.records {
        let key = rec.identity.semantic_key();
        if rec.identity.hash.is_empty() {
            issues.push(ValidationIssue::MissingHash {
                semantic_key: key.clone(),
            });
            issues.push(ValidationIssue::OfflineIncomplete {
                semantic_key: key.clone(),
            });
        }
        for rationale in &rec.rationales {
            if rationale.cache_provenance.contains("signed") && rationale.signature.is_empty() {
                issues.push(ValidationIssue::MissingSignature {
                    semantic_key: key.clone(),
                });
            }
        }
        if !rec.identity.platform.is_empty() {
            if let Some(prev) = seen_platform.get(&key) {
                if prev != &rec.identity.platform {
                    issues.push(ValidationIssue::DomainConflict {
                        key: key.clone(),
                        left: prev.clone(),
                        right: rec.identity.platform.clone(),
                    });
                }
            } else {
                seen_platform.insert(key, rec.identity.platform.clone());
            }
        }
    }
    issues
}

fn validate_source_maps(lock: &SemanticLockFile) -> Vec<ValidationIssue> {
    if lock.source_maps.is_empty() {
        return Vec::new();
    }
    let mut issues = Vec::new();
    for rec in &lock.records {
        if rec.identity.kind != LockRecordKind::Package {
            continue;
        }
        let Some(map) = matching_source_map(lock, &rec.identity.key) else {
            continue;
        };
        let source = rec
            .rationales
            .first()
            .map(|r| {
                if !r.source_ref.is_empty() {
                    r.source_ref.clone()
                } else {
                    r.provider.clone()
                }
            })
            .unwrap_or_default();
        if source.is_empty() {
            continue;
        }
        let allowed = &map.sources;
        let ok = allowed.iter().any(|a| source_matches(a, &source));
        if !ok {
            issues.push(ValidationIssue::SourceAuthority {
                package: rec.identity.key.clone(),
                source,
                allowed: allowed.clone(),
            });
        }
    }
    issues
}

fn matching_source_map<'a>(
    lock: &'a SemanticLockFile,
    package: &str,
) -> Option<&'a SourceMapEntry> {
    lock.source_maps.iter().find(|m| pattern_matches(&m.pattern, package))
}

fn pattern_matches(pattern: &str, package: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix('*') {
        package.starts_with(prefix)
    } else {
        pattern == package
    }
}

fn source_matches(allowed: &str, source: &str) -> bool {
    source == allowed || source.starts_with(allowed) || allowed.starts_with(source)
}

/// Selective update: replace one semantic key; unrelated records stay stable.
pub fn selective_update(
    lock: &mut SemanticLockFile,
    new_record: SemanticRecord,
) -> Option<SemanticRecord> {
    let key = new_record.identity.semantic_key();
    let previous = lock
        .records
        .iter()
        .position(|r| r.identity.semantic_key() == key)
        .map(|idx| lock.records.remove(idx));
    lock.records.push(new_record);
    previous
}

/// Exact overlay/patch invalidation facts (E4-JP13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayInvalidation {
    pub overlay: String,
    pub package: String,
    pub policy_fingerprint_before: String,
    pub policy_fingerprint_after: String,
    pub affected_action_keys: Vec<String>,
    pub reason: String,
}

/// Diff two overlay policies and name exactly which action keys must rebuild.
pub fn overlay_invalidations(
    before_fingerprints: &BTreeMap<(String, String), String>,
    after_fingerprints: &BTreeMap<(String, String), String>,
    actions_by_package: &BTreeMap<String, Vec<String>>,
) -> Vec<OverlayInvalidation> {
    let mut keys: BTreeSet<(String, String)> = BTreeSet::new();
    keys.extend(before_fingerprints.keys().cloned());
    keys.extend(after_fingerprints.keys().cloned());
    let mut out = Vec::new();
    for (overlay, package) in keys {
        let before = before_fingerprints
            .get(&(overlay.clone(), package.clone()))
            .cloned()
            .unwrap_or_default();
        let after = after_fingerprints
            .get(&(overlay.clone(), package.clone()))
            .cloned()
            .unwrap_or_default();
        if before == after {
            continue;
        }
        let affected = actions_by_package
            .get(&package)
            .cloned()
            .unwrap_or_else(|| vec![format!("action:{package}")]);
        let reason = if before.is_empty() {
            format!("overlay `{overlay}` added package `{package}`")
        } else if after.is_empty() {
            format!("overlay `{overlay}` removed package `{package}`")
        } else {
            format!(
                "overlay `{overlay}` changed package `{package}` policy ({before} → {after})"
            )
        };
        out.push(OverlayInvalidation {
            overlay,
            package,
            policy_fingerprint_before: before,
            policy_fingerprint_after: after,
            affected_action_keys: affected,
            reason,
        });
    }
    out
}

/// Apply overlay invalidations: drop stale overlay records; clear hashes of
/// affected packages so offline realize fails until rebuild.
pub fn apply_overlay_invalidations(
    lock: &mut SemanticLockFile,
    invalidations: &[OverlayInvalidation],
) {
    for inv in invalidations {
        let overlay_key = format!("{}:{}", inv.overlay, inv.package);
        lock.records.retain(|r| {
            !(r.identity.kind == LockRecordKind::PackageOverlay
                && r.identity.key == overlay_key)
        });
        for rec in &mut lock.records {
            if rec.identity.key == inv.package
                || inv.affected_action_keys.iter().any(|a| {
                    a == &rec.identity.key || a.ends_with(&format!(":{}", rec.identity.key))
                })
            {
                rec.identity.hash.clear();
            }
        }
    }
}

/// Project lock path (`.jet/lock`).
pub fn live_path(project: &Path) -> PathBuf {
    crate::Store::lock_path(project)
}

/// Load semantic sections from the live unified lock (ignores machine-only
/// `[[package]]` / `[[toolchain]]` blocks — Lock::parse still owns those).
pub fn load(project: &Path) -> Option<SemanticLockFile> {
    let raw = std::fs::read_to_string(live_path(project)).ok()?;
    let lock = parse(&raw);
    if lock.records.is_empty() && lock.inputs.is_empty() && lock.source_maps.is_empty() {
        // Distinguish "no semantic content" from "file missing".
        if raw.contains("[[semantic_record]]")
            || raw.contains("[[lock_input]]")
            || raw.contains("[[source_map]]")
            || raw.contains("semantic-lock-version")
        {
            return Some(lock);
        }
        return Some(SemanticLockFile::default());
    }
    Some(lock)
}

/// Strip semantic sections so machine Lock::parse/write keep their bytes.
pub fn strip_semantic_sections(raw: &str) -> String {
    let mut out = String::new();
    let mut skipping = false;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            skipping = matches!(
                trimmed,
                "[[semantic_record]]"
                    | "[[semantic_record.rationale]]"
                    | "[[lock_input]]"
                    | "[[source_map]]"
            );
            if skipping {
                continue;
            }
        }
        if skipping {
            continue;
        }
        if trimmed.starts_with("semantic-lock-version") {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Merge three-way, revalidate, then atomically write into `.jet/lock`.
pub fn merge_revalidate_commit(
    project: &Path,
    base: &SemanticLockFile,
    left: &SemanticLockFile,
    right: &SemanticLockFile,
) -> Result<SemanticLockFile, LockCommitError> {
    let outcome = merge(base, left, right);
    if !outcome.conflicts.is_empty() {
        return Err(LockCommitError {
            issues: outcome
                .conflicts
                .iter()
                .map(|c| ValidationIssue::DomainConflict {
                    key: c.semantic_key.clone(),
                    left: c.left_identity.clone(),
                    right: c.right_identity.clone(),
                })
                .collect(),
            io: None,
        });
    }
    atomic_commit(project, &outcome.merged)?;
    Ok(outcome.merged)
}

/// Revalidate then atomically splice semantic sections into `.jet/lock`.
pub fn atomic_commit(project: &Path, lock: &SemanticLockFile) -> Result<(), LockCommitError> {
    if let Err(issues) = revalidate(lock) {
        return Err(LockCommitError { issues, io: None });
    }
    let path = live_path(project);
    let project = project.to_path_buf();
    let lock = lock.clone();
    crate::RuntimePolicy::with_project_lock(&project, "semantic-lock", move || {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        let machine = strip_semantic_sections(&existing);
        // Machine half must remain Lock-parseable when present.
        if !machine.trim().is_empty() {
            crate::Lock::parse(&machine).map_err(std::io::Error::other)?;
        }
        let semantic = write(&lock);
        let body = if machine.trim().is_empty() {
            format!("version = 1\n\n{semantic}")
        } else {
            format!("{}\n{semantic}", machine.trim_end())
        };
        let tmp = path
            .parent()
            .map(|p| p.join("lock.tmp"))
            .unwrap_or_else(|| PathBuf::from("lock.tmp"));
        std::fs::write(&tmp, &body)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    })
    .map_err(|e| LockCommitError {
        issues: Vec::new(),
        io: Some(e.to_string()),
    })
}
