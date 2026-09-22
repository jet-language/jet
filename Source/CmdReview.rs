//! D-DEVR-REVIEW1=A: one verdict over meaning, authority, and proof.
//!
//! The command joins compiler semantic operations, the shared gate ledger, and
//! proof receipts. It never asks a text diff to explain a checked change.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::exit;

use jet::Diagnostics::{json_str as json_string, Diagnostic};
use jet::ExitCodes;
use jet::Sema::GateLedger::GateLedger;
use jet_foundation::Report::{StatusEnvelope, StatusFields, StatusValue};
use jet_foundation::DataTree::DataTree;
use jet_foundation::JSON::{json_get, json_str, parse};
use jet_semindex::{
    review_semantic_ops_with_receipts, semantic_ops_for_file, ReviewSemanticOp, SemIndex,
    SemanticOp,
};

struct SnapshotInput {
    path: String,
    digest: String,
}

struct ProofReceipt {
    path: String,
    digest: String,
}

struct ReviewSide {
    path: std::path::PathBuf,
    entry: String,
    inputs: Vec<SnapshotInput>,
    closure: &'static str,
    index: SemIndex,
    authority: BTreeMap<String, String>,
    source_hash: String,
    semantic_ops: Vec<SemanticOp>,
}

struct DiagnosticSide {
    path: std::path::PathBuf,
    source_hash: String,
    scope_root: Option<std::path::PathBuf>,
    entry_scope: String,
    input_revisions: BTreeMap<String, String>,
    diagnostics: Vec<Diagnostic>,
    valid: bool,
    complete: bool,
}

#[derive(Clone)]
struct DiagnosticOccurrence {
    diagnostic: Diagnostic,
    scope: String,
    identity: Option<String>,
}

struct DiagnosticRelation {
    status: &'static str,
    reason: String,
    base: Option<DiagnosticOccurrence>,
    head: Option<DiagnosticOccurrence>,
}

struct DiagnosticDiff {
    new: Vec<DiagnosticRelation>,
    existing: Vec<DiagnosticRelation>,
    resolved: Vec<DiagnosticRelation>,
    unknown: Vec<DiagnosticRelation>,
}

#[derive(Clone)]
struct AuthorityChange {
    status: &'static str,
    key: String,
    before: Option<String>,
    after: Option<String>,
}

#[derive(Clone)]
struct DerivationClaim {
    id: String,
    subject: String,
    claim: String,
    producer: String,
    method: String,
    rule: String,
    premises: Vec<String>,
    identity: jet_foundation::Facts::DerivationIdentity,
    assumptions: Vec<String>,
    disposition: String,
    observation: Option<jet_foundation::Facts::DerivationObservation>,
}

#[derive(Clone)]
struct Observation {
    contract: String,
    inputs: String,
    environment: String,
    premises: String,
    event: Option<String>,
    result: String,
    source: String,
    identity: Option<jet_foundation::Facts::DerivationIdentity>,
    order: usize,
}

#[derive(Clone)]
struct Claim {
    label: String,
    outcome: String,
    state: String,
    proven: bool,
    derivation: Option<DerivationClaim>,
    observation: Option<Observation>,
    order: usize,
}

struct Receipt {
    recorded: bool,
    claims: BTreeMap<String, Claim>,
    derivations: BTreeMap<String, DerivationClaim>,
    claim_key: Option<String>,
    proof_receipt: Option<ProofReceipt>,
}

struct ReceiptChange {
    status: &'static str,
    key: String,
    before: Option<Claim>,
    after: Option<Claim>,
}

struct ObservationComparison {
    status: &'static str,
    key: String,
    reason: String,
    before: Option<Observation>,
    after: Option<Observation>,
    subject: Option<String>,
    source_operation: Option<String>,
}

struct ReceiptDiff {
    base_recorded: bool,
    head_recorded: bool,
    gained: usize,
    lost: usize,
    changed: usize,
    retained: usize,
    changes: Vec<ReceiptChange>,
    preserved: Vec<ReceiptChange>,
    changed_contracts: Vec<ReceiptChange>,
    first_difference: Option<ObservationComparison>,
    sampled_agreements: Vec<ObservationComparison>,
    unknown: Vec<ObservationComparison>,
    unproved: Vec<ObservationComparison>,
}

