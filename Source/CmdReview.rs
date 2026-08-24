//! D-DEVR-REVIEW1=A: one verdict over meaning, authority, and proof.
//!
//! The command joins compiler semantic operations, the shared gate ledger, and
//! proof receipts. It never asks a text diff to explain a checked change.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::exit;

use jet::Diagnostics::json_str as json_string;
use jet::ExitCodes;
use jet::Sema::GateLedger::GateLedger;
use jet_foundation::JSON::{json_get, json_str, parse, JSONValue};
use jet_semindex::{
    review_semantic_ops_with_receipts, semantic_ops_for_file, ReviewSemanticOp, SemIndex,
    SemanticOp,
};

struct ReviewSide {
    index: SemIndex,
    authority: BTreeMap<String, String>,
    source_hash: String,
    semantic_ops: Vec<SemanticOp>,
}

#[derive(Clone)]
struct AuthorityChange {
    status: &'static str,
    key: String,
    before: Option<String>,
    after: Option<String>,
}

#[derive(Clone)]
struct Claim {
    label: String,
    outcome: String,
    state: String,
    proven: bool,
}

struct Receipt {
    recorded: bool,
    claims: BTreeMap<String, Claim>,
}

struct ReceiptChange {
    status: &'static str,
    key: String,
    before: Option<Claim>,
    after: Option<Claim>,
}

struct ReceiptDiff {
    base_recorded: bool,
    head_recorded: bool,
    gained: usize,
    lost: usize,
    changed: usize,
    retained: usize,
    changes: Vec<ReceiptChange>,
}

pub(crate) fn run_review(args: &[String], json: bool) {
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
        .filter(|operation| operation.matches_transition(&base.source_hash, &head.source_hash))
        .cloned()
        .collect::<Vec<_>>();
    let meaning = review_semantic_ops_with_receipts(&base.index, &head.index, &recorded);
    let authority = authority_diff(&base.authority, &head.authority);
    let receipts = receipt_diff(&base_proof, &head_proof);
    let verdict = verdict(&authority, &receipts);
    if json {
        render_json(&meaning, &authority, &receipts, verdict);
    } else {
        render_text(&meaning, &authority, &receipts, verdict);
    }
}

