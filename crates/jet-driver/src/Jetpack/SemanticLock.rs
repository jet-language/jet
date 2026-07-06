//! Explainable semantic lock records and merge support (D-WD4).
//!
//! This is the E4 lock-facts layer. Existing `.jet/lock` package/toolchain
//! records stay readable; these records describe exact identity plus rationale
//! without making human text part of the machine key.

use std::collections::{BTreeMap, BTreeSet};

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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SemanticLockFile {
    pub records: Vec<SemanticRecord>,
}

pub fn write(lock: &SemanticLockFile) -> String {
    let mut records = lock.records.clone();
    records.sort_by(|a, b| a.identity.semantic_key().cmp(&b.identity.semantic_key()));
    let mut out = String::from("semantic-lock-version = 1\n");
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
    let mut current: Option<PartialRecord> = None;
    let mut current_rationale: Option<LockRationale> = None;

    for raw_line in raw.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match line {
            "[[semantic_record]]" => {
                flush_rationale(&mut current, &mut current_rationale);
                flush_record(&mut records, &mut current);
                current = Some(PartialRecord::default());
                continue;
            }
            "[[semantic_record.rationale]]" => {
                flush_rationale(&mut current, &mut current_rationale);
                current_rationale = Some(LockRationale::default());
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
        } else if let Some(record) = &mut current {
            record.set(key, val);
        }
    }
    flush_rationale(&mut current, &mut current_rationale);
    flush_record(&mut records, &mut current);
    SemanticLockFile { records }
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
        },
        conflicts,
    }
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
