//! D-DEVR-SEMID1=A: persisted semantic-operation receipts.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use jet_foundation::DataTree::DataTree;
use jet_foundation::JSON::parse_json;

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
    /// Match an operation to the paths in one comparison as well as its byte
    /// checkpoints. Hashes alone are not enough: two files can share them.
    pub fn matches_file_transition(
        &self,
        paths: &[&Path],
        before_hash: &str,
        after_hash: &str,
    ) -> bool {
        self.files.iter().any(|file| {
            paths
                .iter()
                .any(|path| normalize(&file.path) == normalize(path))
                && same_hash(&file.before_hash, before_hash)
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
                let Ok(DataTree::Object(object)) = parse_json(&raw) else {
                    continue;
                };
                let Some(files) = field(&object, "files").and_then(parse_files) else {
                    continue;
                };
                let files = files
                    .into_iter()
                    .map(|mut file| {
                        file.path = normalize_from(&file.path, &dir);
                        file
                    })
                    .collect::<Vec<_>>();
                if !files.iter().any(|file| {
                    file.path == path
                        && (same_hash(&file.before_hash, source_hash)
                            || same_hash(&file.after_hash, source_hash))
                }) {
                    continue;
                }
                let Some(ops) = field(&object, "semantic_ops").and_then(ops) else {
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

/// Pair compiler facts for a refactor producer. This is deliberately not part
/// of review: a reviewer must not infer a rename from hand-edited text. A
/// tool that already owns the edit may use the unchanged checked signature and
/// module to attach the operation it performed to the edit receipt.
pub fn semantic_rename_ops(before: &SemIndex, after: &SemIndex) -> Vec<SemanticOp> {
    let before_defs = before.definition_facts();
    let after_defs = after.definition_facts();
    let mut used_after = BTreeSet::new();
    let mut out = Vec::new();
    for (before_index, old) in before_defs.iter().enumerate() {
        let candidates = after_defs
            .iter()
            .enumerate()
            .filter(|(after_index, new)| {
                !used_after.contains(after_index)
                    && old.kind == new.kind
                    && old.module_path == new.module_path
                    && old.signature_id == new.signature_id
                    && old.name != new.name
            })
            .map(|(after_index, _)| after_index)
            .collect::<Vec<_>>();
        let Some(&after_index) = candidates.first() else {
            continue;
        };
        if candidates.len() != 1 {
            continue;
        }
        let new = &after_defs[after_index];
        let reverse = before_defs
            .iter()
            .enumerate()
            .filter(|(candidate_index, candidate)| {
                *candidate_index == before_index
                    || (candidate.kind == new.kind
                        && candidate.module_path == new.module_path
                        && candidate.signature_id == new.signature_id
                        && candidate.name != new.name)
            })
            .count();
        if reverse != 1 {
            continue;
        }
        used_after.insert(after_index);
        out.push(SemanticOp {
            kind: "rename".to_string(),
            rule_id: Some("jet-fix".to_string()),
            from: Some(old.name.clone()),
            to: Some(new.name.clone()),
            node: None,
            match_template: None,
            replace_template: None,
            targets: vec![SemanticOpTarget {
                stable_id: old.stable_id.clone(),
                before: old.human_identity.clone(),
                after: new.human_identity.clone(),
                kind: new.kind.clone(),
                module_path: new.module_path.clone(),
            }],
            files: Vec::new(),
        });
    }
    out
}

/// The semantic ownership rows used by a blame consumer. The operation is
/// copied from a receipt; this function never compares source text or guesses
/// intent from matching bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticBlameEntry {
    pub stable_id: String,
    pub identity: String,
    pub kind: String,
    pub operation: Option<SemanticOp>,
}

pub fn semantic_blame(index: &SemIndex, receipts: &[SemanticOp]) -> Vec<SemanticBlameEntry> {
    index
        .definition_facts()
        .iter()
        .map(|fact| SemanticBlameEntry {
            stable_id: fact.stable_id.clone(),
            identity: fact.human_identity.clone(),
            kind: fact.kind.clone(),
            operation: receipts
                .iter()
                .find(|operation| operation_applies_to_fact(operation, fact))
                .cloned(),
        })
        .collect()
}

/// Resolve the receipts for one checked file and project them into semantic
/// ownership rows. This is the complete read-only seam a blame UI needs.
pub fn semantic_blame_for_file(
    path: &Path,
    source_hash: &str,
    index: &SemIndex,
) -> Vec<SemanticBlameEntry> {
    semantic_blame(index, &semantic_ops_for_file(path, source_hash))
}

fn operation_applies_to_fact(operation: &SemanticOp, fact: &DefinitionFact) -> bool {
    operation.targets.iter().any(|target| {
        target.stable_id == fact.stable_id
            || target.after == fact.human_identity
            || target.after == fact.name
            || target
                .after
                .ends_with(&format!("::{name}", name = fact.name))
    }) || (operation.kind == "rename"
        && operation.to.as_deref().is_some_and(|to| {
            to == fact.name
                || to == fact.human_identity
                || fact.human_identity.ends_with(&format!("::{to}"))
        }))
}

#[cfg(test)]
mod semantic_op_tests {
    use super::*;
    use std::fs;

    #[test]
    fn semantic_op_producer_and_blame_consumer_use_checked_facts() {
        let root =
            std::env::temp_dir().join(format!("jet-semantic-op-unit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("run.jet");
        fs::write(&path, "fn report() Int -[]> { return 1 }\nfn run() {}\n").unwrap();
        let before = crate::open(&path).unwrap();
        fs::write(&path, "fn summarize() Int -[]> { return 1 }\nfn run() {}\n").unwrap();
        let after = crate::open(&path).unwrap();

        let operations = semantic_rename_ops(&before, &after);
        assert_eq!(operations.len(), 1);
        assert_eq!(operations[0].from.as_deref(), Some("report"));
        assert_eq!(operations[0].to.as_deref(), Some("summarize"));
        let rows = semantic_blame(&after, &operations);
        assert!(rows.iter().any(|row| {
            row.identity.ends_with("summarize")
                && row
                    .operation
                    .as_ref()
                    .is_some_and(|operation| operation.kind == "rename")
        }));
        let receipt_dir = root.join(".jet/codemods");
        fs::create_dir_all(&receipt_dir).unwrap();
        let before_hash = jet_foundation::SHA256::sha256_hex(
            "fn report() Int -[]> { return 1 }\nfn run() {}\n".as_bytes(),
        );
        let after_hash = jet_foundation::SHA256::sha256_hex(
            "fn summarize() Int -[]> { return 1 }\nfn run() {}\n".as_bytes(),
        );
        fs::write(
            receipt_dir.join("rename.log.json"),
            format!(
                r#"{{"semantic_ops":[{{"kind":"rename","from":"report","to":"summarize"}}],"files":[{{"path":"run.jet","before_hash":"{before_hash}","after_hash":"{after_hash}"}}]}}"#
            ),
        )
        .unwrap();
        assert!(semantic_blame_for_file(&path, &after_hash, &after)
            .iter()
            .any(|row| row.operation.is_some()));
        assert!(semantic_blame(&after, &[])
            .iter()
            .all(|row| row.operation.is_none()));
        let _ = fs::remove_dir_all(root);
    }
}

fn same_hash(recorded: &str, current: &str) -> bool {
    recorded == current
        || recorded.strip_prefix("sha256-") == Some(current)
        || current.strip_prefix("sha256-") == Some(recorded)
}

fn parse_files(value: &DataTree) -> Option<Vec<SemanticOpFile>> {
    let DataTree::Array(values) = value else {
        return None;
    };
    Some(values.iter().filter_map(file).collect())
}

fn file(value: &DataTree) -> Option<SemanticOpFile> {
    let DataTree::Object(object) = value else {
        return None;
    };
    Some(SemanticOpFile {
        path: PathBuf::from(string(object, "path")?),
        before_hash: string(object, "before_hash")?,
        after_hash: string(object, "after_hash")?,
    })
}

fn ops(value: &DataTree) -> Option<Vec<SemanticOp>> {
    let DataTree::Array(values) = value else {
        return None;
    };
    Some(values.iter().filter_map(op).collect())
}

fn op(value: &DataTree) -> Option<SemanticOp> {
    let DataTree::Object(object) = value else {
        return None;
    };
    let targets = field(object, "targets")
        .and_then(|value| match value {
            DataTree::Array(values) => Some(values.iter().filter_map(target).collect()),
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

fn target(value: &DataTree) -> Option<SemanticOpTarget> {
    let DataTree::Object(object) = value else {
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

fn field<'a>(object: &'a [(String, DataTree)], key: &str) -> Option<&'a DataTree> {
    object
        .iter()
        .find_map(|(name, value)| (name == key).then_some(value))
}

fn string(object: &[(String, DataTree)], key: &str) -> Option<String> {
    field(object, key).and_then(|value| match value {
        DataTree::Text(value) | DataTree::TypedText(value) => Some(value.clone()),
        _ => None,
    })
}

fn optional_string(object: &[(String, DataTree)], key: &str) -> Option<String> {
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

fn normalize_from(path: &Path, root: &Path) -> PathBuf {
    if path.is_absolute() {
        normalize(path)
    } else {
        normalize(&root.join(path))
    }
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

/// How a checked subject was aligned between the two review sides.
///
/// Alignment is deliberately separate from the operation kind.  An added or
/// removed subject can be unpaired, while an ambiguous candidate set must not
/// be presented as a semantic match.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReviewAlignment {
    Matched,
    Recorded,
    UnmatchedBefore,
    UnmatchedAfter,
    Ambiguous,
}

impl ReviewAlignment {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Matched => "matched",
            Self::Recorded => "recorded",
            Self::UnmatchedBefore => "unmatched_before",
            Self::UnmatchedAfter => "unmatched_after",
            Self::Ambiguous => "ambiguous",
        }
    }
}

/// One compiler-fact operation consumed by the review verdict.
///
/// `before_identity` and `after_identity` are the exact compiler-owned
/// subject spellings for the two sides.  `before` and `after` remain the
/// operation payload (module, signature, or content ids) for compatibility
/// with the original review projection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewSemanticOp {
    pub kind: ReviewOpKind,
    pub stable_id: String,
    pub identity: String,
    pub before: Option<String>,
    pub after: Option<String>,
    pub before_identity: Option<String>,
    pub after_identity: Option<String>,
    pub alignment: ReviewAlignment,
    /// Rule/receipt identity when a producer recorded the operation.
    pub source_operation: Option<String>,
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

    let mut operations = Vec::new();
    for (before_index, after_index) in pairs {
        let old = &before_defs[before_index];
        let new = &after_defs[after_index];
        if old.module_path != new.module_path {
            operations.push(matched_operation(
                ReviewOpKind::Moved,
                old,
                new,
                compiler_identity(before, old),
                compiler_identity(after, new),
                Some(old.module_path.clone()),
                Some(new.module_path.clone()),
            ));
        }
        if old.signature_id != new.signature_id {
            operations.push(matched_operation(
                ReviewOpKind::SignatureChanged,
                old,
                new,
                compiler_identity(before, old),
                compiler_identity(after, new),
                Some(old.signature_id.clone()),
                Some(new.signature_id.clone()),
            ));
        } else if old.content_id != new.content_id {
            operations.push(matched_operation(
                ReviewOpKind::BodyChanged,
                old,
                new,
                compiler_identity(before, old),
                compiler_identity(after, new),
                Some(old.content_id.clone()),
                Some(new.content_id.clone()),
            ));
        }
    }

    for (index, fact) in before_defs.iter().enumerate() {
        if !matched_before.contains(&index) {
            let alignment = unmatched_alignment(before_defs, after_defs, index, true);
            let identity = compiler_identity(before, fact);
            operations.push(ReviewSemanticOp {
                kind: ReviewOpKind::Removed,
                stable_id: fact.stable_id.clone(),
                identity: identity.clone(),
                before: Some(identity.clone()),
                after: None,
                before_identity: Some(identity),
                after_identity: None,
                alignment,
                source_operation: None,
            });
        }
    }
    for (index, fact) in after_defs.iter().enumerate() {
        if !matched_after.contains(&index) {
            let alignment = unmatched_alignment(before_defs, after_defs, index, false);
            let identity = compiler_identity(after, fact);
            operations.push(ReviewSemanticOp {
                kind: ReviewOpKind::Added,
                stable_id: fact.stable_id.clone(),
                identity: identity.clone(),
                before: None,
                after: Some(identity.clone()),
                before_identity: None,
                after_identity: Some(identity),
                alignment,
                source_operation: None,
            });
        }
    }

    append_effect_operations(&mut operations, before.effects(), after.effects());
    operations.sort_by(|left, right| {
        (
            left.stable_id.as_str(),
            left.identity.as_str(),
            &left.kind,
            left.before.as_deref(),
            left.after.as_deref(),
        )
            .cmp(&(
                right.stable_id.as_str(),
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
        let target = receipt.targets.first();
        let before_name = target
            .map(|target| target.before.clone())
            .or_else(|| receipt.from.clone());
        let after_name = target
            .map(|target| target.after.clone())
            .or_else(|| receipt.to.clone());
        let before_candidates =
            rename_candidates(before.definition_facts(), target, before_name.as_deref());
        let after_candidates =
            rename_candidates(after.definition_facts(), target, after_name.as_deref());
        let old = (before_candidates.len() == 1).then(|| {
            &before.definition_facts()[before_candidates[0]]
        });
        let new = (after_candidates.len() == 1).then(|| {
            &after.definition_facts()[after_candidates[0]]
        });
        let valid_pair = old.zip(new).filter(|(old, new)| {
            old.kind == new.kind
                && old.module_path == new.module_path
                && old.signature_id == new.signature_id
                && old.name != new.name
        });
        let alignment = if let Some((old, new)) = valid_pair {
            if target
                .is_some_and(|target| !target.stable_id.is_empty())
                && target.is_some_and(|target| {
                    target.stable_id != old.stable_id && target.stable_id != new.stable_id
                })
            {
                ReviewAlignment::UnmatchedBefore
            } else {
                let old_id = old.stable_id.clone();
                let new_id = new.stable_id.clone();
                operations.retain(|operation| {
                    !matches!(
                        operation.kind,
                        ReviewOpKind::Removed | ReviewOpKind::Added
                    ) || (operation.stable_id != old_id && operation.stable_id != new_id)
                });
                ReviewAlignment::Recorded
            }
        } else if before_candidates.len() > 1 || after_candidates.len() > 1 {
            ReviewAlignment::Ambiguous
        } else if before_candidates.is_empty() {
            ReviewAlignment::UnmatchedBefore
        } else {
            ReviewAlignment::UnmatchedAfter
        };
        let stable_id = new
            .map(|fact| fact.stable_id.clone())
            .or_else(|| old.map(|fact| fact.stable_id.clone()))
            .or_else(|| target.map(|target| target.stable_id.clone()))
            .unwrap_or_else(|| "rename:unresolved".to_string());
        let before_identity = old
            .map(|fact| fact.human_identity.clone())
            .or_else(|| before_name.clone());
        let after_identity = new
            .map(|fact| fact.human_identity.clone())
            .or_else(|| after_name.clone());
        let identity = after_identity
            .clone()
            .or_else(|| before_identity.clone())
            .unwrap_or_else(|| "unknown".to_string());
        let operation = ReviewSemanticOp {
            kind: ReviewOpKind::Renamed,
            stable_id,
            identity,
            before: before_identity.clone(),
            after: after_identity.clone(),
            before_identity,
            after_identity,
            alignment,
            source_operation: Some(
                receipt
                    .rule_id
                    .clone()
                    .unwrap_or_else(|| receipt.kind.clone()),
            ),
        };
        if !operations.contains(&operation) {
            operations.push(operation);
        }
    }
    sort_review_operations(&mut operations);
    operations
}

fn compiler_identity(index: &SemIndex, fact: &DefinitionFact) -> String {
    index
        .derivation(&fact.stable_id)
        .map(|derivation| derivation.claim.clone())
        .filter(|claim| !claim.is_empty())
        .unwrap_or_else(|| fact.human_identity.clone())
}

fn matched_operation(
    kind: ReviewOpKind,
    _before: &DefinitionFact,
    after: &DefinitionFact,
    before_identity: String,
    after_identity: String,
    old: Option<String>,
    new: Option<String>,
) -> ReviewSemanticOp {
    ReviewSemanticOp {
        kind,
        stable_id: after.stable_id.clone(),
        identity: after_identity.clone(),
        before: old,
        after: new,
        before_identity: Some(before_identity),
        after_identity: Some(after_identity),
        alignment: ReviewAlignment::Matched,
        source_operation: None,
    }
}

fn rename_candidates(
    facts: &[DefinitionFact],
    target: Option<&SemanticOpTarget>,
    requested: Option<&str>,
) -> Vec<usize> {
    facts
        .iter()
        .enumerate()
        .filter(|(_, fact)| {
            target.map_or(true, |target| {
                (target.stable_id.is_empty() || target.stable_id == fact.stable_id)
                    && (target.kind.is_empty() || target.kind == fact.kind)
                    && (target.module_path.is_empty() || target.module_path == fact.module_path)
            })
        })
        .filter(|(_, fact)| {
            requested.map_or(true, |requested| {
                requested == fact.human_identity
                    || requested == fact.name
                    || fact.human_identity.ends_with(&format!("::{requested}"))
            })
        })
        .map(|(index, _)| index)
        .collect()
}

fn sort_review_operations(operations: &mut [ReviewSemanticOp]) {
    operations.sort_by(|left, right| {
        (
            left.stable_id.as_str(),
            left.identity.as_str(),
            &left.kind,
            left.before.as_deref(),
            left.after.as_deref(),
            &left.alignment,
        )
            .cmp(&(
                right.stable_id.as_str(),
                right.identity.as_str(),
                &right.kind,
                right.before.as_deref(),
                right.after.as_deref(),
                &right.alignment,
            ))
    });
}

fn fact_keys(fact: &DefinitionFact) -> [String; 3] {
    [
        format!("{}:{}", fact.kind, fact.stable_id),
        format!("{}:{}", fact.kind, fact.human_identity),
        format!("{}:{}:{}", fact.kind, fact.signature_id, fact.module_path),
    ]
}

fn unmatched_alignment(
    before: &[DefinitionFact],
    after: &[DefinitionFact],
    index: usize,
    before_side: bool,
) -> ReviewAlignment {
    let fact = if before_side {
        &before[index]
    } else {
        &after[index]
    };
    let keys = fact_keys(fact);
    let ambiguous = keys.iter().any(|key| {
        let before_count = before
            .iter()
            .filter(|candidate| fact_keys(candidate).iter().any(|candidate_key| candidate_key == key))
            .count();
        let after_count = after
            .iter()
            .filter(|candidate| fact_keys(candidate).iter().any(|candidate_key| candidate_key == key))
            .count();
        before_count > 0
            && after_count > 0
            && (before_count != 1 || after_count != 1)
    });
    if ambiguous {
        ReviewAlignment::Ambiguous
    } else if before_side {
        ReviewAlignment::UnmatchedBefore
    } else {
        ReviewAlignment::UnmatchedAfter
    }
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
                stable_id: format!("effect:{function}"),
                identity: function.to_string(),
                before: Some(old_shape),
                after: Some(new_shape),
                before_identity: Some(function.to_string()),
                after_identity: Some(function.to_string()),
                alignment: ReviewAlignment::Matched,
                source_operation: None,
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
