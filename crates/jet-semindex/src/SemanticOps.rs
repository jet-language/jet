//! D-DEVR-SEMID1=A: persisted semantic-operation receipts.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use jet_foundation::JSON::{parse_json, JSONValue};

use crate::Types::{DefinitionFact, EffectFact, SemIndex};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticOpTarget {
    pub stable_id: String,
    pub before: String,
    pub after: String,
    pub kind: String,
    pub module_path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticOpFile {
    pub path: PathBuf,
    pub before_hash: String,
    pub after_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticOp {
    pub kind: String,
    pub rule_id: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub node: Option<String>,
    pub match_template: Option<String>,
    pub replace_template: Option<String>,
    pub targets: Vec<SemanticOpTarget>,
    pub files: Vec<SemanticOpFile>,
}

impl SemanticOp {
    pub fn matches_transition(&self, before_hash: &str, after_hash: &str) -> bool {
        self.files.iter().any(|file| {
            same_hash(&file.before_hash, before_hash)
                && same_hash(&file.after_hash, after_hash)
        })
    }
}

/// Read only receipts whose file row names `path` and whose checkpoint is the
/// current source. A hand edit after a refactor therefore cannot inherit the
/// old operation merely because its text resembles the refactor.
pub fn semantic_ops_for_file(path: &Path, source_hash: &str) -> Vec<SemanticOp> {
    let path = normalize(path);
    let mut directory = path.parent().map(Path::to_path_buf);
    let mut out = Vec::new();
    while let Some(dir) = directory {
        let receipts = dir.join(".jet/codemods");
        if let Ok(entries) = fs::read_dir(&receipts) {
            let mut paths = entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|candidate| {
                    candidate.is_file()
                        && candidate
                            .file_name()
                            .and_then(|name| name.to_str())
                            .is_some_and(|name| name.ends_with(".log.json"))
                })
                .collect::<Vec<_>>();
            paths.sort();
            for receipt in paths {
                let Ok(raw) = fs::read_to_string(&receipt) else {
                    continue;
                };
                let Ok(JSONValue::Object(object)) = parse_json(&raw) else {
                    continue;
                };
                let Some(files) = object.get("files").and_then(parse_files) else {
                    continue;
                };
                if !files.iter().any(|file| {
                    normalize(&file.path) == path
                        && (same_hash(&file.before_hash, source_hash)
                            || same_hash(&file.after_hash, source_hash))
                }) {
                    continue;
                }
                let Some(ops) = object.get("semantic_ops").and_then(ops) else {
                    continue;
                };
                out.extend(ops.into_iter().map(|mut op| {
                    op.files = files.clone();
                    op
                }));
            }
        }
        directory = dir.parent().map(Path::to_path_buf);
    }
    out
}

fn same_hash(recorded: &str, current: &str) -> bool {
    recorded == current
        || recorded.strip_prefix("sha256-") == Some(current)
        || current.strip_prefix("sha256-") == Some(recorded)
}

fn parse_files(value: &JSONValue) -> Option<Vec<SemanticOpFile>> {
    let JSONValue::Array(values) = value else {
        return None;
    };
    Some(values.iter().filter_map(file).collect())
}

fn file(value: &JSONValue) -> Option<SemanticOpFile> {
    let JSONValue::Object(object) = value else {
        return None;
    };
    Some(SemanticOpFile {
        path: PathBuf::from(string(object, "path")?),
        before_hash: string(object, "before_hash")?,
        after_hash: string(object, "after_hash")?,
    })
}

fn ops(value: &JSONValue) -> Option<Vec<SemanticOp>> {
    let JSONValue::Array(values) = value else {
        return None;
    };
    Some(values.iter().filter_map(op).collect())
}

fn op(value: &JSONValue) -> Option<SemanticOp> {
    let JSONValue::Object(object) = value else {
        return None;
    };
    let targets = object
        .get("targets")
        .and_then(|value| match value {
            JSONValue::Array(values) => Some(values.iter().filter_map(target).collect()),
            _ => None,
        })
        .unwrap_or_default();
    Some(SemanticOp {
        kind: string(object, "kind")?,
        rule_id: optional_string(object, "rule_id"),
        from: optional_string(object, "from"),
        to: optional_string(object, "to"),
        node: optional_string(object, "node"),
        match_template: optional_string(object, "match"),
        replace_template: optional_string(object, "replace"),
        targets,
        files: Vec::new(),
    })
}

fn target(value: &JSONValue) -> Option<SemanticOpTarget> {
    let JSONValue::Object(object) = value else {
        return None;
    };
    Some(SemanticOpTarget {
        stable_id: string(object, "stable_id")?,
        before: string(object, "before")?,
        after: string(object, "after")?,
        kind: string(object, "kind")?,
        module_path: string(object, "module_path")?,
    })
}

fn string(object: &std::collections::BTreeMap<String, JSONValue>, key: &str) -> Option<String> {
    object.get(key).and_then(|value| match value {
        JSONValue::String(value) => Some(value.clone()),
        _ => None,
    })
}

fn optional_string(
    object: &std::collections::BTreeMap<String, JSONValue>,
    key: &str,
) -> Option<String> {
    string(object, key)
}

fn normalize(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(path)
        }
    })
}