pub(crate) fn run_review(args: &[String], json: bool) {
    if args.iter().any(|argument| argument == "--diagnostics") {
        run_diagnostic_review(args, json);
        return;
    }
    let paths = positional(args);
    if paths.len() != 2 {
        crate::cli_error!(
            @fix "E2104",
            "`jet review` needs a base and a reviewed Jet file",
            "run `jet review base.jet head.jet --base-receipt base.jetproof --receipt head.jetproof`"
        );
        exit(ExitCodes::USAGE);
    }
    let base_receipt = match option_value(args, &["--base-receipt"]) {
        Ok(value) => value,
        Err(message) => usage_error(&message),
    };
    let head_receipt = match option_value(args, &["--receipt", "--head-receipt", "--after-receipt"])
    {
        Ok(value) => value,
        Err(message) => usage_error(&message),
    };
    let base_path = Path::new(&paths[0]);
    let head_path = Path::new(&paths[1]);
    let base = match load_side(base_path) {
        Ok(side) => side,
        Err(message) => input_error(base_path, &message),
    };
    let head = match load_side(head_path) {
        Ok(side) => side,
        Err(message) => input_error(head_path, &message),
    };
    let base_proof = match read_receipt(base_receipt.as_deref().map(Path::new)) {
        Ok(receipt) => receipt,
        Err(message) => receipt_error(&message),
    };
    let head_proof = match read_receipt(head_receipt.as_deref().map(Path::new)) {
        Ok(receipt) => receipt,
        Err(message) => receipt_error(&message),
    };

    let recorded = base
        .semantic_ops
        .iter()
        .chain(head.semantic_ops.iter())
        .filter(|operation| {
            operation.matches_file_transition(
                &[base.path.as_path(), head.path.as_path()],
                &base.source_hash,
                &head.source_hash,
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    let meaning = review_semantic_ops_with_receipts(&base.index, &head.index, &recorded);
    let authority = authority_diff(&base.authority, &head.authority);
    let mut receipts = receipt_diff(&base_proof, &head_proof);
    link_source_operations(&mut receipts, &meaning);
    let verdict = verdict(&authority, &receipts);
    if json {
        render_json(
            &base,
            &head,
            &base_proof,
            &head_proof,
            &meaning,
            &authority,
            &receipts,
            verdict,
        );
    } else {
        render_text(&meaning, &authority, &receipts, verdict);
    }
}

fn run_diagnostic_review(args: &[String], json: bool) {
    let paths = positional(args);
    if paths.len() != 2 {
        diagnostic_usage_error(
            "`jet review --diagnostics` needs a base and a reviewed Jet file",
        );
    }
    let base_path = Path::new(&paths[0]);
    let head_path = Path::new(&paths[1]);
    let base = match load_diagnostic_side(base_path) {
        Ok(side) => side,
        Err(message) => input_error(base_path, &message),
    };
    let head = match load_diagnostic_side(head_path) {
        Ok(side) => side,
        Err(message) => input_error(head_path, &message),
    };
    let diff = compare_diagnostics(&base, &head);
    if json {
        render_diagnostic_json(&base, &head, &diff);
    } else {
        render_diagnostic_text(&base, &head, &diff);
    }
}

fn load_diagnostic_side(path: &Path) -> Result<DiagnosticSide, String> {
    let source = fs::read_to_string(path)
        .map_err(|error| format!("could not read `{}`: {error}", path.display()))?;
    let source_hash = jet::SHA256::sha256_hex(source.as_bytes());
    let projection = crate::CmdInspect::check_diagnostics(path)?;
    let scope_root = diagnostic_scope_root(path);
    let entry_scope = diagnostic_scope_key(path, scope_root.as_deref());
    let input_revisions: BTreeMap<String, String> = projection
        .source_revisions
        .into_iter()
        .map(|(path, revision)| (diagnostic_scope_key(&path, scope_root.as_deref()), revision))
        .collect();
    Ok(DiagnosticSide {
        path: path.to_path_buf(),
        source_hash,
        scope_root,
        entry_scope,
        input_revisions,
        diagnostics: projection.diagnostics,
        valid: projection.valid,
        complete: projection.complete,
    })
}

fn diagnostic_scope_root(path: &Path) -> Option<std::path::PathBuf> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let root = jet::Loader::find_package_root_checked(parent).ok().flatten()?;
    Some(fs::canonicalize(&root).unwrap_or(root))
}

fn diagnostic_scope_key(path: &Path, root: Option<&Path>) -> String {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else if let Some(root) = root {
        root.join(path)
    } else {
        path.to_path_buf()
    };
    let path = fs::canonicalize(&path).unwrap_or(path);
    if let Some(root) = root {
        if let Ok(relative) = path.strip_prefix(root) {
            return relative.to_string_lossy().replace('\\', "/");
        }
    }
    path.to_string_lossy().replace('\\', "/")
}

fn diagnostic_occurrences(side: &DiagnosticSide) -> Vec<DiagnosticOccurrence> {
    side.diagnostics
        .iter()
        .map(|diagnostic| {
            let origin_path = diagnostic
                .origin
                .as_ref()
                .filter(|origin| !origin.path.is_empty())
                .map(|origin| Path::new(origin.path.as_str()))
                .unwrap_or(side.path.as_path());
            let scope = diagnostic_scope_key(origin_path, side.scope_root.as_deref());
            let identity = diagnostic.span.map(|span| {
                format!(
                    "{}|{}|{}:{}",
                    diagnostic.code, scope, span.start, span.end
                )
            });
            DiagnosticOccurrence {
                diagnostic: diagnostic.clone(),
                scope,
                identity,
            }
        })
        .collect()
}

fn diagnostic_identity_map(
    occurrences: &[DiagnosticOccurrence],
) -> BTreeMap<String, Vec<usize>> {
    let mut map = BTreeMap::new();
    for (index, occurrence) in occurrences.iter().enumerate() {
        if let Some(identity) = &occurrence.identity {
            map.entry(identity.clone())
                .or_insert_with(Vec::new)
                .push(index);
        }
    }
    map
}

fn compatible_code_occurrence(
    occurrence: &DiagnosticOccurrence,
    other: &[DiagnosticOccurrence],
) -> bool {
    other.iter().any(|candidate| {
        candidate.diagnostic.code == occurrence.diagnostic.code
            && candidate.scope == occurrence.scope
    })
}

fn input_changed_for_occurrence(
    occurrence: &DiagnosticOccurrence,
    side: &DiagnosticSide,
    other: &DiagnosticSide,
) -> bool {
    if occurrence.scope == side.entry_scope {
        return false;
    }
    match (
        side.input_revisions.get(&occurrence.scope),
        other.input_revisions.get(&occurrence.scope),
    ) {
        (Some(before), Some(after)) => before != after,
        _ => true,
    }
}

fn unmatched_diagnostic_relation(
    occurrence: DiagnosticOccurrence,
    side_complete: bool,
    other_complete: bool,
    other: &[DiagnosticOccurrence],
    input_changed: bool,
    status: &'static str,
) -> DiagnosticRelation {
    let reason = if occurrence.identity.is_none() {
        "diagnostic occurrence has no proven source span".to_string()
    } else if !side_complete || !other_complete {
        "one side has incomplete checker coverage".to_string()
    } else if input_changed {
        "diagnostic source snapshot changed in a dependency scope".to_string()
    } else if compatible_code_occurrence(&occurrence, other) {
        "same-code occurrence has a different location in the compatible scope".to_string()
    } else if other
        .iter()
        .any(|candidate| candidate.diagnostic.code == occurrence.diagnostic.code)
    {
        "same-code occurrence has an incompatible scope".to_string()
    } else {
        String::new()
    };
    if reason.is_empty() {
        DiagnosticRelation {
            status,
            reason: if status == "new" {
                "diagnostic is present only on the reviewed side".to_string()
            } else {
                "diagnostic is present only on the baseline side".to_string()
            },
            base: if status == "resolved" {
                Some(occurrence.clone())
            } else {
                None
            },
            head: if status == "new" {
                Some(occurrence)
            } else {
                None
            },
        }
    } else {
        DiagnosticRelation {
            status: "unknown",
            reason,
            base: if status == "resolved" {
                Some(occurrence.clone())
            } else {
                None
            },
            head: if status == "new" {
                Some(occurrence)
            } else {
                None
            },
        }
    }
}

fn compare_diagnostics(base: &DiagnosticSide, head: &DiagnosticSide) -> DiagnosticDiff {
    let base_occurrences = diagnostic_occurrences(base);
    let head_occurrences = diagnostic_occurrences(head);
    let base_map = diagnostic_identity_map(&base_occurrences);
    let head_map = diagnostic_identity_map(&head_occurrences);
    let keys = base_map
        .keys()
        .chain(head_map.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut diff = DiagnosticDiff {
        new: Vec::new(),
        existing: Vec::new(),
        resolved: Vec::new(),
        unknown: Vec::new(),
    };
    let mut base_seen = BTreeSet::new();
    let mut head_seen = BTreeSet::new();
    for key in keys {
        let old = base_map.get(&key).cloned().unwrap_or_default();
        let new = head_map.get(&key).cloned().unwrap_or_default();
        match (old.as_slice(), new.as_slice()) {
            ([base_index], [head_index]) => {
                base_seen.insert(*base_index);
                head_seen.insert(*head_index);
                let base_occurrence = base_occurrences[*base_index].clone();
                let head_occurrence = head_occurrences[*head_index].clone();
                if input_changed_for_occurrence(&base_occurrence, base, head)
                    || input_changed_for_occurrence(&head_occurrence, head, base)
                {
                    diff.unknown.push(DiagnosticRelation {
                        status: "unknown",
                        reason: "diagnostic source snapshot changed in a dependency scope"
                            .to_string(),
                        base: Some(base_occurrence),
                        head: Some(head_occurrence),
                    });
                } else {
                    diff.existing.push(DiagnosticRelation {
                        status: "existing",
                        reason: "diagnostic occurrence identity matches on both sides"
                            .to_string(),
                        base: Some(base_occurrence),
                        head: Some(head_occurrence),
                    });
                }
            }
            _ if !old.is_empty() && !new.is_empty() => {
                for base_index in old {
                    base_seen.insert(base_index);
                    diff.unknown.push(DiagnosticRelation {
                        status: "unknown",
                        reason: "diagnostic occurrence identity is ambiguous".to_string(),
                        base: Some(base_occurrences[base_index].clone()),
                        head: None,
                    });
                }
                for head_index in new {
                    head_seen.insert(head_index);
                    diff.unknown.push(DiagnosticRelation {
                        status: "unknown",
                        reason: "diagnostic occurrence identity is ambiguous".to_string(),
                        base: None,
                        head: Some(head_occurrences[head_index].clone()),
                    });
                }
            }
            (old, []) => {
                for base_index in old {
                    base_seen.insert(*base_index);
                    let relation = unmatched_diagnostic_relation(
                        base_occurrences[*base_index].clone(),
                        base.complete,
                        head.complete,
                        &head_occurrences,
                        input_changed_for_occurrence(
                            &base_occurrences[*base_index],
                            base,
                            head,
                        ),
                        "resolved",
                    );
                    if relation.status == "resolved" {
                        diff.resolved.push(relation);
                    } else {
                        diff.unknown.push(relation);
                    }
                }
            }
            ([], new) => {
                for head_index in new {
                    head_seen.insert(*head_index);
                    let relation = unmatched_diagnostic_relation(
                        head_occurrences[*head_index].clone(),
                        head.complete,
                        base.complete,
                        &base_occurrences,
                        input_changed_for_occurrence(
                            &head_occurrences[*head_index],
                            head,
                            base,
                        ),
                        "new",
                    );
                    if relation.status == "new" {
                        diff.new.push(relation);
                    } else {
                        diff.unknown.push(relation);
                    }
                }
            }
            _ => {}
        }
    }
    for (index, occurrence) in base_occurrences.iter().enumerate() {
        if base_seen.contains(&index) {
            continue;
        }
        diff.unknown.push(DiagnosticRelation {
            status: "unknown",
            reason: "diagnostic occurrence identity is unavailable".to_string(),
            base: Some(occurrence.clone()),
            head: None,
        });
    }
    for (index, occurrence) in head_occurrences.iter().enumerate() {
        if head_seen.contains(&index) {
            continue;
        }
        diff.unknown.push(DiagnosticRelation {
            status: "unknown",
            reason: "diagnostic occurrence identity is unavailable".to_string(),
            base: None,
            head: Some(occurrence.clone()),
        });
    }
    diff
}

fn diagnostic_origin_json(
    origin: Option<&jet::Diagnostics::DiagnosticOrigin>,
    side: &DiagnosticSide,
    has_span: bool,
) -> String {
    match origin {
        Some(origin) => format!(
            "{{\"display\":{},\"path\":{},\"revision\":{},\"inferred\":false}}",
            json_string(&origin.display),
            json_string(&origin.path),
            json_string(&origin.revision),
        ),
        None if has_span => format!(
            "{{\"display\":{},\"path\":{},\"revision\":{},\"inferred\":true}}",
            json_string(&side.path.display().to_string()),
            json_string(&side.path.display().to_string()),
            json_string(&side.source_hash),
        ),
        None => "null".to_string(),
    }
}

fn diagnostic_cause_json(cause: &jet::Diagnostics::DiagnosticCause) -> String {
    let span = cause
        .span
        .map(|span| format!("{{\"start\":{},\"end\":{}}}", span.start, span.end))
        .unwrap_or_else(|| "null".to_string());
    let origin = cause
        .origin
        .as_deref()
        .map(|origin| {
            format!(
                "{{\"display\":{},\"path\":{},\"revision\":{}}}",
                json_string(&origin.display),
                json_string(&origin.path),
                json_string(&origin.revision),
            )
        })
        .unwrap_or_else(|| "null".to_string());
    format!(
        "{{\"code\":{},\"span\":{},\"origin\":{}}}",
        json_string(&cause.code),
        span,
        origin
    )
}

fn diagnostic_json(diagnostic: &Diagnostic, side: &DiagnosticSide) -> String {
    let severity = match diagnostic.severity {
        jet::Diagnostics::Severity::Error => "error",
        jet::Diagnostics::Severity::Lint => "warning",
    };
    let span = diagnostic
        .span
        .map(|span| format!("{{\"start\":{},\"end\":{}}}", span.start, span.end))
        .unwrap_or_else(|| "null".to_string());
    let detail = diagnostic
        .detail
        .as_deref()
        .map(json_string)
        .unwrap_or_else(|| "null".to_string());
    let causes = diagnostic
        .cause
        .iter()
        .map(diagnostic_cause_json)
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"code\":{},\"severity\":{},\"moment\":{},\"what\":{},\"why\":{},\"fix\":{},\"span\":{},\"origin\":{},\"cause\":[{}],\"detail\":{}}}",
        json_string(&diagnostic.code),
        json_string(severity),
        json_string(diagnostic.moment.as_str()),
        json_string(&diagnostic.what),
        json_string(&diagnostic.why),
        json_string(&diagnostic.fix),
        span,
        diagnostic_origin_json(
            diagnostic.origin.as_deref(),
            side,
            diagnostic.span.is_some(),
        ),
        causes,
        detail,
    )
}

fn diagnostic_relation_json(
    relation: &DiagnosticRelation,
    base: &DiagnosticSide,
    head: &DiagnosticSide,
) -> String {
    let identity = relation
        .base
        .as_ref()
        .and_then(|occurrence| occurrence.identity.as_deref())
        .or_else(|| {
            relation
                .head
                .as_ref()
                .and_then(|occurrence| occurrence.identity.as_deref())
        })
        .map(json_string)
        .unwrap_or_else(|| "null".to_string());
    let base_value = relation
        .base
        .as_ref()
        .map(|occurrence| diagnostic_json(&occurrence.diagnostic, base))
        .unwrap_or_else(|| "null".to_string());
    let head_value = relation
        .head
        .as_ref()
        .map(|occurrence| diagnostic_json(&occurrence.diagnostic, head))
        .unwrap_or_else(|| "null".to_string());
    format!(
        "{{\"status\":{},\"reason\":{},\"identity\":{},\"base\":{},\"head\":{}}}",
        json_string(relation.status),
        json_string(&relation.reason),
        identity,
        base_value,
        head_value,
    )
}

fn diagnostic_relation_list(
    relations: &[DiagnosticRelation],
    base: &DiagnosticSide,
    head: &DiagnosticSide,
) -> String {
    relations
        .iter()
        .map(|relation| diagnostic_relation_json(relation, base, head))
        .collect::<Vec<_>>()
        .join(",")
}

fn diagnostic_side_json(side: &DiagnosticSide) -> String {
    let diagnostics = side
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic_json(diagnostic, side))
        .collect::<Vec<_>>()
        .join(",");
    let validity = if side.valid { "valid" } else { "invalid" };
    let completeness = if side.complete {
        "complete"
    } else {
        "incomplete"
    };
    format!(
        "{{\"path\":{},\"revision\":{},\"scope\":\"explicit-file\",\"valid\":{},\"validity\":{},\"complete\":{},\"completeness\":{},\"coverage\":{{\"status\":{},\"basis\":\"checker-bundle\"}},\"diagnostics\":[{}]}}",
        json_string(&side.path.display().to_string()),
        json_string(&side.source_hash),
        side.valid,
        json_string(validity),
        side.complete,
        json_string(completeness),
        json_string(completeness),
        diagnostics,
    )
}

fn render_diagnostic_json(
    base: &DiagnosticSide,
    head: &DiagnosticSide,
    diff: &DiagnosticDiff,
) {
    let new = diagnostic_relation_list(&diff.new, base, head);
    let existing = diagnostic_relation_list(&diff.existing, base, head);
    let resolved = diagnostic_relation_list(&diff.resolved, base, head);
    let unknown = diagnostic_relation_list(&diff.unknown, base, head);
    let issues = diff
        .new
        .iter()
        .chain(diff.existing.iter())
        .chain(diff.resolved.iter())
        .chain(diff.unknown.iter())
        .map(|relation| diagnostic_relation_json(relation, base, head))
        .collect::<Vec<_>>()
        .join(",");
    let mut output = String::from(
        "{\"schema\":\"jet.diagnostic-review/v1\",\"kind\":\"diagnostic-review\",\"mode\":\"diagnostics\",\"status\":\"completed\",\"limitations\":[\"checker projection has no explicit truncation marker; incomplete means no final bundle\"],\"base\":",
    );
    output.push_str(&diagnostic_side_json(base));
    output.push_str(",\"head\":");
    output.push_str(&diagnostic_side_json(head));
    output.push_str(",\"comparison\":{\"new\":[");
    output.push_str(&new);
    output.push_str("],\"existing\":[");
    output.push_str(&existing);
    output.push_str("],\"resolved\":[");
    output.push_str(&resolved);
    output.push_str("],\"unknown\":[");
    output.push_str(&unknown);
    output.push_str("],\"counts\":{\"new\":");
    output.push_str(&diff.new.len().to_string());
    output.push_str(",\"existing\":");
    output.push_str(&diff.existing.len().to_string());
    output.push_str(",\"resolved\":");
    output.push_str(&diff.resolved.len().to_string());
    output.push_str(",\"unknown\":");
    output.push_str(&diff.unknown.len().to_string());
    output.push_str("}},\"issues\":[");
    output.push_str(&issues);
    output.push_str("]}");
    println!("{output}");
}

fn render_diagnostic_text(
    base: &DiagnosticSide,
    head: &DiagnosticSide,
    diff: &DiagnosticDiff,
) {
    println!(
        "diagnostics base valid={} complete={} count={}",
        base.valid,
        base.complete,
        base.diagnostics.len()
    );
    println!(
        "diagnostics head valid={} complete={} count={}",
        head.valid,
        head.complete,
        head.diagnostics.len()
    );
    for (status, relations) in [
        ("new", &diff.new),
        ("existing", &diff.existing),
        ("resolved", &diff.resolved),
        ("unknown", &diff.unknown),
    ] {
        for relation in relations {
            let code = relation
                .head
                .as_ref()
                .or_else(|| relation.base.as_ref())
                .map(|occurrence| occurrence.diagnostic.code.as_str())
                .unwrap_or("unknown");
            println!("diagnostic {status} {code}: {}", relation.reason);
        }
    }
}

fn load_side(path: &Path) -> Result<ReviewSide, String> {
    let projection = crate::CmdInspect::check_projection(path).map_err(|diagnostics| {
        diagnostics
            .iter()
            .map(|diagnostic| format!("{}: {}", diagnostic.code, diagnostic.what))
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    let mut inputs = projection
        .bundle
        .modules
        .iter()
        .map(|module| SnapshotInput {
            path: module.path.display().to_string(),
            digest: jet::SHA256::sha256_hex(module.source.as_bytes()),
        })
        .collect::<Vec<_>>();
    inputs.sort_by(|left, right| left.path.cmp(&right.path));
    inputs.dedup_by(|left, right| left.path == right.path);
    let entry_path = normalized_snapshot_path(path);
    let source_hash = inputs
        .iter()
        .find(|input| normalized_snapshot_path(Path::new(&input.path)) == entry_path)
        .map(|input| input.digest.clone())
        .ok_or_else(|| format!("checked bundle omitted entry `{}`", path.display()))?;
    let semantic_ops = semantic_ops_for_file(path, &source_hash);
    let mut ledger = GateLedger::collect(&projection.bundle, jet::Policy::GateSet::default());
    crate::CmdGates::append_external_writers(&mut ledger, &projection.bundle, &[]);
    if let Some(diagnostic) = ledger.diagnostics().first() {
        return Err(format!(
            "{}: {}",
            diagnostic.diagnostic.code, diagnostic.diagnostic.what
        ));
    }
    ledger.sort();
    let authority = authority_facts(&ledger, &projection.index);
    let closure = match inputs.len() {
        0 => "unknown",
        1 => "entry_only",
        _ => "complete",
    };
    Ok(ReviewSide {
        path: path.to_path_buf(),
        entry: path.display().to_string(),
        inputs,
        closure,
        index: projection.index,
        authority,
        source_hash,
        semantic_ops,
    })
}

fn normalized_snapshot_path(path: &Path) -> std::path::PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join(path)
        }
    })
}

