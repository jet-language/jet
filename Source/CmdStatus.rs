//! D-DEVR-STATUS1=A: one read-only project truth surface.
//!
//! Status consumes authenticated receipts. It never runs a producer command;
//! missing and stale evidence stay visible as non-proven.

use jet_foundation::PerformanceBudget::CanonicalJson;
use jet_foundation::Report::render_status_json;
use std::path::{Path, PathBuf};

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
    name: &'static str,
    action: String,
    state: State,
    receipt: Option<String>,
    reason: &'static str,
}

const CLAIMS: &[(&str, &[&str], &str)] = &[
    ("check", &["check"], "jet check"),
    ("build", &["build"], "jet build"),
    ("tests", &["test"], "jet test"),
    ("proofs", &["prove"], "jet prove"),
    ("budgets", &["budget"], "jet budget check"),
    ("goldens", &["golden"], "jet test"),
    ("api", &["api"], "jet publish"),
];

pub(crate) fn run_status(args: &[String], json: bool) -> i32 {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let target = match target_arg(args, &cwd) {
        Ok(target) => target,
        Err(message) => {
            crate::emit_cli_report(
                "E2102",
                message,
                "jet status accepts one project target and registered output flags".to_string(),
                "run `jet status [<file.jet|dir>]` with one target".to_string(),
                json,
            );
            return 2;
        }
    };
    let target_path = cwd.join(&target);
    let store_args = vec!["prove".to_string(), target.clone()];
    let root = jet::ReceiptStore::receipt_root_for("prove", &store_args, &cwd);
    let store = jet::ReceiptStore::ReceiptStore::new(root);
    let receipts = match store.list() {
        Ok(receipts) => receipts,
        Err(error) => {
            let rows = CLAIMS
                .iter()
                .map(|(name, _verbs, action)| ClaimRow {
                    name,
                    action: format!("{action} {target}"),
                    state: State::Unproven,
                    receipt: None,
                    reason: "receipt store is unreadable",
                })
                .collect::<Vec<_>>();
            return render(
                &target,
                &rows,
                format!("receipt store is unreadable: {error}"),
                json,
            );
        }
    };
    let rows = CLAIMS
        .iter()
        .map(|(name, verbs, action)| {
            summarize(
                &store,
                &receipts,
                &target_path,
                name,
                verbs,
                action,
            )
        })
        .collect::<Vec<_>>();
    render(&target, &rows, String::new(), json)
}

fn target_arg(args: &[String], cwd: &Path) -> Result<String, String> {
    let mut target = None;
    let mut skip_next = false;
    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if matches!(arg.as_str(), "--json" | "--quiet" | "--no-color")
            || arg.starts_with("--color=")
        {
            continue;
        }
        if arg == "--color" {
            skip_next = true;
            continue;
        }
        if arg.starts_with('-') {
            return Err(format!("unknown `jet status` flag `{arg}`"));
        }
        if target.replace(arg.clone()).is_some() {
            return Err("`jet status` accepts at most one target".into());
        }
    }
    if let Some(target) = target {
        return Ok(target);
    }
    let root = jet::Loader::find_manifest_root(cwd).unwrap_or_else(|| cwd.to_path_buf());
    let entry = crate::find_project_entry(&root);
    Ok(entry
        .strip_prefix(cwd)
        .unwrap_or(&entry)
        .to_string_lossy()
        .replace('\\', "/"))
}

fn summarize(
    store: &jet::ReceiptStore::ReceiptStore,
    receipts: &[jet::Receipt],
    target_path: &Path,
    name: &'static str,
    verbs: &'static [&'static str],
    action: &str,
) -> ClaimRow {
    let target = canonical_or_absolute(target_path);
    let mut current_success = None;
    let mut current_failure = None;
    let mut stale = None;
    for receipt in receipts.iter().filter(|receipt| {
        verbs.contains(&receipt.claim.verb.as_str())
            && receipt_matches(&receipt.claim.inputs, &target)
    }) {
        match store.is_current(receipt) {
            Ok(true) if receipt.status == 0 => current_success = Some(short_id(&receipt.claim.key)),
            Ok(true) => current_failure = Some(short_id(&receipt.claim.key)),
            Ok(false) => stale = Some(short_id(&receipt.claim.key)),
            Err(_) => current_failure = Some(short_id(&receipt.claim.key)),
        }
    }
    let (state, receipt, reason) = if let Some(id) = current_failure {
        (State::Unproven, Some(id), "current receipt records failure")
    } else if let Some(id) = current_success {
        (State::Proven, Some(id), "current receipt records success")
    } else if let Some(id) = stale {
        (State::Stale, Some(id), "receipt input closure changed")
    } else {
        (State::Unproven, None, "no receipt proves this claim")
    };
    ClaimRow {
        name,
        action: format!("{action} {}", target.display()),
        state,
        receipt,
        reason,
    }
}

fn canonical_or_absolute(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn receipt_matches(inputs: &[jet::ReceiptInput], target: &Path) -> bool {
    let target_is_dir = target.is_dir();
    inputs.iter().any(|input| {
        let path = std::fs::canonicalize(&input.path).unwrap_or_else(|_| input.path.clone());
        path == target || (target_is_dir && path.starts_with(target))
    })
}

fn short_id(id: &str) -> String {
    id[..12.min(id.len())].to_string()
}

fn render(target: &str, rows: &[ClaimRow], error: String, json: bool) -> i32 {
    if json {
        let claims = rows
            .iter()
            .map(|row| {
                CanonicalJson::object([
                    ("action".into(), CanonicalJson::String(row.action.clone())),
                    ("claim".into(), CanonicalJson::String(row.name.into())),
                    ("reason".into(), CanonicalJson::String(row.reason.into())),
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
            .collect();
        let document = CanonicalJson::object([
            ("claims".into(), CanonicalJson::Array(claims)),
            (
                "error".into(),
                if error.is_empty() {
                    CanonicalJson::Null
                } else {
                    CanonicalJson::String(error)
                },
            ),
            ("target".into(), CanonicalJson::String(target.into())),
        ])
        .expect("status keys are unique");
        let payload = String::from_utf8(document.bytes()).expect("canonical JSON is UTF-8");
        println!(
            "{}",
            render_status_json("ok", true, "status", &format!(",\"status_report\":{payload}"))
        );
    } else {
        println!("target  {target}");
        for row in rows {
            let receipt = row.receipt.as_deref().unwrap_or("none");
            if row.state == State::Proven {
                println!(
                    "{:<8} {:<9} receipt {}",
                    row.name,
                    row.state.as_str(),
                    receipt
                );
            } else {
                println!(
                    "{:<8} {:<9} receipt {} · act: {} · {}",
                    row.name,
                    row.state.as_str(),
                    receipt,
                    row.action,
                    row.reason
                );
            }
        }
    }
    0
}
