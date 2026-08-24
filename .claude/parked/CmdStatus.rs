//! D-DEVR-STATUS1=A — render project truth from durable receipts.
//!
//! This command is deliberately observational. It reads action receipts,
//! development receipts, and proof receipts. A missing or non-current witness
//! is rendered as unproven or stale; no producer command is invoked here.

use jet_foundation::DevelopmentReceipt::{
    DevelopmentReceipt, ReceiptGrade, ReceiptStore as DevelopmentReceiptStore,
};
use jet_foundation::PerformanceBudget::{verify_budget_report, CanonicalJson};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const STATUS_SCHEMA: &str = "jet.status";

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Proven,
    Unproven,
    Stale,
}

impl State {
    fn as_str(self) -> &'static str {
        match self {
            Self::Proven => "proven",
            Self::Unproven => "unproven",
            Self::Stale => "stale",
        }
    }
}

struct ClaimRow {
    claim: String,
    state: State,
    receipt: Option<String>,
    reason: String,
    action: String,
}

struct ProofReceipt {
    id: String,
    root: String,
    input_sha256: String,
    result: String,
    exit_code: u64,
    front_end_selected: u64,
    front_end_failed: u64,
    unit_selected: u64,
    unit_failed: u64,
    unit_skipped: u64,
}

struct ReceiptScan {
    receipts: Vec<ProofReceipt>,
    invalid: bool,
}

pub(crate) fn run_status(raw: &[String], json: bool) -> i32 {
    let target = match target_arg(raw) {
        Ok(Some(target)) => target,
        Ok(None) => default_target(),
        Err(message) => {
            eprintln!("Error [E2102]: {message}\n Fix: run `jet status [<file.jet|dir>]`");
            return 2;
        }
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = project_root(&cwd, &target);
    let input_sha256 = crate::CmdProve::target_input_sha256(&target).ok();
    let proof_scan = scan_proofs(&root);
    let development_receipts = DevelopmentReceiptStore::new(&root).list();

    let mut rows = Vec::new();
    rows.push(proof_row(
        &proof_scan,
        &target,
        input_sha256.as_deref(),
        "check",
        format!("jet prove {target}"),
    ));
    rows.push(proof_row(
        &proof_scan,
        &target,
        input_sha256.as_deref(),
        "proofs",
        format!("jet prove {target}"),
    ));
    rows.push(test_row(
        &proof_scan,
        &target,
        input_sha256.as_deref(),
        format!("jet prove {target}"),
    ));
    rows.push(budget_row(&development_receipts, &target, &cwd, &root));

    let report = status_json(&target, input_sha256.as_deref(), &rows);
    if json {
        print!(
            "{}",
            String::from_utf8(report.bytes()).expect("canonical status is UTF-8")
        );
    } else {
        print_status(&target, input_sha256.as_deref(), &rows);
    }
    0
}

fn target_arg(raw: &[String]) -> Result<Option<String>, String> {
    let mut target = None;
    let mut skip_next = false;
    for arg in raw {
        if skip_next {
            skip_next = false;
            continue;
        }
        if arg == "--json" || arg == "--quiet" || arg == "--no-color" || arg.starts_with("--color=")
        {
            continue;
        }
        if arg == "--color" {
            skip_next = true;
            continue;
        }
        if arg.starts_with('-') {
            return Err(format!("unknown jet status flag {arg}"));
        }
        if target.replace(arg.clone()).is_some() {
            return Err("jet status accepts at most one target".into());
        }
    }
    Ok(target)
}

fn default_target() -> String {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = jet::Loader::find_manifest_root(&cwd).unwrap_or(cwd.clone());
    let entry = crate::find_project_entry(&root);
    if fs::symlink_metadata(&entry)
        .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
    {
        display_path(&cwd, &entry)
    } else {
        ".".into()
    }
}

fn project_root(cwd: &Path, target: &str) -> PathBuf {
    let path = cwd.join(target);
    let search = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(cwd).to_path_buf()
    };
    jet::Loader::find_manifest_root(&search).unwrap_or_else(|| cwd.to_path_buf())
}