fn authority_facts(ledger: &GateLedger, index: &SemIndex) -> BTreeMap<String, String> {
    let mut grouped: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for entry in ledger.entries() {
        let key = format!(
            "{}:{}:{}:{}",
            entry.kind.name(),
            entry.domain,
            entry.scope,
            entry.subject
        );
        let value = format!(
            "status={};detail={};reason={}",
            entry.status.as_deref().unwrap_or(""),
            entry.detail,
            entry.reason.as_deref().unwrap_or("")
        );
        grouped.entry(key).or_default().insert(value);
    }
    for effect in index.effects() {
        let key = format!("effect:{}", effect.function);
        let value = format!(
            "direct={};inferred={};maximal={}",
            sorted_join(&effect.direct),
            sorted_join(&effect.inferred),
            effect.maximal
        );
        grouped.entry(key).or_default().insert(value);
    }
    grouped
        .into_iter()
        .map(|(key, values)| (key, values.into_iter().collect::<Vec<_>>().join(" | ")))
        .collect()
}

fn authority_diff(
    before: &BTreeMap<String, String>,
    after: &BTreeMap<String, String>,
) -> Vec<AuthorityChange> {
    let keys = before
        .keys()
        .chain(after.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut changes = Vec::new();
    for key in keys {
        let old = before.get(&key);
        let new = after.get(&key);
        if old == new {
            continue;
        }
        let status = match (old, new) {
            (None, Some(_)) => "widened",
            (Some(_), None) => "narrowed",
            (Some(old), Some(new)) => classify_authority_change(old, new),
            (None, None) => continue,
        };
        changes.push(AuthorityChange {
            status,
            key,
            before: old.cloned(),
            after: new.cloned(),
        });
    }
    changes
}

fn authority_tokens(value: &str) -> BTreeSet<&str> {
    value
        .split(|ch: char| ch == ',' || ch.is_whitespace())
        .filter(|token| !token.is_empty())
        .collect()
}

fn classify_authority_change(before: &str, after: &str) -> &'static str {
    let before_tokens = authority_tokens(before);
    let after_tokens = authority_tokens(after);
    if before_tokens < after_tokens {
        "widened"
    } else if after_tokens < before_tokens {
        "narrowed"
    } else {
        "changed"
    }
}