/// One meaning change in a checked change set.  This is separate from the
/// persisted codemod [`SemanticOp`] because review also covers ordinary hand
/// edits that have no codemod receipt.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReviewOpKind {
    Added,
    Removed,
    Renamed,
    Moved,
    SignatureChanged,
    BodyChanged,
    EffectChanged,
}

impl ReviewOpKind {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Removed => "removed",
            Self::Renamed => "renamed",
            Self::Moved => "moved",
            Self::SignatureChanged => "signature_changed",
            Self::BodyChanged => "body_changed",
            Self::EffectChanged => "effect_changed",
        }
    }
}

/// A compiler-fact operation consumed by the review verdict.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewSemanticOp {
    pub kind: ReviewOpKind,
    pub identity: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

/// Compare two checked programs using semantic-index facts only.
///
/// No source text is read here.  Unique compiler-owned ancestry, identity, and
/// signature keys pair definitions; content hashes are used only after that
/// pairing to classify a body change.  Same-kind siblings remain unpaired when
/// the fact data is ambiguous.
pub fn review_semantic_ops(before: &SemIndex, after: &SemIndex) -> Vec<ReviewSemanticOp> {
    let before_defs = before.definition_facts();
    let after_defs = after.definition_facts();
    let mut matched_before = BTreeSet::new();
    let mut matched_after = BTreeSet::new();
    let mut pairs = Vec::new();

    pair_by_key(
        before_defs,
        after_defs,
        &mut matched_before,
        &mut matched_after,
        |fact| format!("{}:{}", fact.kind, fact.stable_id),
        &mut pairs,
    );
    pair_by_key(
        before_defs,
        after_defs,
        &mut matched_before,
        &mut matched_after,
        |fact| format!("{}:{}", fact.kind, fact.human_identity),
        &mut pairs,
    );
    pair_by_key(
        before_defs,
        after_defs,
        &mut matched_before,
        &mut matched_after,
        |fact| format!("{}:{}:{}", fact.kind, fact.signature_id, fact.module_path),
        &mut pairs,
    );
    pair_by_key(
        before_defs,
        after_defs,
        &mut matched_before,
        &mut matched_after,
        |fact| format!("{}:{}", fact.kind, fact.signature_id),
        &mut pairs,
    );

    let mut operations = Vec::new();
    for (before_index, after_index) in pairs {
        let old = &before_defs[before_index];
        let new = &after_defs[after_index];
        let identity = new.human_identity.clone();
        if old.name != new.name {
            operations.push(ReviewSemanticOp {
                kind: ReviewOpKind::Renamed,
                identity: identity.clone(),
                before: Some(old.human_identity.clone()),
                after: Some(new.human_identity.clone()),
            });
        }
        if old.module_path != new.module_path {
            operations.push(ReviewSemanticOp {
                kind: ReviewOpKind::Moved,
                identity: identity.clone(),
                before: Some(old.module_path.clone()),
                after: Some(new.module_path.clone()),
            });
        }
        if old.signature_id != new.signature_id {
            operations.push(ReviewSemanticOp {
                kind: ReviewOpKind::SignatureChanged,
                identity: identity.clone(),
                before: Some(old.signature_id.clone()),
                after: Some(new.signature_id.clone()),
            });
        } else if old.content_id != new.content_id {
            operations.push(ReviewSemanticOp {
                kind: ReviewOpKind::BodyChanged,
                identity,
                before: Some(old.content_id.clone()),
                after: Some(new.content_id.clone()),
            });
        }
    }

    for (index, fact) in before_defs.iter().enumerate() {
        if !matched_before.contains(&index) {
            operations.push(ReviewSemanticOp {
                kind: ReviewOpKind::Removed,
                identity: fact.human_identity.clone(),
                before: Some(fact.human_identity.clone()),
                after: None,
            });
        }
    }
    for (index, fact) in after_defs.iter().enumerate() {
        if !matched_after.contains(&index) {
            operations.push(ReviewSemanticOp {
                kind: ReviewOpKind::Added,
                identity: fact.human_identity.clone(),
                before: None,
                after: Some(fact.human_identity.clone()),
            });
        }
    }

    append_effect_operations(&mut operations, before.effects(), after.effects());
    operations.sort_by(|left, right| {
        (
            left.identity.as_str(),
            &left.kind,
            left.before.as_deref(),
            left.after.as_deref(),
        )
            .cmp(&(
                right.identity.as_str(),
                &right.kind,
                right.before.as_deref(),
                right.after.as_deref(),
            ))
    });
    operations
}