fn display_path(cwd: &Path, path: &Path) -> String {
    path.strip_prefix(cwd)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn test_row(
    proofs: &ReceiptScan,
    target: &str,
    input_sha256: Option<&str>,
    action: String,
) -> ClaimRow {
    let mut row = proof_row(proofs, target, input_sha256, "tests", action);
    let proof_match = proofs
        .receipts
        .iter()
        .filter(|receipt| same_target(&receipt.root, target));
    let mut has_current = false;
    let mut has_current_failure = false;
    let mut has_stale = false;
    let mut receipt = None;
    for proof in proof_match {
        let current = input_sha256.is_some_and(|sha| sha == proof.input_sha256);
        if current {
            has_current = true;
            receipt = Some(short_id(&proof.id));
            if proof.result != "pass"
                || proof.exit_code != 0
                || proof.unit_selected == 0
                || proof.unit_failed != 0
                || proof.unit_skipped != 0
            {
                has_current_failure = true;
            }
        } else {
            has_stale = true;
        }
    }
    if has_current_failure {
        row.state = State::Unproven;
        row.receipt = receipt;
        row.reason = "current test evidence is incomplete or failed".into();
    } else if has_current {
        row.state = State::Proven;
        row.receipt = receipt;
        row.reason = "current test evidence passed".into();
    } else if has_stale {
        row.state = State::Stale;
        row.reason = "test receipt input closure changed".into();
    } else if proofs.invalid {
        row.state = State::Unproven;
        row.reason = "proof receipt is invalid".into();
    }
    row
}

fn proof_row(
    scan: &ReceiptScan,
    target: &str,
    input_sha256: Option<&str>,
    claim: &str,
    action: String,
) -> ClaimRow {
    let matches = scan
        .receipts
        .iter()
        .filter(|receipt| same_target(&receipt.root, target));
    let mut current_pass = None;
    let mut current_failure = None;
    let mut stale = None;
    for receipt in matches {
        if input_sha256.is_some_and(|sha| sha == receipt.input_sha256) {
            if receipt.result == "pass"
                && receipt.exit_code == 0
                && receipt.front_end_selected > 0
                && receipt.front_end_failed == 0
            {
                current_pass = Some(short_id(&receipt.id));
            } else {
                current_failure = Some(short_id(&receipt.id));
            }
        } else {
            stale = Some(short_id(&receipt.id));
        }
    }
    let (state, receipt, reason) = if let Some(id) = current_failure {
        (
            State::Unproven,
            Some(id),
            "current proof is incomplete or failed".into(),
        )
    } else if let Some(id) = current_pass {
        (State::Proven, Some(id), "current proof passed".into())
    } else if let Some(id) = stale {
        (
            State::Stale,
            Some(id),
            "proof receipt input closure changed".into(),
        )
    } else if scan.invalid {
        (State::Unproven, None, "proof receipt is invalid".into())
    } else {
        (State::Unproven, None, "no current proof receipt".into())
    };
    ClaimRow {
        claim: claim.into(),
        state,
        receipt,
        reason,
        action,
    }
}

fn budget_row(
    development_receipts: &Result<Vec<DevelopmentReceipt>, String>,
    target: &str,
    cwd: &Path,
    root: &Path,
) -> ClaimRow {
    let action = "jet budget check".into();
    let mut current_pass = None;
    let mut current_failure = None;
    let mut stale = None;
    match development_receipts {
        Ok(receipts) => {
            for receipt in receipts.iter().filter(|receipt| receipt.action == "budget") {
                match budget_receipt_current(receipt, target, cwd, root) {
                    Some(true) if receipt.grade == ReceiptGrade::Met => {
                        current_pass = Some(short_id(&receipt.id))
                    }
                    Some(true) => current_failure = Some(short_id(&receipt.id)),
                    Some(false) => stale = Some(short_id(&receipt.id)),
                    None => {}
                }
            }
        }
        Err(error) => {
            return ClaimRow {
                claim: "budgets".into(),
                state: State::Unproven,
                receipt: None,
                reason: format!("receipt store unreadable: {error}"),
                action,
            };
        }
    }
    let (state, receipt, reason) = if let Some(id) = current_failure {
        (
            State::Unproven,
            Some(id),
            "current budget receipt is not met".into(),
        )
    } else if let Some(id) = current_pass {
        (
            State::Proven,
            Some(id),
            "current budget receipt is met".into(),
        )
    } else if let Some(id) = stale {
        (
            State::Stale,
            Some(id),
            "budget receipt input closure changed".into(),
        )
    } else {
        (State::Unproven, None, "no current budget receipt".into())
    };
    ClaimRow {
        claim: "budgets".into(),
        state,
        receipt,
        reason,
        action,
    }
}

fn budget_receipt_current(
    receipt: &DevelopmentReceipt,
    target: &str,
    cwd: &Path,
    root: &Path,
) -> Option<bool> {
    let report = verify_budget_report(&receipt.payload.bytes()).ok()?;
    let wrapper = object(&report)?;
    let content = object(wrapper.get("content")?)?;
    let subject = object(content.get("subject")?)?;
    let sources = array(subject.get("member_sources")?)?;
    let target = canonical_path(&cwd.join(target))?;
    let mut relevant = false;
    let mut current = true;
    for source in sources {
        let source = object(source)?;
        let path = text(source, "path")?;
        let source_path = root.join(path);
        let source_canonical = canonical_path(&source_path)?;
        relevant |= target == source_canonical
            || target.starts_with(&source_canonical)
            || source_canonical.starts_with(&target);
        let expected = text(source, "sha256")?;
        let actual = jet::SHA256::sha256_file_hex(&source_path).ok()?;
        current &= expected == actual;
    }
    Some(relevant && current)
}

fn scan_proofs(root: &Path) -> ReceiptScan {
    let mut paths = Vec::new();
    collect_receipt_paths(&root.join(".jet").join("proofs"), &mut paths);
    let mut receipts = Vec::new();
    let mut invalid = false;
    for path in paths {
        match fs::read(&path)
            .ok()
            .and_then(|bytes| parse_proof(&bytes).ok())
        {
            Some(receipt) => receipts.push(receipt),
            None => invalid = true,
        }
    }
    ReceiptScan { receipts, invalid }
}

fn collect_receipt_paths(path: &Path, out: &mut Vec<PathBuf>) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if metadata.file_type().is_symlink() {
        return;
    }
    if metadata.is_file() {
        if path.extension().and_then(|value| value.to_str()) == Some("jetproof") {
            out.push(path.to_path_buf());
        }
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        collect_receipt_paths(&entry.path(), out);
    }
}

