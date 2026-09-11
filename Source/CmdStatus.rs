//! D-DEVR-STATUS1=A: one read-only project truth surface.
//!
//! Status consumes authenticated receipts. It never runs a producer command;
//! stale stored evidence stays visible, and absent evidence creates no claim.

use jet_foundation::Report::{StatusEnvelope, StatusFields, StatusValue};
use std::collections::BTreeMap;
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
    name: String,
    action: String,
    state: State,
    receipt: Option<String>,
    sections: StatusValue,
    reason: &'static str,
}

pub(crate) fn run_status(args: &[String], json: bool) -> i32 {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let target = match target_arg(args, &cwd) {
        Ok(target) => target,
        Err(message) => {
            emit_report(
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
            return render(
                &target,
                &[],
                format!("receipt store is unreadable: {error}"),
                json,
            );
        }
    };
    let rows = rows_from_receipts(&store, &receipts, &target_path, &target);
    render(&target, &rows, String::new(), json)
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum CacheOption {
    To,
    Host,
}

enum CacheRequest {
    Status,
    Prune { target_bytes: u64 },
    Limit { limit_bytes: u64 },
}

/// Run the machine-wide artifact-store commands.
pub(crate) fn run_cache(args: &[String], json: bool) -> i32 {
    let request = match parse_cache_request(args) {
        Ok(request) => request,
        Err(message) => return cache_usage_error(message, json),
    };
    match request {
        CacheRequest::Status => run_cache_status(json),
        CacheRequest::Prune { target_bytes } => run_cache_prune(target_bytes, json),
        CacheRequest::Limit { limit_bytes } => run_cache_limit(limit_bytes, json),
    }
}

fn parse_cache_request(args: &[String]) -> Result<CacheRequest, String> {
    let mut command = None;
    let mut option: Option<(CacheOption, String)> = None;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        match arg {
            "--json" | "--quiet" | "--no-color" => {
                index += 1;
                continue;
            }
            "--color" => {
                if args.get(index + 1).is_none() {
                    return Err("`jet cache --color` needs auto, always, or never".to_string());
                }
                index += 2;
                continue;
            }
            _ if arg.starts_with("--color=") => {
                index += 1;
                continue;
            }
            "--to" | "--host" => {
                let kind = if arg == "--to" {
                    CacheOption::To
                } else {
                    CacheOption::Host
                };
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| format!("`jet cache {arg}` needs a size"))?;
                if option
                    .replace((kind, value.clone()))
                    .is_some()
                {
                    return Err("`jet cache` accepts one size option".to_string());
                }
                index += 2;
                continue;
            }
            _ if arg.starts_with("--to=") || arg.starts_with("--host=") => {
                let kind = if arg.starts_with("--to=") {
                    CacheOption::To
                } else {
                    CacheOption::Host
                };
                let value = arg
                    .split_once('=')
                    .map(|(_, value)| value)
                    .unwrap_or_default();
                if option
                    .replace((kind, value.to_string()))
                    .is_some()
                {
                    return Err("`jet cache` accepts one size option".to_string());
                }
                index += 1;
                continue;
            }
            _ if arg.starts_with('-') => {
                return Err(format!("unknown `jet cache` flag `{arg}`"));
            }
            _ => {
                if command.replace(arg).is_some() {
                    return Err("`jet cache` accepts one command".to_string());
                }
                index += 1;
            }
        }
    }

    let command = command.ok_or_else(|| "missing `jet cache` command".to_string())?;
    match (command, option) {
        ("status", None) => Ok(CacheRequest::Status),
        ("status", Some(_)) => Err("`jet cache status` does not take a size option".to_string()),
        ("prune", Some((CacheOption::To, value))) => Ok(CacheRequest::Prune {
            target_bytes: parse_cache_size(&value)?,
        }),
        ("prune", Some((CacheOption::Host, _))) => {
            Err("`jet cache prune` requires `--to <size>`".to_string())
        }
        ("prune", None) => Err("`jet cache prune` requires `--to <size>`".to_string()),
        ("limit", Some((CacheOption::Host, value))) => Ok(CacheRequest::Limit {
            limit_bytes: parse_cache_size(&value)?,
        }),
        ("limit", Some((CacheOption::To, _))) => {
            Err("`jet cache limit` requires `--host <size>`".to_string())
        }
        ("limit", None) => Err("`jet cache limit` requires `--host <size>`".to_string()),
        (other, _) => Err(format!("`{other}` isn't a jet cache command")),
    }
}

fn parse_cache_size(value: &str) -> Result<u64, String> {
    jet_store::parse_size(value).map_err(|error| format!("invalid cache size `{value}`: {error}"))
}

fn run_cache_status(json: bool) -> i32 {
    let store = match jet_store::Store::from_env() {
        Ok(store) => store,
        Err(error) => return cache_store_error("open the machine-wide store", error, json),
    };
    let status = match store.status() {
        Ok(status) => status,
        Err(error) => return cache_store_error("read the machine-wide store", error, json),
    };
    render_cache_status(&store, &status, json);
    jet::ExitCodes::OK
}