/// Render review meaning with recorded refactors as the authority for rename
/// intent. Other checked changes still come from semantic facts, but a rename
/// is never inferred from two same-shaped definitions without its receipt.
pub fn review_semantic_ops_with_receipts(
    before: &SemIndex,
    after: &SemIndex,
    receipts: &[SemanticOp],
) -> Vec<ReviewSemanticOp> {
    let mut operations = review_semantic_ops(before, after)
        .into_iter()
        .filter(|operation| operation.kind != ReviewOpKind::Renamed)
        .collect::<Vec<_>>();
    for receipt in receipts.iter().filter(|receipt| receipt.kind == "rename") {
        let before_name = receipt
            .targets
            .first()
            .map(|target| target.before.clone())
            .or_else(|| receipt.from.clone());
        let after_name = receipt
            .targets
            .first()
            .map(|target| target.after.clone())
            .or_else(|| receipt.to.clone());
        let Some(identity) = after_name.clone() else {
            continue;
        };
        let operation = ReviewSemanticOp {
            kind: ReviewOpKind::Renamed,
            identity,
            before: before_name,
            after: after_name,
        };
        if !operations.contains(&operation) {
            operations.push(operation);
        }
    }
    operations.sort_by(|left, right| {
        (
            left.identity.as_str(),
            &left.kind,
            left.before.as_deref(),
            left.after.as_deref(),
        )
            .cmp(&(
                right.identity.as_str(),
                &right.kind,
                right.before.as_deref(),
                right.after.as_deref(),
            ))
    });
    operations
}

fn pair_by_key<F>(
    before: &[DefinitionFact],
    after: &[DefinitionFact],
    matched_before: &mut BTreeSet<usize>,
    matched_after: &mut BTreeSet<usize>,
    key: F,
    pairs: &mut Vec<(usize, usize)>,
) where
    F: Fn(&DefinitionFact) -> String,
{
    let mut before_by_key: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut after_by_key: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, fact) in before.iter().enumerate() {
        if !matched_before.contains(&index) {
            before_by_key.entry(key(fact)).or_default().push(index);
        }
    }
    for (index, fact) in after.iter().enumerate() {
        if !matched_after.contains(&index) {
            after_by_key.entry(key(fact)).or_default().push(index);
        }
    }
    for (identity, old) in before_by_key {
        let Some(new) = after_by_key.get(&identity) else {
            continue;
        };
        if old.len() == 1 && new.len() == 1 {
            let old_index = old[0];
            let new_index = new[0];
            matched_before.insert(old_index);
            matched_after.insert(new_index);
            pairs.push((old_index, new_index));
        }
    }
}

fn append_effect_operations(
    operations: &mut Vec<ReviewSemanticOp>,
    before: &[EffectFact],
    after: &[EffectFact],
) {
    let before_by_function = before
        .iter()
        .map(|fact| (fact.function.as_str(), fact))
        .collect::<BTreeMap<_, _>>();
    let after_by_function = after
        .iter()
        .map(|fact| (fact.function.as_str(), fact))
        .collect::<BTreeMap<_, _>>();
    for (function, old) in before_by_function {
        let Some(new) = after_by_function.get(function) else {
            continue;
        };
        let old_shape = effect_shape(old);
        let new_shape = effect_shape(new);
        if old_shape != new_shape {
            operations.push(ReviewSemanticOp {
                kind: ReviewOpKind::EffectChanged,
                identity: function.to_string(),
                before: Some(old_shape),
                after: Some(new_shape),
            });
        }
    }
}

fn effect_shape(effect: &EffectFact) -> String {
    let direct = sorted_join(&effect.direct);
    let inferred = sorted_join(&effect.inferred);
    let callees = sorted_join(&effect.callees);
    format!(
        "direct={direct};inferred={inferred};callees={callees};maximal={}",
        effect.maximal
    )
}

fn sorted_join(values: &[String]) -> String {
    let mut values = values.to_vec();
    values.sort();
    values.dedup();
    values.join(",")
}