fn read_receipt(path: Option<&Path>) -> Result<Receipt, String> {
    let Some(path) = path else {
        return Ok(Receipt {
            recorded: false,
            claims: BTreeMap::new(),
            derivations: BTreeMap::new(),
            claim_key: None,
            proof_receipt: None,
        });
    };
    let raw_bytes = fs::read(path)
        .map_err(|error| format!("could not read receipt `{}`: {error}", path.display()))?;
    let raw = String::from_utf8(raw_bytes.clone())
        .map_err(|error| format!("could not read receipt `{}`: {error}", path.display()))?;
    let proof_receipt = Some(ProofReceipt {
        path: path.display().to_string(),
        digest: jet::SHA256::sha256_hex(&raw_bytes),
    });
    let root = parse(&raw)
        .map_err(|error| format!("could not parse receipt `{}`: {error}", path.display()))?;
    let report = json_get(&root, "proofReport")
        .or_else(|| json_get(&root, "proof_report"))
        .unwrap_or(&root);
    let receipt_claim_key = json_get(&root, "claim_key")
        .or_else(|| json_get(&root, "claimKey"))
        .or_else(|| json_get(report, "claim_key"))
        .or_else(|| json_get(report, "claimKey"))
        .and_then(json_str)
        .map(str::to_string)
        .filter(|value| !value.is_empty());
    let mut derivations = BTreeMap::new();
    if let Some(values) = json_get(report, "derivations") {
        let values = values.as_array().map_err(|error| {
            format!("receipt `{}` has invalid derivations: {error}", path.display())
        })?;
        for value in values {
            let object = value.as_object().map_err(|error| {
                format!("receipt `{}` has invalid derivation: {error}", path.display())
            })?;
            let derivation = parse_derivation_claim(object)
                .map_err(|error| format!("receipt `{}` has invalid derivation: {error}", path.display()))?;
            if derivations.insert(derivation.id.clone(), derivation).is_some() {
                return Err(format!(
                    "receipt `{}` has duplicate derivation ids",
                    path.display()
                ));
            }
        }
    }
    let Some(evidence) = json_get(report, "evidence").or_else(|| json_get(report, "claims")) else {
        return Ok(Receipt {
            recorded: true,
            claims: BTreeMap::new(),
            derivations,
            claim_key: receipt_claim_key,
            proof_receipt,
        });
    };
    let evidence = evidence
        .as_array()
        .map_err(|error| format!("receipt `{}` has invalid evidence: {error}", path.display()))?;
    let mut claims = BTreeMap::new();
    for (order, item) in evidence.iter().enumerate() {
        let object = item.as_object().map_err(|error| {
            format!(
                "receipt `{}` has invalid evidence item: {error}",
                path.display()
            )
        })?;
        let key = claim_key(object);
        if claims.contains_key(&key) {
            return Err(format!(
                "receipt `{}` has duplicate evidence claim `{key}`",
                path.display()
            ));
        }
        let outcome = string_field(object, "outcome")
            .or_else(|| string_field(object, "result"))
            .unwrap_or_else(|| "unknown".to_string());
        let state = string_field(object, "state")
            .or_else(|| {
                object_field(object, "completeness")
                    .and_then(|value| value.as_object().ok())
                    .and_then(|object| string_field(object, "state"))
            })
            .unwrap_or_else(|| "unknown".to_string());
        let label = string_field(object, "claimId")
            .or_else(|| string_field(object, "claim"))
            .or_else(|| string_field(object, "id"))
            .or_else(|| string_field(object, "property"))
            .unwrap_or_else(|| key.clone());
        let derivation = derivation_for_evidence(object, &derivations)?;
        let proven = derivation.as_ref().is_some_and(is_proven_derivation);
        let observation =
            observation_for_evidence(object, derivation.as_ref(), &outcome, &state, order);
        claims.insert(
            key,
            Claim {
                label,
                proven,
                outcome,
                state,
                derivation,
                observation,
                order,
            },
        );
    }
    Ok(Receipt {
        recorded: true,
        claims,
        derivations,
        claim_key: receipt_claim_key,
        proof_receipt,
    })
}

fn parse_derivation_claim(object: &[(String, DataTree)]) -> Result<DerivationClaim, String> {
    let id = string_field(object, "id").ok_or("derivation has no id")?;
    Ok(DerivationClaim {
        id,
        subject: string_field(object, "subject").unwrap_or_default(),
        claim: string_field(object, "claim").unwrap_or_default(),
        producer: string_field(object, "producer").unwrap_or_default(),
        method: string_field(object, "method").unwrap_or_else(|| "unknown".to_string()),
        rule: string_field(object, "rule").unwrap_or_default(),
        premises: string_array_field(object, "premises")?,
        identity: parse_derivation_identity(object_field(object, "identity"))?,
        assumptions: string_array_field(object, "assumptions")?,
        disposition: string_field(object, "disposition").unwrap_or_else(|| "unknown".to_string()),
        observation: parse_derivation_observation(object_field(object, "observation"))?,
    })
}