fn parse_proof(bytes: &[u8]) -> Result<ProofReceipt, String> {
    let value = CanonicalJson::parse_canonical(bytes)?;
    let envelope = object(&value).ok_or("proof envelope is not an object")?;
    if text(envelope, "schema") != Some("jet.jproof") || integer(envelope, "version") != Some(1) {
        return Err("unsupported proof schema".into());
    }
    let report = envelope
        .get("proofReport")
        .ok_or("proof report is missing")?;
    let id = text(envelope, "report_id").ok_or("proof report id is missing")?;
    if report.sha256() != id {
        return Err("proof report id is stale".into());
    }
    let report_fields = object(report).ok_or("proof report is not an object")?;
    let target = object(
        report_fields
            .get("target")
            .ok_or("proof target is missing")?,
    )
    .ok_or("proof target is not an object")?;
    let summaries = object(
        report_fields
            .get("summaries")
            .ok_or("proof summaries are missing")?,
    )
    .ok_or("proof summaries are not an object")?;
    let unit = object(summaries.get("unit").ok_or("unit summary is missing")?)
        .ok_or("unit summary is not an object")?;
    let front_end = object(
        summaries
            .get("frontEnd")
            .ok_or("front-end summary is missing")?,
    )
    .ok_or("front-end summary is not an object")?;
    Ok(ProofReceipt {
        id: id.into(),
        root: text(target, "root").ok_or("proof root is missing")?.into(),
        input_sha256: text(target, "inputSha256")
            .ok_or("proof input hash is missing")?
            .into(),
        result: text(report_fields, "result")
            .ok_or("proof result is missing")?
            .into(),
        exit_code: integer(report_fields, "exitCode").ok_or("proof exit code is missing")?,
        front_end_selected: integer(front_end, "selected")
            .ok_or("front-end selected count is missing")?,
        front_end_failed: integer(front_end, "failed")
            .ok_or("front-end failed count is missing")?,
        unit_selected: integer(unit, "selected").ok_or("unit selected count is missing")?,
        unit_failed: integer(unit, "failed").ok_or("unit failed count is missing")?,
        unit_skipped: integer(unit, "skipped").ok_or("unit skipped count is missing")?,
    })
}