fn run_cache_prune(target_bytes: u64, json: bool) -> i32 {
    let store = match jet_store::Store::from_env() {
        Ok(store) => store,
        Err(error) => return cache_store_error("open the machine-wide store", error, json),
    };
    let report = match store.prune_to(target_bytes) {
        Ok(report) => report,
        Err(error) => return cache_store_error("prune the machine-wide store", error, json),
    };
    if json {
        let cache = StatusFields::new()
            .with("after_bytes", report.after_bytes)
            .with("before_bytes", report.before_bytes)
            .with("blocked", report.blocked)
            .with(
                "freed_bytes",
                report.before_bytes.saturating_sub(report.after_bytes),
            )
            .with("pinned_bytes", report.pinned_bytes)
            .with("removed_entries", report.removed.len())
            .with("target_bytes", report.target_bytes);
        println!(
            "{}",
            StatusEnvelope::new("cache.prune", true)
                .with_field("cache", StatusValue::object(cache))
                .json()
        );
    } else {
        let freed = report.before_bytes.saturating_sub(report.after_bytes);
        println!(
            "prune      {} -> {} (target {})",
            format_size(report.before_bytes),
            format_size(report.after_bytes),
            format_size(report.target_bytes)
        );
        println!(
            "removed    {} entries · freed {}",
            report.removed.len(),
            format_size(freed)
        );
        println!("pinned     {}", format_size(report.pinned_bytes));
        if report.blocked {
            println!("result     target blocked by live leases");
        }
    }
    jet::ExitCodes::OK
}

fn run_cache_limit(limit_bytes: u64, json: bool) -> i32 {
    let store = match jet_store::Store::from_env() {
        Ok(store) => store,
        Err(error) => return cache_store_error("open the machine-wide store", error, json),
    };
    if let Err(error) = store.set_host_limit(Some(limit_bytes)) {
        return cache_store_error("set the host store limit", error, json);
    }
    if json {
        let cache = StatusFields::new()
            .with("host_limit_bytes", limit_bytes)
            .with("root", store.root().display().to_string());
        println!(
            "{}",
            StatusEnvelope::new("cache.limit", true)
                .with_field("cache", StatusValue::object(cache))
                .json()
        );
    } else {
        println!("host limit {} (persisted)", format_size(limit_bytes));
        println!("store      {}", store.root().display());
    }
    jet::ExitCodes::OK
}

fn render_cache_status(store: &jet_store::Store, status: &jet_store::StoreStatus, json: bool) {
    let blobs = entry_count(status, jet_store::EntryKind::Blob);
    let records = entry_count(status, jet_store::EntryKind::Action);
    let lto = entry_count(status, jet_store::EntryKind::Lto);
    if json {
        let cache = StatusFields::new()
            .with(
                "available_bytes",
                status
                    .available_bytes
                    .map(StatusValue::from)
                    .unwrap_or(StatusValue::Null),
            )
            .with("blobs", blobs)
            .with("entries", status.entries.len())
            .with("footprint_bytes", status.footprint_bytes)
            .with(
                "host_limit_bytes",
                status
                    .host_limit_bytes
                    .map(StatusValue::from)
                    .unwrap_or(StatusValue::Null),
            )
            .with("limit_bytes", status.limit_bytes)
            .with("live_leases", status.live_leases)
            .with("records", records)
            .with("reserve_bytes", status.reserve_bytes)
            .with("root", status.root.display().to_string())
            .with(
                "tiers",
                StatusValue::array(status.tiers.iter().cloned().map(StatusValue::String)),
            )
            .with("thinlto_caches", lto);
        println!(
            "{}",
            StatusEnvelope::new("cache.status", true)
                .with_field("cache", StatusValue::object(cache))
                .json()
        );
        return;
    }

    let limit_source = if status.host_limit_bytes.is_some() {
        "host policy"
    } else if store.config().cap_bytes().is_some() {
        "JET_STORE_CAP_BYTES"
    } else {
        "adaptive rule"
    };
    println!("store      {}", status.root.display());
    println!(
        "limit      {} ({limit_source})",
        format_size(status.limit_bytes)
    );
    println!(
        "reserve    keep {} free on that filesystem",
        format_size(status.reserve_bytes)
    );
    println!(
        "used       {} · {} blobs · {} records · {} ThinLTO caches",
        format_size(status.footprint_bytes),
        blobs,
        records,
        lto
    );
    println!("leases     {} live", status.live_leases);
    println!("tiers      {}", status.tiers.join(" · "));
}

fn entry_count(status: &jet_store::StoreStatus, kind: jet_store::EntryKind) -> usize {
    status.entries.iter().filter(|entry| entry.kind == kind).count()
}


fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if value.fract() == 0.0 || value >= 10.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn cache_usage_error(message: String, json: bool) -> i32 {
    emit_report(
        "E2102",
        message,
        "`jet cache` has one read-only status command and two explicit mutations".to_string(),
        "run `jet cache status`, `jet cache prune --to <size>`, or `jet cache limit --host <size>`"
            .to_string(),
        json,
    );
    jet::ExitCodes::USAGE
}

fn cache_store_error(operation: &str, error: jet_store::StoreError, json: bool) -> i32 {
    emit_report(
        "E2105",
        format!("could not {operation}: {error}"),
        "cache commands use one machine-wide store with atomic, verified entries".to_string(),
        "check the JET_STORE_DIR path and permissions, then retry the cache command".to_string(),
        json,
    );
    jet::ExitCodes::USER_ERROR
}
fn emit_report(code: &str, what: String, why: String, fix: String, json: bool) {
    let diagnostic = jet::Diagnostics::Diagnostic::error(code, what, why, fix, None);
    if json {
        let report = diagnostic.to_report(
            &jet::Diagnostics::ReportPath::from_process(""),
            "",
        );
        print!(
            "{}",
            StatusEnvelope::new("status", false)
                .with_report(report)
                .json()
        );
    } else {
        eprint!(
            "{}",
            jet::render_all_colored("", "", std::slice::from_ref(&diagnostic), false)
        );
    }
}


fn rows_from_receipts(
    store: &jet::ReceiptStore::ReceiptStore,
    receipts: &[jet::Receipt],
    target_path: &Path,
    display_target: &str,
) -> Vec<ClaimRow> {
    let target = canonical_or_absolute(target_path);
    let mut by_verb = BTreeMap::<String, Vec<&jet::Receipt>>::new();
    for receipt in receipts {
        if receipt_matches(&receipt.claim.inputs, &target) {
            by_verb
                .entry(receipt.claim.verb.clone())
                .or_default()
                .push(receipt);
        }
    }
    by_verb
        .into_iter()
        .map(|(name, receipts)| {
            let action = format!("jet {name}");
            summarize(store, &receipts, name, action, display_target)
        })
        .collect()
}

fn target_arg(args: &[String], cwd: &Path) -> Result<String, String> {
    let mut target = None;
    let mut skip_next = false;
    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if matches!(
            arg.as_str(),
            jet::CLI::MACHINE_OUTPUT_FLAG | "--quiet" | "--no-color"
        ) || arg.starts_with("--color=")
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
    receipts: &[&jet::Receipt],
    name: String,
    action: String,
    display_target: &str,
) -> ClaimRow {
    let mut current_success = None;
    let mut current_failure = None;
    let mut stale = None;
    for receipt in receipts {
        match store.is_current(receipt) {
            Ok(true) if receipt.status == 0 => current_success = Some(*receipt),
            Ok(true) => current_failure = Some(*receipt),
            Ok(false) => stale = Some(*receipt),
            Err(_) => current_failure = Some(*receipt),
        }
    }
    let (state, selected, reason) = if let Some(receipt) = current_failure {
        (
            State::Unproven,
            Some(receipt),
            "current receipt records failure",
        )
    } else if let Some(receipt) = current_success {
        (
            State::Proven,
            Some(receipt),
            "current receipt records success",
        )
    } else if let Some(receipt) = stale {
        (State::Stale, Some(receipt), "receipt input closure changed")
    } else {
        (State::Unproven, None, "no receipt proves this claim")
    };
    let sections = selected
        .and_then(|receipt| store.query_sections(receipt, None).ok())
        .map(|value| {
            let bytes = value.bytes();
            let text = std::str::from_utf8(&bytes).expect("receipt sections are UTF-8");
            StatusValue::parse(text.trim_end_matches('\n'))
                .expect("receipt sections contain valid canonical JSON")
        })
        .unwrap_or_else(empty_sections);
    ClaimRow {
        name,
        action: format!("{action} {display_target}"),
        state,
        receipt: selected.map(|receipt| short_id(&receipt.claim.key)),
        sections,
        reason,
    }
}

fn empty_sections() -> StatusValue {
    StatusValue::object(StatusFields::new())
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
        let claims = StatusValue::array(rows.iter().map(|row| {
            StatusValue::object(
                StatusFields::new()
                    .with("action", row.action.clone())
                    .with("claim", row.name.clone())
                    .with("reason", row.reason)
                    .with(
                        "receipt",
                        row.receipt
                            .clone()
                            .map(StatusValue::String)
                            .unwrap_or(StatusValue::Null),
                    )
                    .with("sections", row.sections.clone())
                    .with("state", row.state.as_str()),
            )
        }));
        println!(
            "{}",
            StatusEnvelope::new("status", true)
                .with_field("claims", claims)
                .with_field(
                    "error",
                    if error.is_empty() {
                        StatusValue::Null
                    } else {
                        StatusValue::String(error)
                    },
                )
                .with_field("target", target)
                .json()
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