fn parse_derivation_identity(
    value: Option<&DataTree>,
) -> Result<jet_foundation::Facts::DerivationIdentity, String> {
    let Some(value) = value else {
        return Ok(jet_foundation::Facts::DerivationIdentity::new("", "", "", ""));
    };
    if matches!(value, DataTree::Null) {
        return Ok(jet_foundation::Facts::DerivationIdentity::new("", "", "", ""));
    }
    let object = value
        .as_object()
        .map_err(|error| format!("derivation identity is invalid: {error}"))?;
    Ok(jet_foundation::Facts::DerivationIdentity::new(
        string_field(object, "source").unwrap_or_default(),
        string_field(object, "build").unwrap_or_default(),
        string_field(object, "run").unwrap_or_default(),
        string_field(object, "target").unwrap_or_default(),
    ))
}

fn parse_derivation_observation(
    value: Option<&DataTree>,
) -> Result<Option<jet_foundation::Facts::DerivationObservation>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    if matches!(value, DataTree::Null) {
        return Ok(None);
    }
    let object = value
        .as_object()
        .map_err(|error| format!("derivation observation is invalid: {error}"))?;
    let event = string_field(object, "event");
    let counterexample = string_field(object, "counterexample");
    Ok(match (event, counterexample) {
        (Some(event), _) => Some(jet_foundation::Facts::DerivationObservation::event(event)),
        (None, Some(counterexample)) => Some(
            jet_foundation::Facts::DerivationObservation::counterexample(counterexample),
        ),
        (None, None) => None,
    })
}

fn string_array_field(
    object: &[(String, DataTree)],
    field: &str,
) -> Result<Vec<String>, String> {
    let Some(value) = object_field(object, field) else {
        return Ok(Vec::new());
    };
    let values = value
        .as_array()
        .map_err(|error| format!("{field} is invalid: {error}"))?;
    values
        .iter()
        .map(|value| {
            json_str(value)
                .map(str::to_string)
                .ok_or_else(|| format!("{field} has a non-text item"))
        })
        .collect()
}

fn derivation_for_evidence(
    object: &[(String, DataTree)],
    table: &BTreeMap<String, DerivationClaim>,
) -> Result<Option<DerivationClaim>, String> {
    let Some(value) = object_field(object, "derivation") else {
        return Ok(None);
    };
    if matches!(value, DataTree::Null) {
        return Ok(None);
    }
    let derivation = value
        .as_object()
        .map_err(|error| format!("evidence has invalid derivation: {error}"))?;
    let Some(id) = string_field(derivation, "id") else {
        return Err("evidence derivation has no id".to_string());
    };
    if let Some(value) = table.get(&id) {
        return Ok(Some(value.clone()));
    }
    let mut inline = parse_derivation_claim(derivation)?;
    inline.id = id;
    Ok(Some(inline))
}

fn observation_for_evidence(
    object: &[(String, DataTree)],
    derivation: Option<&DerivationClaim>,
    outcome: &str,
    state: &str,
    order: usize,
) -> Option<Observation> {
    let derivation = derivation?;
    let contract = object_field(object, "contract")
        .map(canonical_json)
        .or_else(|| string_field(object, "claim").map(|value| canonical_json(&DataTree::Text(value))))
        .unwrap_or_default();
    let inputs = object_field(object, "inputs")
        .or_else(|| object_field(object, "property"))
        .map(canonical_json)
        .unwrap_or_default();
    let environment = object_field(object, "environment")
        .or_else(|| object_field(object, "target"))
        .or_else(|| object_field(object, "build"))
        .map(canonical_json)
        .or_else(|| {
            if derivation.identity.build.is_empty() && derivation.identity.target.is_empty() {
                None
            } else {
                Some(format!(
                    "{{\"build\":{},\"target\":{}}}",
                    canonical_json(&DataTree::Text(derivation.identity.build.clone())),
                    canonical_json(&DataTree::Text(derivation.identity.target.clone()))
                ))
            }
        })
        .unwrap_or_default();
    let mut premise_values = derivation.premises.clone();
    premise_values.extend(derivation.assumptions.clone());
    for field in ["premises", "numerical", "scheduling", "schedule"] {
        if let Some(value) = object_field(object, field) {
            premise_values.push(canonical_json(value));
        }
    }
    premise_values.sort();
    let premises = premise_values.join(",");
    let event = derivation
        .observation
        .as_ref()
        .and_then(|observation| observation.event_id.clone())
        .or_else(|| {
            derivation
                .observation
                .as_ref()
                .and_then(|observation| observation.counterexample_id.clone())
        })
        .or_else(|| string_field(object, "event"))
        .or_else(|| string_field(object, "observation"));
    let source = object_field(object, "source")
        .map(canonical_json)
        .unwrap_or_default();
    Some(Observation {
        contract,
        inputs,
        environment,
        premises,
        event,
        result: format!("{outcome}:{state}"),
        source,
        identity: Some(derivation.identity.clone()),
        order,
    })
}

fn derivation_key(claim: &Claim) -> Option<&str> {
    claim.derivation.as_ref().map(|derivation| derivation.id.as_str())
}

fn is_proven_derivation(derivation: &DerivationClaim) -> bool {
    matches!(
        (
            jet_foundation::Facts::DerivationDisposition::from_str(&derivation.disposition),
            jet_foundation::Facts::DerivationMethod::from_str(&derivation.method),
        ),
        (
            Some(jet_foundation::Facts::DerivationDisposition::Current),
            Some(
                jet_foundation::Facts::DerivationMethod::StaticDerivation
                    | jet_foundation::Facts::DerivationMethod::FormalProof
            )
        )
    )
}
fn claim_key(object: &[(String, DataTree)]) -> String {
    for field in ["claimId", "claim"] {
        if let Some(value) = string_field(object, field) {
            return format!("{field}:{value}");
        }
    }
    let mut parts = Vec::new();
    for field in [
        "kind", "facet", "producer", "contract", "budget", "property", "reason", "solver",
    ] {
        if let Some(value) = object_field(object, field) {
            let rendered = if field == "contract" {
                contract_identity(value)
            } else {
                canonical_json(value)
            };
            parts.push(format!("{field}={rendered}"));
        }
    }
    if let Some(source) = object_field(object, "source") {
        if let Some(path) = json_get(source, "path") {
            parts.push(format!("source.path={}", canonical_json(path)));
        }
    }
    if parts.is_empty() {
        return string_field(object, "id")
            .map(|id| format!("id:{id}"))
            .unwrap_or_else(|| "anonymous".to_string());
    }
    parts.join("|")
}

fn contract_identity(value: &DataTree) -> String {
    let Ok(object) = value.as_object() else {
        return canonical_json(value);
    };
    let marker = string_field(object, "marker").unwrap_or_default();
    let observation = string_field(object, "observation").unwrap_or_default();
    if marker.is_empty() && observation.is_empty() {
        return canonical_json(value);
    }
    format!("marker={marker}|observation={observation}")
}