fn load_side(path: &Path) -> Result<ReviewSide, String> {
    let source = fs::read_to_string(path)
        .map_err(|error| format!("could not read `{}`: {error}", path.display()))?;
    let source_hash = jet::SHA256::sha256_hex(source.as_bytes());
    let semantic_ops = semantic_ops_for_file(path, &source_hash);
    let projection = crate::CmdInspect::check_projection(path).map_err(|diagnostics| {
        diagnostics
            .iter()
            .map(|diagnostic| format!("{}: {}", diagnostic.code, diagnostic.what))
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    let root = projection.bundle.project_root.clone();
    let mut ledger = GateLedger::collect(&projection.bundle, jet::Policy::GateSet::default());
    crate::CmdGates::append_external_writers(&mut ledger, &root, &[]);
    if let Some(diagnostic) = ledger.diagnostics().first() {
        return Err(format!(
            "{}: {}",
            diagnostic.diagnostic.code, diagnostic.diagnostic.what
        ));
    }
    ledger.sort();
    let authority = authority_facts(&ledger, &projection.index);
    Ok(ReviewSide {
        index: projection.index,
        authority,
        source_hash,
        semantic_ops,
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

fn authority_tokens(value: &str) -> BTreeSet<String> {
    value
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .filter(|token| !token.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

fn read_receipt(path: Option<&Path>) -> Result<Receipt, String> {
    let Some(path) = path else {
        return Ok(Receipt {
            recorded: false,
            claims: BTreeMap::new(),
        });
    };
    let raw = fs::read_to_string(path)
        .map_err(|error| format!("could not read receipt `{}`: {error}", path.display()))?;
    let root = parse(&raw)
        .map_err(|error| format!("could not parse receipt `{}`: {error}", path.display()))?;
    let report = json_get(&root, "proofReport")
        .or_else(|| json_get(&root, "proof_report"))
        .unwrap_or(&root);
    let Some(evidence) = json_get(report, "evidence").or_else(|| json_get(report, "claims")) else {
        return Ok(Receipt {
            recorded: true,
            claims: BTreeMap::new(),
        });
    };
    let evidence = evidence
        .as_array()
        .map_err(|error| format!("receipt `{}` has invalid evidence: {error}", path.display()))?;
    let mut claims = BTreeMap::new();
    for item in evidence {
        let object = item.as_object().map_err(|error| {
            format!(
                "receipt `{}` has invalid evidence item: {error}",
                path.display()
            )
        })?;
        let key = claim_key(object);
        let outcome = string_field(object, "outcome")
            .or_else(|| string_field(object, "result"))
            .unwrap_or_else(|| "unknown".to_string());
        let state = string_field(object, "state").unwrap_or_else(|| "unknown".to_string());
        let label = string_field(object, "claimId")
            .or_else(|| string_field(object, "claim"))
            .or_else(|| string_field(object, "id"))
            .or_else(|| string_field(object, "property"))
            .unwrap_or_else(|| key.clone());
        claims.insert(
            key,
            Claim {
                label,
                proven: proof_is_present(&outcome, &state),
                outcome,
                state,
            },
        );
    }
    Ok(Receipt {
        recorded: true,
        claims,
    })
}

fn claim_key(object: &BTreeMap<String, JSONValue>) -> String {
    for field in ["claimId", "claim"] {
        if let Some(value) = string_field(object, field) {
            return format!("{field}:{value}");
        }
    }
    let mut parts = Vec::new();
    for field in [
        "kind", "facet", "producer", "contract", "budget", "property", "reason", "solver",
    ] {
        if let Some(value) = object.get(field) {
            parts.push(format!("{field}={}", canonical_json(value)));
        }
    }
    if let Some(source) = object.get("source") {
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
    };
    for key in keys {
        let old = before.claims.get(&key);
        let new = after.claims.get(&key);
        let status = match (old, new) {
            (Some(old), Some(new)) if old.proven && !new.proven => Some("lost"),
            (Some(old), None) if old.proven => Some("lost"),
            (None, Some(new)) if new.proven => Some("gained"),
            (Some(old), Some(new)) if !old.proven && new.proven => Some("gained"),
            (Some(old), Some(new)) if old.outcome != new.outcome || old.state != new.state => {
                Some("changed")
            }
            _ => None,
        };
        let Some(status) = status else {
            diff.retained += 1;
            continue;
        };
        match status {
            "gained" => diff.gained += 1,
            "lost" => diff.lost += 1,
            _ => diff.changed += 1,
        }
        diff.changes.push(ReceiptChange {
            status,
            key,
            before: old.cloned(),
            after: new.cloned(),
        });
    }
    diff
}

fn proof_is_present(outcome: &str, state: &str) -> bool {
    matches!(
        outcome.to_ascii_lowercase().as_str(),
        "proved" | "passed" | "observed" | "met" | "verified" | "valid"
    ) || matches!(
        state.to_ascii_lowercase().as_str(),
        "proved" | "passed" | "observed" | "met" | "verified" | "valid"
    )
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
                "  {}: {} [{}]",
                operation.kind.name(),
                operation.identity,
                operation.stable_id
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
    println!("verdict    {verdict}");
}

fn render_json(
    meaning: &[ReviewSemanticOp],
    authority: &[AuthorityChange],
    receipts: &ReceiptDiff,
    verdict: &str,
) {
    let meaning = meaning
        .iter()
        .map(|operation| {
            format!(
                "{{\"kind\":{},\"stable_id\":{},\"identity\":{},\"before\":{},\"after\":{}}}",
                json_string(operation.kind.name()),
                json_string(&operation.stable_id),
                json_string(&operation.identity),
                optional_json(operation.before.as_ref()),
                optional_json(operation.after.as_ref()),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let authority = authority
        .iter()
        .map(|change| {
            format!(
                "{{\"status\":{},\"key\":{},\"before\":{},\"after\":{}}}",
                json_string(change.status),
                json_string(&change.key),
                optional_json(change.before.as_ref()),
                optional_json(change.after.as_ref()),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let receipt_changes = receipts
        .changes
        .iter()
        .map(|change| {
            format!(
                "{{\"status\":{},\"key\":{},\"before\":{},\"after\":{}}}",
                json_string(change.status),
                json_string(&change.key),
                optional_claim_json(change.before.as_ref()),
                optional_claim_json(change.after.as_ref()),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    println!(
        "{{\"schema_version\":1,\"kind\":\"review\",\"meaning\":{{\"semantic_ops\":[{meaning}]}},\"authority\":{{\"changes\":[{authority}]}},\"receipts\":{{\"base_recorded\":{},\"head_recorded\":{},\"gained\":{},\"lost\":{},\"changed\":{},\"retained\":{},\"changes\":[{receipt_changes}]}},\"verdict\":{}}}",
        receipts.base_recorded,
        receipts.head_recorded,
        receipts.gained,
        receipts.lost,
        receipts.changed,
        receipts.retained,
        json_string(verdict),
    );
}

fn optional_json(value: Option<&String>) -> String {
    value
        .map(|value| json_string(value))
        .unwrap_or_else(|| "null".to_string())
}

fn optional_claim_json(claim: Option<&Claim>) -> String {
    let Some(claim) = claim else {
        return "null".to_string();
    };
    format!(
        "{{\"label\":{},\"outcome\":{},\"state\":{},\"proven\":{}}}",
        json_string(&claim.label),
        json_string(&claim.outcome),
        json_string(&claim.state),
        claim.proven,
    )
}

fn canonical_json(value: &JSONValue) -> String {
    match value {
        JSONValue::Null => "null".to_string(),
        JSONValue::Bool(value) => value.to_string(),
        JSONValue::Number(value) => value.to_string(),
        JSONValue::Flt(value) => value.to_string(),
        JSONValue::String(value) => json_string(value),
        JSONValue::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(canonical_json)
                .collect::<Vec<_>>()
                .join(",")
        ),
        JSONValue::Object(values) => values
            .iter()
            .map(|(key, value)| format!("{}:{}", json_string(key), canonical_json(value)))
            .collect::<Vec<_>>()
            .join(","),
    }
}

fn string_field(object: &BTreeMap<String, JSONValue>, field: &str) -> Option<String> {
    object.get(field).and_then(json_str).map(str::to_string)
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