fn same_target(left: &str, right: &str) -> bool {
    match (
        canonical_path(Path::new(left)),
        canonical_path(Path::new(right)),
    ) {
        (Some(left), Some(right)) => left == right,
        _ => normalize(left) == normalize(right),
    }
}

fn canonical_path(path: &Path) -> Option<PathBuf> {
    fs::symlink_metadata(path)
        .ok()
        .filter(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        .and_then(|_| fs::canonicalize(path).ok())
}

fn normalize(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches("./").to_string()
}

fn short_id(id: &str) -> String {
    id[..12.min(id.len())].into()
}

fn object(value: &CanonicalJson) -> Option<&BTreeMap<String, CanonicalJson>> {
    match value {
        CanonicalJson::Object(fields) => Some(fields),
        _ => None,
    }
}

fn array(value: &CanonicalJson) -> Option<&[CanonicalJson]> {
    match value {
        CanonicalJson::Array(values) => Some(values),
        _ => None,
    }
}

fn text<'a>(fields: &'a BTreeMap<String, CanonicalJson>, key: &str) -> Option<&'a str> {
    match fields.get(key) {
        Some(CanonicalJson::String(value)) => Some(value),
        _ => None,
    }
}

fn integer(fields: &BTreeMap<String, CanonicalJson>, key: &str) -> Option<u64> {
    match fields.get(key) {
        Some(CanonicalJson::Integer(value)) => value.parse().ok(),
        _ => None,
    }
}

fn status_json(target: &str, input_sha256: Option<&str>, rows: &[ClaimRow]) -> CanonicalJson {
    CanonicalJson::object([
        (
            "claims".into(),
            CanonicalJson::Array(
                rows.iter()
                    .map(|row| {
                        CanonicalJson::object([
                            ("action".into(), CanonicalJson::String(row.action.clone())),
                            ("claim".into(), CanonicalJson::String(row.claim.clone())),
                            ("reason".into(), CanonicalJson::String(row.reason.clone())),
                            (
                                "receipt".into(),
                                row.receipt
                                    .clone()
                                    .map(CanonicalJson::String)
                                    .unwrap_or(CanonicalJson::Null),
                            ),
                            (
                                "state".into(),
                                CanonicalJson::String(row.state.as_str().into()),
                            ),
                        ])
                        .expect("status claim keys are unique")
                    })
                    .collect(),
            ),
        ),
        (
            "closure".into(),
            CanonicalJson::object([
                (
                    "input_sha256".into(),
                    input_sha256
                        .map(|value| CanonicalJson::String(value.into()))
                        .unwrap_or(CanonicalJson::Null),
                ),
                ("target".into(), CanonicalJson::String(target.into())),
            ])
            .expect("status closure keys are unique"),
        ),
        ("schema".into(), CanonicalJson::String(STATUS_SCHEMA.into())),
        ("version".into(), CanonicalJson::Integer("1".into())),
    ])
    .expect("status keys are unique")
}

fn print_status(target: &str, input_sha256: Option<&str>, rows: &[ClaimRow]) {
    println!("target  {target}");
    println!(
        "closure {}",
        input_sha256
            .map(|value| &value[..12.min(value.len())])
            .unwrap_or("unknown")
    );
    for row in rows {
        let receipt = row.receipt.as_deref().unwrap_or("none");
        if row.state == State::Proven {
            println!(
                "{:<8} {:<9} receipt {}",
                row.claim,
                row.state.as_str(),
                receipt
            );
        } else {
            println!(
                "{:<8} {:<9} receipt {} · act: {} · {}",
                row.claim,
                row.state.as_str(),
                receipt,
                row.action,
                row.reason
            );
        }
    }
}