fn receipt_diff(before: &Receipt, after: &Receipt) -> ReceiptDiff {
    let keys = before
        .claims
        .keys()
        .chain(after.claims.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut diff = ReceiptDiff {
        base_recorded: before.recorded,
        head_recorded: after.recorded,
        gained: 0,
        lost: 0,
        changed: 0,
        retained: 0,
        changes: Vec::new(),
        preserved: Vec::new(),
        changed_contracts: Vec::new(),
        first_difference: None,
        sampled_agreements: Vec::new(),
        unknown: Vec::new(),
        unproved: Vec::new(),
    };
    let mut first_difference_order = usize::MAX;
    for key in keys {
        let old = before.claims.get(&key);
        let new = after.claims.get(&key);
        let status = match (old, new) {
            (Some(old), Some(new)) if old.proven && !new.proven => Some("lost"),
            (Some(old), None) if old.proven => Some("lost"),
            (None, Some(new)) if new.proven => Some("gained"),
            (Some(old), Some(new)) if !old.proven && new.proven => Some("gained"),
            (Some(old), Some(new))
                if old.outcome != new.outcome
                    || old.state != new.state
                    || derivation_key(old) != derivation_key(new) =>
            {
                Some("changed")
            }
            _ => None,
        };
        if let (Some(old), Some(new)) = (old, new) {
            if let Some(comparison) = compare_observations(&key, old, new) {
                match comparison.status {
                    "first_difference" => {
                        let order = comparison
                            .before
                            .as_ref()
                            .map(|observation| observation.order)
                            .unwrap_or(usize::MAX);
                        if order < first_difference_order {
                            first_difference_order = order;
                            diff.first_difference = Some(comparison);
                        }
                    }
                    "sampled_agreement" => diff.sampled_agreements.push(comparison),
                    "unproved" => diff.unproved.push(comparison),
                    _ => diff.unknown.push(comparison),
                }
            }
        }
        let Some(status) = status else {
            diff.retained += 1;
            if let (Some(old), Some(new)) = (old, new) {
                if old.proven
                    && new.proven
                    && derivation_key(old) == derivation_key(new)
                {
                    diff.preserved.push(ReceiptChange {
                        status: "preserved_checked",
                        key: key.clone(),
                        before: Some(old.clone()),
                        after: Some(new.clone()),
                    });
                }
            }
            continue;
        };
        match status {
            "gained" => diff.gained += 1,
            "lost" => diff.lost += 1,
            _ => diff.changed += 1,
        }
        let change = ReceiptChange {
            status,
            key,
            before: old.cloned(),
            after: new.cloned(),
        };
        diff.changed_contracts.push(ReceiptChange {
            status: change.status,
            key: change.key.clone(),
            before: change.before.clone(),
            after: change.after.clone(),
        });
        diff.changes.push(change);
    }
    diff.sampled_agreements.sort_by_key(|comparison| {
        comparison
            .before
            .as_ref()
            .map(|observation| observation.order)
            .unwrap_or(usize::MAX)
    });
    diff.unknown.sort_by_key(|comparison| {
        comparison
            .before
            .as_ref()
            .map(|observation| observation.order)
            .unwrap_or(usize::MAX)
    });
    diff.unproved.sort_by_key(|comparison| {
        comparison
            .before
            .as_ref()
            .map(|observation| observation.order)
            .unwrap_or(usize::MAX)
    });
    diff
}

fn compare_observations(
    key: &str,
    before: &Claim,
    after: &Claim,
) -> Option<ObservationComparison> {
    let before_observation = before.observation.clone();
    let after_observation = after.observation.clone();
    let subject = before
        .derivation
        .as_ref()
        .map(|derivation| derivation.subject.clone())
        .filter(|subject| !subject.is_empty())
        .or_else(|| {
            after
                .derivation
                .as_ref()
                .map(|derivation| derivation.subject.clone())
                .filter(|subject| !subject.is_empty())
        });
    let source_operation = None;
    let Some(before_value) = before_observation.as_ref() else {
        return Some(ObservationComparison {
            status: "unknown",
            key: key.to_string(),
            reason: "base observation is missing".to_string(),
            before: before_observation,
            after: after_observation,
            subject,
            source_operation,
        });
    };
    let Some(after_value) = after_observation.as_ref() else {
        return Some(ObservationComparison {
            status: "unknown",
            key: key.to_string(),
            reason: "reviewed observation is missing".to_string(),
            before: before_observation,
            after: after_observation,
            subject,
            source_operation,
        });
    };
    let mismatches = observation_mismatches(before_value, after_value);
    if !mismatches.is_empty() {
        return Some(ObservationComparison {
            status: "unknown",
            key: key.to_string(),
            reason: mismatches.join("; "),
            before: before_observation,
            after: after_observation,
            subject,
            source_operation,
        });
    }
    if before_value.result.is_empty()
        || after_value.result.is_empty()
        || before_value.result.ends_with(":unknown")
        || after_value.result.ends_with(":unknown")
    {
        return Some(ObservationComparison {
            status: "unknown",
            key: key.to_string(),
            reason: "comparable event result is missing".to_string(),
            before: before_observation,
            after: after_observation,
            subject,
            source_operation,
        });
    }
    if before_value.result != after_value.result || before_value.event != after_value.event {
        return Some(ObservationComparison {
            status: "first_difference",
            key: key.to_string(),
            reason: "first differing comparable event or result".to_string(),
            before: before_observation,
            after: after_observation,
            subject,
            source_operation,
        });
    }
    let sampled = before
        .derivation
        .as_ref()
        .is_some_and(|derivation| derivation.method == "sampled_agreement")
        || after
            .derivation
            .as_ref()
            .is_some_and(|derivation| derivation.method == "sampled_agreement");
    if sampled {
        return Some(ObservationComparison {
            status: "sampled_agreement",
            key: key.to_string(),
            reason: "equal sampled traces do not establish universal equivalence".to_string(),
            before: before_observation,
            after: after_observation,
            subject,
            source_operation,
        });
    }
    if !before.proven || !after.proven {
        let before_disposition = before
            .derivation
            .as_ref()
            .map(|derivation| derivation.disposition.as_str())
            .unwrap_or("missing");
        let after_disposition = after
            .derivation
            .as_ref()
            .map(|derivation| derivation.disposition.as_str())
            .unwrap_or("missing");
        return Some(ObservationComparison {
            status: "unproved",
            key: key.to_string(),
            reason: format!(
                "recorded agreement is not a checked universal contract (base disposition: {before_disposition}; reviewed disposition: {after_disposition})"
            ),
            before: before_observation,
            after: after_observation,
            subject,
            source_operation,
        });
    }
    None
}

fn observation_mismatches(
    before_observation: &Observation,
    after_observation: &Observation,
) -> Vec<String> {
    let mut mismatches = Vec::new();
    if before_observation.contract.is_empty() || after_observation.contract.is_empty() {
        mismatches.push("observation contract is missing".to_string());
    } else if before_observation.contract != after_observation.contract {
        mismatches.push("observation contract differs".to_string());
    }
    if before_observation.inputs.is_empty() || after_observation.inputs.is_empty() {
        mismatches.push("declared inputs are missing".to_string());
    } else if before_observation.inputs != after_observation.inputs {
        mismatches.push("declared inputs differ".to_string());
    }
    if before_observation.environment.is_empty() || after_observation.environment.is_empty() {
        mismatches.push("relevant environment identity is missing".to_string());
    } else if before_observation.environment != after_observation.environment {
        mismatches.push("relevant environment identity differs".to_string());
    }
    if before_observation.premises.is_empty() || after_observation.premises.is_empty() {
        mismatches.push("numerical or scheduling premises are missing".to_string());
    } else if before_observation.premises != after_observation.premises {
        mismatches.push("numerical or scheduling premises differ".to_string());
    }
    for (name, old, new) in [
        (
            "build",
            before_observation
                .identity
                .as_ref()
                .map(|identity| identity.build.as_str()),
            after_observation
                .identity
                .as_ref()
                .map(|identity| identity.build.as_str()),
        ),
        (
            "run",
            before_observation
                .identity
                .as_ref()
                .map(|identity| identity.run.as_str()),
            after_observation
                .identity
                .as_ref()
                .map(|identity| identity.run.as_str()),
        ),
        (
            "target",
            before_observation
                .identity
                .as_ref()
                .map(|identity| identity.target.as_str()),
            after_observation
                .identity
                .as_ref()
                .map(|identity| identity.target.as_str()),
        ),
    ] {
        match (old, new) {
            (Some(old), Some(new)) if old != new => {
                mismatches.push(format!("{name} identity differs"));
            }
            (Some(_), Some(_)) => {}
            _ => mismatches.push(format!("{name} identity is missing")),
        }
    }
    mismatches
}

fn link_source_operations(receipts: &mut ReceiptDiff, meaning: &[ReviewSemanticOp]) {
    let link = |comparison: &mut ObservationComparison| {
        let Some(subject) = comparison.subject.as_deref() else {
            return;
        };
        let Some(operation) = meaning.iter().find(|operation| {
            [
                Some(operation.identity.as_str()),
                operation.before_identity.as_deref(),
                operation.after_identity.as_deref(),
            ]
            .into_iter()
            .flatten()
            .any(|identity| {
                identity == subject
                    || identity.ends_with(&format!("::{subject}"))
                    || comparison.key == identity
                    || comparison.key.ends_with(identity)
            })
        }) else {
            return;
        };
        comparison.source_operation = Some(
            operation
                .source_operation
                .clone()
                .unwrap_or_else(|| operation.kind.name().to_string()),
        );
    };
    if let Some(comparison) = receipts.first_difference.as_mut() {
        link(comparison);
    }
    for comparison in receipts
        .sampled_agreements
        .iter_mut()
        .chain(receipts.unknown.iter_mut())
        .chain(receipts.unproved.iter_mut())
    {
        link(comparison);
    }
}

fn verdict(authority: &[AuthorityChange], receipts: &ReceiptDiff) -> &'static str {
    if authority.iter().any(|change| change.status == "widened") && receipts.lost > 0 {
        "authority widened and proof lost"
    } else if authority.iter().any(|change| change.status == "widened") {
        "authority widened"
    } else if receipts.lost > 0 {
        "proof lost"
    } else {
        "reviewable"
    }
}

fn render_text(
    meaning: &[ReviewSemanticOp],
    authority: &[AuthorityChange],
    receipts: &ReceiptDiff,
    verdict: &str,
) {
    if meaning.is_empty() {
        println!("meaning    no semantic changes");
    } else {
        println!("meaning    {} semantic change(s)", meaning.len());
        for operation in meaning {
            println!(
                "  {}: {} [{}] alignment={} before_identity={} after_identity={} source_operation={}",
                operation.kind.name(),
                operation.identity,
                operation.stable_id,
                operation.alignment.name(),
                operation.before_identity.as_deref().unwrap_or("none"),
                operation.after_identity.as_deref().unwrap_or("none"),
                operation.source_operation.as_deref().unwrap_or("none"),
            );
        }
    }
    if authority.is_empty() {
        println!("authority  no authority changes");
    } else {
        println!("authority  {} authority change(s)", authority.len());
        for change in authority {
            println!("  {}: {}", change.status, change.key);
        }
    }
    println!(
        "claims     +{} gained · {} lost · {} changed · {} retained (base={} head={})",
        receipts.gained,
        receipts.lost,
        receipts.changed,
        receipts.retained,
        if receipts.base_recorded {
            "recorded"
        } else {
            "missing"
        },
        if receipts.head_recorded {
            "recorded"
        } else {
            "missing"
        },
    );
    for change in &receipts.changes {
        println!("  {}: {}", change.status, change.key);
    }
    println!(
        "proof      preserved={} changed_contracts={} first_difference={} sampled_agreements={} unknown={} unproved={}",
        receipts.preserved.len(),
        receipts.changed_contracts.len(),
        receipts.first_difference.is_some(),
        receipts.sampled_agreements.len(),
        receipts.unknown.len(),
        receipts.unproved.len(),
    );
    for comparison in receipts
        .first_difference
        .iter()
        .chain(receipts.sampled_agreements.iter())
        .chain(receipts.unknown.iter())
        .chain(receipts.unproved.iter())
    {
        println!("  {}", observation_comparison_text(comparison));
    }
    for change in receipts
        .preserved
        .iter()
        .chain(receipts.changed_contracts.iter())
    {
        println!("  {}: {}", change.status, change.key);
    }
    println!("verdict    {verdict}");
}

fn observation_text(observation: &Observation) -> String {
    format!(
        "contract={} inputs={} environment={} premises={} event={} result={} source={} order={}",
        observation.contract,
        observation.inputs,
        observation.environment,
        observation.premises,
        observation.event.as_deref().unwrap_or("none"),
        observation.result,
        observation.source,
        observation.order,
    )
}

fn observation_comparison_text(comparison: &ObservationComparison) -> String {
    format!(
        "{}: {} reason={} subject={} source_operation={} before=[{}] after=[{}]",
        comparison.status,
        comparison.key,
        comparison.reason,
        comparison.subject.as_deref().unwrap_or("none"),
        comparison.source_operation.as_deref().unwrap_or("none"),
        comparison
            .before
            .as_ref()
            .map(observation_text)
            .unwrap_or_else(|| "none".to_string()),
        comparison
            .after
            .as_ref()
            .map(observation_text)
            .unwrap_or_else(|| "none".to_string()),
    )
}
fn identity_value(identity: &jet_foundation::Facts::DerivationIdentity) -> StatusValue {
    StatusValue::object(
        StatusFields::new()
            .with("source", identity.source.as_str())
            .with("build", identity.build.as_str())
            .with("run", identity.run.as_str())
            .with("target", identity.target.as_str()),
    )
}

fn derivation_observation_value(
    observation: Option<&jet_foundation::Facts::DerivationObservation>,
) -> StatusValue {
    observation
        .map(|observation| {
            StatusValue::object(
                StatusFields::new()
                    .with(
                        "event",
                        observation
                            .event_id
                            .as_deref()
                            .map(StatusValue::from)
                            .unwrap_or(StatusValue::Null),
                    )
                    .with(
                        "counterexample",
                        observation
                            .counterexample_id
                            .as_deref()
                            .map(StatusValue::from)
                            .unwrap_or(StatusValue::Null),
                    ),
            )
        })
        .unwrap_or(StatusValue::Null)
}

fn derivation_value(derivation: &DerivationClaim) -> StatusValue {
    StatusValue::object(
        StatusFields::new()
            .with("id", derivation.id.as_str())
            .with("subject", derivation.subject.as_str())
            .with("claim", derivation.claim.as_str())
            .with("producer", derivation.producer.as_str())
            .with("method", derivation.method.as_str())
            .with("rule", derivation.rule.as_str())
            .with(
                "premises",
                StatusValue::array(
                    derivation
                        .premises
                        .iter()
                        .map(|premise| StatusValue::from(premise.as_str())),
                ),
            )
            .with("identity", identity_value(&derivation.identity))
            .with(
                "assumptions",
                StatusValue::array(
                    derivation
                        .assumptions
                        .iter()
                        .map(|assumption| StatusValue::from(assumption.as_str())),
                ),
            )
            .with("disposition", derivation.disposition.as_str())
            .with(
                "observation",
                derivation_observation_value(derivation.observation.as_ref()),
            ),
    )
}

fn receipt_observation_value(observation: &Observation) -> StatusValue {
    StatusValue::object(
        StatusFields::new()
            .with("contract", observation.contract.as_str())
            .with("inputs", observation.inputs.as_str())
            .with("environment", observation.environment.as_str())
            .with("premises", observation.premises.as_str())
            .with(
                "event",
                observation
                    .event
                    .as_deref()
                    .map(StatusValue::from)
                    .unwrap_or(StatusValue::Null),
            )
            .with("result", observation.result.as_str())
            .with("source", observation.source.as_str())
            .with(
                "identity",
                observation
                    .identity
                    .as_ref()
                    .map(identity_value)
                    .unwrap_or(StatusValue::Null),
            )
            .with("order", observation.order),
    )
}

fn comparison_value(comparison: &ObservationComparison) -> StatusValue {
    StatusValue::object(
        StatusFields::new()
            .with("status", comparison.status)
            .with("key", comparison.key.as_str())
            .with("reason", comparison.reason.as_str())
            .with(
                "before",
                comparison
                    .before
                    .as_ref()
                    .map(receipt_observation_value)
                    .unwrap_or(StatusValue::Null),
            )
            .with(
                "after",
                comparison
                    .after
                    .as_ref()
                    .map(receipt_observation_value)
                    .unwrap_or(StatusValue::Null),
            )
            .with(
                "subject",
                comparison
                    .subject
                    .as_deref()
                    .map(StatusValue::from)
                    .unwrap_or(StatusValue::Null),
            )
            .with(
                "source_operation",
                comparison
                    .source_operation
                    .as_deref()
                    .map(StatusValue::from)
                    .unwrap_or(StatusValue::Null),
            ),
    )
}

fn receipt_change_value(change: &ReceiptChange) -> StatusValue {
    StatusValue::object(
        StatusFields::new()
            .with("status", change.status)
            .with("key", change.key.as_str())
            .with(
                "before",
                change
                    .before
                    .as_ref()
                    .map(claim_value)
                    .unwrap_or(StatusValue::Null),
            )
            .with(
                "after",
                change
                    .after
                    .as_ref()
                    .map(claim_value)
                    .unwrap_or(StatusValue::Null),
            ),
    )
}


fn snapshot_value(side: &ReviewSide, receipt: &Receipt) -> StatusValue {
    let inputs = StatusValue::array(side.inputs.iter().map(|input| {
        StatusValue::object(
            StatusFields::new()
                .with("path", input.path.as_str())
                .with("digest", input.digest.as_str()),
        )
    }));
    let proof_receipt = receipt
        .proof_receipt
        .as_ref()
        .map(|proof| {
            StatusValue::object(
                StatusFields::new()
                    .with("path", proof.path.as_str())
                    .with("digest", proof.digest.as_str()),
            )
        })
        .unwrap_or(StatusValue::Null);
    StatusValue::object(
        StatusFields::new()
            .with("entry", side.entry.as_str())
            .with("inputs", inputs)
            .with(
                "claim_key",
                receipt
                    .claim_key
                    .as_deref()
                    .map(StatusValue::from)
                    .unwrap_or(StatusValue::Null),
            )
            .with("closure", side.closure)
            .with("proof_receipt", proof_receipt),
    )
}

fn render_json(
    base: &ReviewSide,
    head: &ReviewSide,
    base_receipt: &Receipt,
    head_receipt: &Receipt,
    meaning: &[ReviewSemanticOp],
    authority: &[AuthorityChange],
    receipts: &ReceiptDiff,
    verdict: &str,
) {
    let meaning = StatusValue::array(meaning.iter().map(|operation| {
        StatusValue::object(
            StatusFields::new()
                .with("kind", operation.kind.name())
                .with("stable_id", operation.stable_id.as_str())
                .with("identity", operation.identity.as_str())
                .with(
                    "before",
                    operation
                        .before
                        .as_deref()
                        .map(StatusValue::from)
                        .unwrap_or(StatusValue::Null),
                )
                .with(
                    "after",
                    operation
                        .after
                        .as_deref()
                        .map(StatusValue::from)
                        .unwrap_or(StatusValue::Null),
                )
                .with(
                    "before_identity",
                    operation
                        .before_identity
                        .as_deref()
                        .map(StatusValue::from)
                        .unwrap_or(StatusValue::Null),
                )
                .with(
                    "after_identity",
                    operation
                        .after_identity
                        .as_deref()
                        .map(StatusValue::from)
                        .unwrap_or(StatusValue::Null),
                )
                .with("alignment", operation.alignment.name())
                .with(
                    "source_operation",
                    operation
                        .source_operation
                        .as_deref()
                        .map(StatusValue::from)
                        .unwrap_or(StatusValue::Null),
                ),
        )
    }));
    let authority = StatusValue::array(authority.iter().map(|change| {
        StatusValue::object(
            StatusFields::new()
                .with("status", change.status)
                .with("key", change.key.as_str())
                .with(
                    "before",
                    change
                        .before
                        .as_deref()
                        .map(StatusValue::from)
                        .unwrap_or(StatusValue::Null),
                )
                .with(
                    "after",
                    change
                        .after
                        .as_deref()
                        .map(StatusValue::from)
                        .unwrap_or(StatusValue::Null),
                ),
        )
    }));
    let receipt_changes =
        StatusValue::array(receipts.changes.iter().map(receipt_change_value));
    let preserved = StatusValue::array(receipts.preserved.iter().map(receipt_change_value));
    let changed_contracts =
        StatusValue::array(receipts.changed_contracts.iter().map(receipt_change_value));
    let first_difference = receipts
        .first_difference
        .as_ref()
        .map(comparison_value)
        .unwrap_or(StatusValue::Null);
    let sampled_agreements =
        StatusValue::array(receipts.sampled_agreements.iter().map(comparison_value));
    let unknown = StatusValue::array(receipts.unknown.iter().map(comparison_value));
    let unproved = StatusValue::array(receipts.unproved.iter().map(comparison_value));
    let receipts = StatusValue::object(
        StatusFields::new()
            .with("base_recorded", receipts.base_recorded)
            .with("head_recorded", receipts.head_recorded)
            .with("gained", receipts.gained)
            .with("lost", receipts.lost)
            .with("changed", receipts.changed)
            .with("retained", receipts.retained)
            .with("changes", receipt_changes)
            .with("preserved", preserved)
            .with("changed_contracts", changed_contracts)
            .with("first_difference", first_difference)
            .with("sampled_agreements", sampled_agreements)
            .with("unknown", unknown)
            .with("unproved", unproved),
    );
    let snapshots = StatusValue::object(
        StatusFields::new()
            .with("base", snapshot_value(base, base_receipt))
            .with("head", snapshot_value(head, head_receipt)),
    );
    let review = StatusValue::object(
        StatusFields::new()
            .with("schema", "jet.review/v2")
            .with("kind", "review")
            .with("snapshots", snapshots)
            .with(
                "meaning",
                StatusValue::object(StatusFields::new().with("semantic_ops", meaning)),
            )
            .with(
                "authority",
                StatusValue::object(StatusFields::new().with("changes", authority)),
            )
            .with("receipts", receipts)
            .with("verdict", verdict),
    );
    println!(
        "{}",
        StatusEnvelope::new("review", true)
            .with_field("review", review)
            .json()
    );
}

fn claim_value(claim: &Claim) -> StatusValue {
    StatusValue::object(
        StatusFields::new()
            .with("label", claim.label.as_str())
            .with("outcome", claim.outcome.as_str())
            .with("state", claim.state.as_str())
            .with("proven", claim.proven)
            .with(
                "derivation",
                claim
                    .derivation
                    .as_ref()
                    .map(derivation_value)
                    .unwrap_or(StatusValue::Null),
            )
            .with(
                "observation",
                claim
                    .observation
                    .as_ref()
                    .map(receipt_observation_value)
                    .unwrap_or(StatusValue::Null),
            ),
    )
}
fn canonical_json(value: &DataTree) -> String {
    match value {
        DataTree::Null => "null".to_string(),
        DataTree::Bool(value) => value.to_string(),
        DataTree::Int(value) => value.to_string(),
        DataTree::Float(value) => value.to_string(),
        DataTree::Number(value) => value.clone(),
        DataTree::TypedText(value) | DataTree::Text(value) => json_string(value),
        DataTree::Bytes(values) => format!(
            "[{}]",
            values
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
                .join(",")
        ),
        DataTree::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(canonical_json)
                .collect::<Vec<_>>()
                .join(",")
        ),
        DataTree::Object(values) => values
            .iter()
            .map(|(key, value)| format!("{}:{}", json_string(key), canonical_json(value)))
            .collect::<Vec<_>>()
            .join(","),
    }
}

fn object_field<'a>(object: &'a [(String, DataTree)], field: &str) -> Option<&'a DataTree> {
    object.iter().find_map(|(key, value)| (key == field).then_some(value))
}

fn string_field(object: &[(String, DataTree)], field: &str) -> Option<String> {
    object_field(object, field).and_then(json_str).map(str::to_string)
}

fn sorted_join(values: &[String]) -> String {
    let mut values = values.to_vec();
    values.sort();
    values.dedup();
    values.join(",")
}

fn positional(args: &[String]) -> Vec<String> {
    let mut values = Vec::new();
    let mut skip = false;
    for (index, argument) in args.iter().enumerate() {
        if index == 0 {
            continue;
        }
        if skip {
            skip = false;
            continue;
        }
        if matches!(
            argument.as_str(),
            "--base-receipt" | "--receipt" | "--head-receipt" | "--after-receipt"
        ) {
            skip = true;
            continue;
        }
        if argument.starts_with("--base-receipt=")
            || argument.starts_with("--receipt=")
            || argument.starts_with("--head-receipt=")
            || argument.starts_with("--after-receipt=")
            || argument.starts_with('-')
        {
            continue;
        }
        values.push(argument.clone());
    }
    values
}

fn option_value(args: &[String], names: &[&str]) -> Result<Option<String>, String> {
    for (index, argument) in args.iter().enumerate() {
        for name in names {
            if let Some(value) = argument.strip_prefix(&format!("{name}=")) {
                return Ok(Some(value.to_string()));
            }
            if argument == name {
                return args
                    .get(index + 1)
                    .filter(|value| !value.starts_with('-'))
                    .cloned()
                    .map(Some)
                    .ok_or_else(|| format!("`{name}` needs a receipt path"));
            }
        }
    }
    Ok(None)
}

fn diagnostic_usage_error(message: &str) -> ! {
    crate::cli_error!(
        @fix "E2104",
        message,
        "provide a base and reviewed Jet file after `--diagnostics`"
    );
    exit(ExitCodes::USAGE);
}

fn usage_error(message: &str) -> ! {
    crate::cli_error!(@fix "E2104", message, "provide a path after the receipt flag");
    exit(ExitCodes::USAGE);
}

fn input_error(path: &Path, message: &str) -> ! {
    crate::cli_error!(@fix "E2105", format!("could not review `{}`: {message}", path.display()), "fix the checked input and run `jet review` again");
    exit(ExitCodes::USER_ERROR);
}

fn receipt_error(message: &str) -> ! {
    crate::cli_error!(@fix "E2105", message, "provide a readable `.jetproof` receipt");
    exit(ExitCodes::USER_ERROR);
}
