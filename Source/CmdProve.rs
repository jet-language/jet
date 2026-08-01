//! D-PROVE-SEM1: first real `jet prove` producer tranche.
//!
//! This owns target discovery and front-end evidence. Runtime producers and
//! artifacts are deliberately not represented here until they can contribute
//! genuine typed evidence to the same report.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::exit;
use std::process::Command;
use std::process::Stdio;
use std::time::{Duration, Instant};

use jet::Diagnostics::{span_line_col, Diagnostic, Severity};
use jet::ExitCodes;

#[derive(Clone)]
struct Member {
    path: String,
    bytes: Vec<u8>,
    sha256: String,
}

struct Target {
    kind: &'static str,
    root: String,
    members: Vec<Member>,
    input_sha256: String,
}

struct FrontEndItem {
    id: String,
    path: String,
    diagnostic: Option<Diagnostic>,
    source: String,
}

struct TestItem {
    id: String,
    path: String,
    state: u8,
    kind: u8,
    message: String,
    line: u32,
    name: String,
    seed: String,
}

struct ProducerRecord {
    kind: u8,
    state: u8,
    name: String,
    message: String,
    file: String,
    line: u32,
}

enum ChildOutcome {
    Exited(Option<i32>),
    TimedOut,
    LaunchFailed,
}

pub(crate) fn run_prove(args: &[String], json: bool) {
    let mut positional = Vec::new();
    let mut lenses = Vec::new();
    let mut capture: Option<crate::ProveReplay::CaptureOpts> = None;
    let mut replay: Option<String> = None;
    let mut i = 0usize;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--json" {
            i += 1;
            continue;
        }
        if let Some(opts) = crate::ProveReplay::parse_capture_flag(arg) {
            if capture.is_some() || replay.is_some() {
                eprintln!("error: `jet prove` accepts at most one of `--capture` / `--replay`");
                exit(ExitCodes::USAGE);
            }
            capture = Some(opts);
            i += 1;
            continue;
        }
        if let Some(parsed) = crate::ProveReplay::parse_replay_flag(arg, args.get(i + 1).map(String::as_str)) {
            if capture.is_some() || replay.is_some() {
                eprintln!("error: `jet prove` accepts at most one of `--capture` / `--replay`");
                exit(ExitCodes::USAGE);
            }
            match parsed {
                Ok(path) => {
                    replay = Some(path);
                    i += if arg == "--replay" { 2 } else { 1 };
                }
                Err(message) => {
                    eprintln!("error: {message}");
                    exit(ExitCodes::USAGE);
                }
            }
            continue;
        }
        if let Some(value) = arg.strip_prefix("--lens=") {
            validate_lens(value, positional.first().copied().unwrap_or("TARGET"), json);
            lenses.push(value.to_string());
            i += 1;
            continue;
        }
        if arg == "--lens" {
            let value = args.get(i + 1).map(String::as_str).unwrap_or("");
            validate_lens(value, positional.first().copied().unwrap_or("TARGET"), json);
            lenses.push(value.to_string());
            i += 2;
            continue;
        }
        if arg.starts_with('-') {
            eprintln!("error: unknown `jet prove` flag `{arg}`");
            exit(ExitCodes::USAGE);
        }
        positional.push(arg);
        i += 1;
    }
    if positional.len() != 1 {
        eprintln!("error: `jet prove` needs exactly one file, package, or workspace target");
        eprintln!(" Fix: jet prove path/to/program.jet");
        exit(ExitCodes::USAGE);
    }
    let target = match resolve_target(positional[0]) {
        Ok(target) => target,
        Err(message) => {
            eprintln!("error: {message}");
            exit(ExitCodes::USER_ERROR);
        }
    };

    let identity = crate::ProveReplay::ReplayIdentity {
        entry: target.root.clone(),
        source_digest: target.input_sha256.clone(),
        execution_adapter: "dev-tir-v1".to_string(),
        target_triple: "x86_64-linux".to_string(),
    };
    if let Some(opts) = capture {
        exit(crate::ProveReplay::run_safe_capture(&identity, &opts, json));
    }
    if let Some(path) = replay {
        exit(crate::ProveReplay::run_replay(&identity, &path, json));
    }

    let mut items = Vec::new();
    for member in &target.members {
        let source = String::from_utf8_lossy(&member.bytes).into_owned();
        let diagnostics = jet::check_with_path(&member.path);
        if diagnostics.is_empty() {
            items.push(FrontEndItem {
                id: evidence_id(&target, "front_end", &member.path, "0:0-0:0", "program checked"),
                path: member.path.clone(),
                diagnostic: None,
                source,
            });
        } else {
            for diagnostic in diagnostics {
                let span = diagnostic_span(&source, &diagnostic);
                let claim = format!("{}:{}", diagnostic.code, diagnostic.what);
                items.push(FrontEndItem {
                    id: evidence_id(&target, "front_end", &member.path, &span, &claim),
                    path: member.path.clone(),
                    diagnostic: Some(diagnostic),
                    source: source.clone(),
                });
            }
        }
    }

    let failed = items.iter().filter(|item| item.diagnostic.is_some()).count();
    let proved = items.len() - failed;
    let (tests, producer_exit) = if failed == 0 {
        run_test_producers(&target)
    } else {
        (Vec::new(), ExitCodes::OK)
    };
    let test_failed = tests.iter().filter(|item| (item.kind == 0 || item.kind == 3 || item.kind == 4) && item.state == 1).count();
    let budgets = budget_projection(&target);
    let budget_failed = budgets.facts.iter().any(|fact| fact.outcome == "fail");
    let enable_solver = lenses.iter().any(|lens| lens == "solver");
    let solver_members: Vec<(String, String)> = target
        .members
        .iter()
        .map(|member| {
            (
                member.path.clone(),
                String::from_utf8_lossy(&member.bytes).into_owned(),
            )
        })
        .collect();
    let solver = match crate::ProveSolver::run_solver_producer(&solver_members, enable_solver) {
        Ok(items) => items,
        Err(message) => {
            eprintln!("error: solver producer failed: {message}");
            exit(ExitCodes::ICE);
        }
    };
    let solver_disproved = solver
        .iter()
        .any(|item| matches!(item.outcome, crate::ProveSolver::SolverOutcome::Disproved { .. }));
    let exit_code = if producer_exit == ExitCodes::ICE {
        ExitCodes::ICE
    } else if producer_exit == ExitCodes::RUNTIME_PANIC {
        ExitCodes::RUNTIME_PANIC
    } else if failed > 0 || test_failed > 0 || budget_failed || solver_disproved {
        ExitCodes::USER_ERROR
    } else {
        ExitCodes::OK
    };
    let report = render_report(
        &target,
        &items,
        &tests,
        &budgets,
        &solver,
        &lenses,
        proved,
        failed,
        exit_code,
    );
    // D-JPROOF1=A (#1127): persist the exact ProofReport under the canonical
    // `.jetproof` envelope. `--json` still prints only the ProofReport object.
    if let Err(message) = write_jetproof(&target, &report) {
        eprintln!("error: failed to write .jetproof: {message}");
        exit(ExitCodes::ICE);
    }
    if json {
        println!("{report}");
    } else {
        let show = |facet: &str| lens_shows(&lenses, facet);
        if !lenses.is_empty() {
            let mut uniq = lenses.clone();
            uniq.sort();
            uniq.dedup();
            println!("LENSES   {}", uniq.join(", "));
        }
        if show("refinements") || show("all") || show("effects") || show("taint") || lenses.is_empty() {
            println!("CHECKED  front end: {proved} proved, {failed} failed");
        }
        for item in &items {
            if let Some(diagnostic) = &item.diagnostic {
                eprintln!("{}", diagnostic.render(&item.path, &item.source));
            }
        }
        if show("tests") || lenses.is_empty() {
            if !tests.is_empty() {
                let passed = tests.iter().filter(|item| item.kind == 0 && item.state == 0).count();
                let skipped = tests.iter().filter(|item| item.kind == 0 && item.state == 2).count();
                println!("TESTS    unit: {passed} passed, {test_failed} failed, {skipped} skipped");
            }
        }
        if show("budgets") || lenses.is_empty() {
            if !budgets.facts.is_empty() {
                let met = budgets.facts.iter().filter(|fact| fact.outcome == "pass").count();
                let failed = budgets.facts.iter().filter(|fact| fact.outcome == "fail").count();
                let warned = budgets.facts.len() - met - failed;
                println!("BUDGETS  {met} met, {failed} failed, {warned} warned · verified canonical reports");
            }
        }
        if enable_solver {
            let (selected, proved_s, disproved_s, unknown_s, _) =
                crate::ProveSolver::summarize(&solver);
            println!(
                "SOLVER   {selected} selected, {proved_s} proved, {disproved_s} disproved, {unknown_s} unknown"
            );
            for item in &solver {
                if let crate::ProveSolver::SolverOutcome::Disproved { assignment, .. } = &item.outcome
                {
                    let values = assignment
                        .iter()
                        .map(|(k, v)| format!("{k} = {v}"))
                        .collect::<Vec<_>>()
                        .join(" and ");
                    eprintln!(
                        "Error [E2950]: solver found a counterexample to {}",
                        item.obligation.kind
                    );
                    eprintln!(" Why: {values} satisfy the assumptions but make the claim false");
                    eprintln!(
                        " Fix: change {} so every return satisfies the claim, or correct the contract",
                        item.obligation.origin
                    );
                }
            }
        }
        let unavailable = tests.iter().filter(|item| item.state == 3).count();
        println!(
            "RESULT   {}",
            if failed > 0 || test_failed > 0 || budget_failed || solver_disproved {
                "fail"
            } else if unavailable > 0
                || solver.iter().any(|item| {
                    matches!(item.outcome, crate::ProveSolver::SolverOutcome::Unknown { .. })
                })
            {
                "pass_incomplete"
            } else {
                "pass"
            }
        );
    }
    exit(exit_code);
}

fn lens_shows(lenses: &[String], facet: &str) -> bool {
    if lenses.is_empty() {
        return true;
    }
    if lenses.iter().any(|lens| lens == "all") {
        return facet != "solver";
    }
    lenses.iter().any(|lens| lens == facet)
}

const PROOF_LENSES: &[&str] = &[
    "all", "refinements", "effects", "taint", "contracts", "tests", "budgets", "replay",
    "solver",
];

fn validate_lens(value: &str, target: &str, json_mode: bool) {
    if PROOF_LENSES.contains(&value) {
        return;
    }
    let what = format!("unknown proof lens `{value}`");
    let why = "`jet prove` accepts all, refinements, effects, taint, contracts, tests, budgets, replay, solver";
    let fix = format!("try `jet prove {target} --lens tests`");
    if json_mode {
        println!(
            "{{\"schema_version\":1,\"code\":\"E2941\",\"severity\":\"error\",\"message\":{},\"why\":{},\"fix\":{},\"detail\":null,\"file\":null,\"line\":null,\"col\":null,\"span\":null,\"edit\":null}}",
            json(&what), json(why), json(&fix)
        );
    } else {
        eprintln!("Error [E2941]: {what}");
        eprintln!(" Why: {why}");
        eprintln!(" Fix: {fix}");
    }
    exit(ExitCodes::USAGE);
}

fn run_test_producers(target: &Target) -> (Vec<TestItem>, i32) {
    let mut items = Vec::new();
    let mut highest_exit = ExitCodes::OK;
    for member in &target.members {
        if !jet::has_test_blocks(&member.path) && jet::Doctest::discover(&String::from_utf8_lossy(&member.bytes)).is_empty() {
            continue;
        }
        let report_path = std::env::temp_dir().join(format!(
            "jet_prove_test_{}_{}.bin",
            std::process::id(),
            &member.sha256[..16]
        ));
        let _ = fs::remove_file(&report_path);
        let mut command = Command::new(std::env::current_exe().unwrap_or_else(|_| PathBuf::from("jet")));
        command
            .args(["test", &member.path, "--serial"])
            .env("JET_TEST_PROOF_REPORT", &report_path)
            .env("JET_PROVE_FRESH_TEST", "1");
        match supervise_child(&mut command, Duration::from_secs(120)) {
            ChildOutcome::Exited(Some(code)) => {
                if code == ExitCodes::ICE {
                    highest_exit = ExitCodes::ICE;
                } else if code == ExitCodes::RUNTIME_PANIC && highest_exit != ExitCodes::ICE {
                    highest_exit = ExitCodes::RUNTIME_PANIC;
                }
                match read_test_report(&report_path) {
                    Ok(records) => {
                        if records.iter().any(|record| record.kind == 2 || (record.kind == 1 && record.state == 1))
                            && highest_exit != ExitCodes::ICE
                        {
                            highest_exit = ExitCodes::RUNTIME_PANIC;
                        }
                        for record in records {
                            let claim = format!("{}:{}:{}", record.kind, record.name, record.message);
                            items.push(TestItem {
                                id: evidence_id(target, if record.kind == 1 { "contract" } else { "unit" }, &member.path, "0:0-0:0", &claim),
                                path: if record.kind == 3 || record.file.is_empty() { member.path.clone() } else { record.file.clone() },
                                state: record.state,
                                kind: record.kind,
                                message: record.message,
                                line: record.line,
                                name: record.name,
                                seed: if record.kind == 3 { record.file } else { String::new() },
                            });
                        }
                    }
                    Err(_) => highest_exit = ExitCodes::ICE,
                }
            }
            ChildOutcome::Exited(None) => {
                highest_exit = ExitCodes::ICE;
            }
            ChildOutcome::TimedOut | ChildOutcome::LaunchFailed => {
                items.push(TestItem {
                    id: evidence_id(target, "unit", &member.path, "0:0-0:0", "producer unavailable"),
                    path: member.path.clone(),
                    state: 3,
                    kind: 0,
                    message: String::new(),
                    line: 0,
                    name: String::new(),
                    seed: String::new(),
                });
            }
        }
        let _ = fs::remove_file(report_path);
    }
    (items, highest_exit)
}

fn supervise_child(command: &mut Command, deadline: Duration) -> ChildOutcome {
    command.stdout(Stdio::null()).stderr(Stdio::null());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => return ChildOutcome::LaunchFailed,
    };
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return ChildOutcome::Exited(status.code()),
            Ok(None) if started.elapsed() < deadline => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return ChildOutcome::TimedOut;
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return ChildOutcome::Exited(None);
            }
        }
    }
}

fn read_test_report(path: &Path) -> Result<Vec<ProducerRecord>, String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("oversized test producer report".into());
    }
    read_test_report_bytes(&bytes)
}

fn read_test_report_bytes(bytes: &[u8]) -> Result<Vec<ProducerRecord>, String> {
    if bytes.get(..8) != Some(b"JETTEST2") {
        return Err("invalid test producer report".into());
    }
    let mut at = 8usize;
    let mut records = Vec::new();
    while at < bytes.len() {
        if records.len() == 10_000 { return Err("too many test producer records".into()); }
        let kind = *bytes.get(at).ok_or("truncated test producer report")?;
        let state = *bytes.get(at + 1).ok_or("truncated test producer report")?;
        at += 2;
        let line = read_u64(&bytes, &mut at)? as u32;
        let name = read_string(&bytes, &mut at)?;
        let message = read_string(&bytes, &mut at)?;
        let file = read_string(&bytes, &mut at)?;
        records.push(ProducerRecord { kind, state, name, message, file, line });
    }
    Ok(records)
}

fn read_u64(bytes: &[u8], at: &mut usize) -> Result<u64, String> {
    let raw: [u8; 8] = bytes.get(*at..*at + 8).ok_or("truncated test producer report")?.try_into().unwrap();
    *at += 8;
    Ok(u64::from_be_bytes(raw))
}

fn read_string(bytes: &[u8], at: &mut usize) -> Result<String, String> {
    let len = read_u64(bytes, at)? as usize;
    if len > 1024 * 1024 {
        return Err("oversized test producer field".into());
    }
    let raw = bytes.get(*at..*at + len).ok_or("truncated test producer report")?;
    *at += len;
    String::from_utf8(raw.to_vec()).map_err(|_| "non-UTF-8 test producer report".into())
}

#[cfg(test)]
mod protocol_tests {
    use super::read_test_report_bytes;

    #[test]
    fn hostile_protocol_shapes_fail_closed() {
        assert!(read_test_report_bytes(b"BADMAGIC").is_err());
        assert!(read_test_report_bytes(b"JETTEST2\0").is_err());

        let mut oversized = b"JETTEST2".to_vec();
        oversized.extend_from_slice(&[0, 0]);
        oversized.extend_from_slice(&0u64.to_be_bytes());
        oversized.extend_from_slice(&(1024u64 * 1024 + 1).to_be_bytes());
        assert!(read_test_report_bytes(&oversized).is_err());

        let mut trailing = b"JETTEST2".to_vec();
        trailing.push(0);
        assert!(read_test_report_bytes(&trailing).is_err());
    }
}

#[cfg(test)]
mod supervision_tests {
    use super::{supervise_child, ChildOutcome};
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::Duration;

    #[test]
    #[ignore]
    fn child_helper() {
        match std::env::var("JET_PROVE_CHILD_MODE").as_deref() {
            Ok("exit70") => std::process::exit(70),
            Ok("crash") => std::process::abort(),
            Ok("timeout") => {
                std::thread::sleep(Duration::from_millis(300));
                if let Some(path) = std::env::var_os("JET_PROVE_CHILD_MARKER") {
                    fs::write(path, b"orphan survived").unwrap();
                }
            }
            _ => {}
        }
    }

    fn helper(mode: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "CmdProve::supervision_tests::child_helper",
                "--ignored",
            ])
            .env("JET_PROVE_CHILD_MODE", mode);
        command
    }

    #[test]
    fn classifies_exit_crash_timeout_and_unavailable_without_orphan() {
        assert!(matches!(
            supervise_child(&mut helper("exit70"), Duration::from_secs(2)),
            ChildOutcome::Exited(Some(70))
        ));

        assert!(matches!(
            supervise_child(&mut helper("crash"), Duration::from_secs(2)),
            ChildOutcome::Exited(None) | ChildOutcome::Exited(Some(_))
        ));

        let marker = PathBuf::from(format!(
            "/tmp/jet-prove-supervision-{}-marker",
            std::process::id()
        ));
        let _ = fs::remove_file(&marker);
        let mut timeout = helper("timeout");
        timeout.env("JET_PROVE_CHILD_MARKER", &marker);
        assert!(matches!(
            supervise_child(&mut timeout, Duration::from_millis(20)),
            ChildOutcome::TimedOut
        ));
        std::thread::sleep(Duration::from_millis(350));
        assert!(!marker.exists(), "timed-out child survived kill+wait");

        let mut missing = Command::new(format!(
            "/tmp/jet-prove-missing-executable-{}",
            std::process::id()
        ));
        assert!(matches!(
            supervise_child(&mut missing, Duration::from_millis(20)),
            ChildOutcome::LaunchFailed
        ));
    }
}

fn resolve_target(raw: &str) -> Result<Target, String> {
    let path = Path::new(raw);
    if !path.exists() {
        return Err(format!("can't find proof target `{raw}`"));
    }
    let (kind, mut paths) = if path.is_file() {
        if path.extension().and_then(|ext| ext.to_str()) != Some(jet::Syntax::FILE_EXT) {
            return Err(format!("proof target `{raw}` is not a .jet file"));
        }
        ("file", vec![path.to_path_buf()])
    } else {
        let kind = if path.join("pkg.jet").is_file() { "package" } else { "workspace" };
        let mut found = Vec::new();
        collect_jet_files(path, &mut found)?;
        if found.is_empty() {
            return Err(format!("proof target `{raw}` contains no .jet files"));
        }
        (kind, found)
    };
    paths.sort_by(|a, b| normalized(a).as_bytes().cmp(normalized(b).as_bytes()));
    let mut members = Vec::with_capacity(paths.len());
    for path in paths {
        let bytes = fs::read(&path).map_err(|e| format!("couldn't read `{}`: {e}", path.display()))?;
        members.push(Member {
            path: normalized(&path),
            sha256: jet::SHA256::sha256_hex(&bytes),
            bytes,
        });
    }
    let mut identity = Vec::new();
    for member in &members {
        identity.extend_from_slice(
            format!("{{\"path\":{},\"sha256\":{}}}\n", json(&member.path), json(&member.sha256))
                .as_bytes(),
        );
    }
    Ok(Target {
        kind,
        root: normalized(path),
        input_sha256: jet::SHA256::sha256_hex(&identity),
        members,
    })
}

fn collect_jet_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("couldn't read `{}`: {e}", dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| format!("couldn't inspect `{}`: {e}", dir.display()))?.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "build" || name.starts_with('.') {
                continue;
            }
            collect_jet_files(&path, out)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some(jet::Syntax::FILE_EXT) {
            out.push(path);
        }
    }
    Ok(())
}

fn normalized(path: &Path) -> String {
    let path = if path.is_absolute() {
        path.strip_prefix(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
            .unwrap_or(path)
    } else {
        path
    };
    path.components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn budget_projection(target: &Target) -> jet::BudgetView::BudgetProjection {
    let target_path = Path::new(&target.root);
    let search = if target_path.is_file() { target_path.parent().unwrap_or(Path::new(".")) } else { target_path };
    let root = jet::Loader::find_manifest_root(search).unwrap_or_else(|| search.to_path_buf());
    let sources = target.members.iter().map(|member| {
        let path = Path::new(&member.path);
        let path = path.strip_prefix(&root).unwrap_or(path).to_string_lossy().replace('\\', "/");
        (path.trim_start_matches("./").to_string(), member.sha256.clone())
    }).collect::<Vec<_>>();
    jet::BudgetView::read_compatible(&root, &sources)
}

fn diagnostic_span(source: &str, diagnostic: &Diagnostic) -> String {
    let Some(span) = diagnostic.span else { return "0:0-0:0".into() };
    let (sl, sc) = span_line_col(source, span.start);
    let (el, ec) = span_line_col(source, span.end);
    format!("{sl}:{sc}-{el}:{ec}")
}

fn evidence_id(target: &Target, kind: &str, origin: &str, span: &str, claim: &str) -> String {
    let claim_sha = jet::SHA256::sha256_hex(claim.as_bytes());
    let mut preimage = Vec::new();
    for field in [&target.input_sha256, kind, origin, span, &claim_sha] {
        preimage.extend_from_slice(&(field.len() as u64).to_be_bytes());
        preimage.extend_from_slice(field.as_bytes());
    }
    jet::SHA256::sha256_hex(&preimage)
}

fn render_report(
    target: &Target,
    items: &[FrontEndItem],
    tests: &[TestItem],
    budgets: &jet::BudgetView::BudgetProjection,
    solver: &[crate::ProveSolver::SolverEvidence],
    _lenses: &[String],
    proved: usize,
    failed: usize,
    exit_code: i32,
) -> String {
    let members = target.members.iter().map(|m| format!("{{\"path\":{},\"sha256\":{}}}", json(&m.path), json(&m.sha256))).collect::<Vec<_>>().join(",");
    let mut diagnostics = items.iter().filter_map(|item| item.diagnostic.as_ref().map(|d| diagnostic_json(&item.path, &item.source, d))).collect::<Vec<_>>();
    let mut diagnostic_index = 0usize;
    let mut evidence_rows = items.iter().map(|item| {
        let (outcome, indexes) = if item.diagnostic.is_some() { let i = diagnostic_index; diagnostic_index += 1; ("failed", format!("[{i}]")) } else { ("proved", "[]".into()) };
        format!("{{\"attachment\":null,\"budget\":null,\"contract\":null,\"count\":1,\"diagnosticIndexes\":{indexes},\"facet\":\"all\",\"id\":{},\"kind\":\"front_end\",\"outcome\":\"{outcome}\",\"producer\":\"jet-sema\",\"property\":null,\"reason\":null,\"solver\":null,\"source\":{{\"column\":1,\"line\":1,\"path\":{}}},\"state\":\"checked\"}}", json(&item.id), json(&item.path))
    }).collect::<Vec<_>>();
    for item in tests {
        let (kind, facet, producer) = match item.kind {
            1 => ("contract", "contracts", "jet-runtime"),
            3 => ("property", "tests", "jet-property"),
            4 => ("doctest", "tests", "jet-doctest"),
            _ => ("unit", "tests", "jet-test"),
        };
        let (state, outcome, reason) = match item.state { 0 => ("executed", "passed", "null"), 1 => ("executed", "failed", "null"), 2 => ("skipped", "not_run", "\"fail_fast_policy\""), _ => ("unavailable", "unavailable", "\"producer_start_failed\"") };
        let diagnostic_indexes = if item.kind == 2 || (item.kind == 1 && item.state == 1) {
            let index = diagnostics.len();
            let code = if item.kind == 1 { "E3005" } else { "E3001" };
            diagnostics.push(format!("{{\"caret\":null,\"code\":{},\"context\":[],\"frames\":[],\"message\":{},\"notes\":[],\"origin\":{{\"producer\":\"jet-runtime\",\"stage\":\"runtime\"}},\"safeLocals\":[],\"severity\":\"error\",\"span\":{{\"endColumn\":1,\"endLine\":{},\"path\":{},\"sourceLine\":null,\"startColumn\":1,\"startLine\":{}}},\"type\":\"runtime\"}}", json(code), json(&item.message), item.line, json(&item.path), item.line));
            format!("[{index}]")
        } else { "[]".into() };
        if item.kind == 2 { continue; }
        let contract = if item.kind == 1 { format!("{{\"marker\":{},\"observation\":\"{}\",\"site\":{{\"column\":1,\"line\":{},\"path\":{}}}}}", json(&item.name), if item.state == 0 { "reached_pass" } else { "reached_fail" }, item.line, json(&item.path)) } else { "null".into() };
        let property = if item.kind == 3 { format!("{{\"caseIndex\":{},\"effectiveSeed\":{},\"generatedCases\":{},\"shrinkTrace\":{},\"source\":{{\"column\":1,\"line\":1,\"path\":{}}},\"toolchain\":{{\"jet\":{},\"targetTriple\":{}}}}}", item.line.saturating_sub(1), item.seed.parse::<u64>().unwrap_or(0), item.line, if item.message.is_empty() { "[]".into() } else { format!("[{{\"name\":\"minimized_inputs\",\"value\":{}}}]", json(&item.message)) }, json(&item.path), json(env!("CARGO_PKG_VERSION")), json(std::env::consts::ARCH)) } else { "null".into() };
        evidence_rows.push(format!("{{\"attachment\":null,\"budget\":null,\"contract\":{contract},\"count\":1,\"diagnosticIndexes\":{diagnostic_indexes},\"facet\":\"{facet}\",\"id\":{},\"kind\":\"{kind}\",\"outcome\":\"{outcome}\",\"producer\":\"{producer}\",\"property\":{property},\"reason\":{reason},\"solver\":null,\"source\":{{\"column\":1,\"line\":{},\"path\":{}}},\"state\":\"{state}\"}}", json(&item.id), item.line.max(1), json(&item.path)));
    }
    for fact in &budgets.facts {
        let kind = if fact.statistical { "statistical_budget" } else { "deterministic_budget" };
        let facet = "budgets";
        let outcome = match fact.outcome.as_str() { "pass" => "met", "warn" => "warning", _ => "failed" };
        let budget = format!("{{\"budgetId\":{},\"enforcement\":{},\"evidenceId\":{},\"reportId\":{},\"statistical\":{}}}", json(&fact.budget_id), json(&fact.enforcement), json(&fact.evidence_id), json(&fact.report_id), fact.statistical);
        evidence_rows.push(format!("{{\"attachment\":null,\"budget\":{budget},\"contract\":null,\"count\":1,\"diagnosticIndexes\":[],\"facet\":\"{facet}\",\"id\":{},\"kind\":\"{kind}\",\"outcome\":\"{outcome}\",\"producer\":\"jet-budget\",\"property\":null,\"reason\":null,\"solver\":null,\"source\":{{\"column\":1,\"line\":1,\"path\":{}}},\"state\":\"checked\"}}", json(&fact.evidence_id), json(&target.root)));
    }
    for item in solver {
        evidence_rows.push(crate::ProveSolver::evidence_json(item));
    }
    let evidence = evidence_rows.join(",");
    let (solver_selected, solver_proved, solver_disproved, solver_unknown, solver_unavailable) =
        crate::ProveSolver::summarize(solver);
    let unit_passed = tests.iter().filter(|item| item.kind == 0 && item.state == 0).count();
    let unit_failed = tests.iter().filter(|item| item.kind == 0 && item.state == 1).count();
    let unit_skipped = tests.iter().filter(|item| item.kind == 0 && item.state == 2).count();
    let unit_unavailable = tests.iter().filter(|item| item.kind == 0 && item.state == 3).count();
    let contract_passed = tests.iter().filter(|item| item.kind == 1 && item.state == 0).count();
    let contract_failed = tests.iter().filter(|item| item.kind == 1 && item.state == 1).count();
    let contract_selected = contract_passed + contract_failed;
    let property_passed = tests.iter().filter(|item| item.kind == 3 && item.state == 0).count();
    let property_failed = tests.iter().filter(|item| item.kind == 3 && item.state == 1).count();
    let property_cases: u32 = tests.iter().filter(|item| item.kind == 3).map(|item| item.line).sum();
    let doctest_passed = tests.iter().filter(|item| item.kind == 4 && item.state == 0).count();
    let doctest_failed = tests.iter().filter(|item| item.kind == 4 && item.state == 1).count();
    let unit_selected = unit_passed + unit_failed + unit_skipped;
    let deterministic_selected = budgets.facts.iter().filter(|fact| !fact.statistical).count();
    let deterministic_failed = budgets.facts.iter().filter(|fact| !fact.statistical && fact.outcome == "fail").count();
    let deterministic_met = budgets.facts.iter().filter(|fact| !fact.statistical && fact.outcome == "pass").count();
    let deterministic_unavailable = budgets.facts.iter().filter(|fact| !fact.statistical && fact.evidence == "unavailable").count();
    let statistical_selected = budgets.facts.iter().filter(|fact| fact.statistical).count();
    let statistical_failed = budgets.facts.iter().filter(|fact| fact.statistical && fact.outcome == "fail").count();
    let statistical_met = budgets.facts.iter().filter(|fact| fact.statistical && fact.outcome == "pass").count();
    let statistical_unavailable = budgets.facts.iter().filter(|fact| fact.statistical && fact.evidence == "unavailable").count();
    format!("{{\"diagnostics\":[{}],\"evidence\":[{evidence}],\"evidencePolicy\":\"allow_incomplete\",\"exitCode\":{exit_code},\"result\":\"{}\",\"schemaVersion\":1,\"summaries\":{{\"contract\":{{\"declared\":0,\"failed\":0,\"notObserved\":0,\"observed\":0,\"passed\":0,\"selected\":0,\"skipped\":0}},\"deterministicBudget\":{{\"failed\":0,\"met\":0,\"selected\":0,\"skipped\":0,\"unavailable\":0}},\"doctest\":{{\"failed\":0,\"passed\":0,\"selected\":0,\"skipped\":0}},\"frontEnd\":{{\"failed\":{failed},\"proved\":{proved},\"selected\":{},\"skipped\":0}},\"property\":{{\"failed\":0,\"generatedCases\":0,\"passed\":0,\"selected\":0,\"shrunkFailures\":0,\"skipped\":0}},\"solver\":{{\"disproved\":0,\"proved\":0,\"selected\":0,\"unavailable\":0,\"unknown\":0}},\"statisticalBudget\":{{\"failed\":0,\"met\":0,\"selected\":0,\"skipped\":0,\"unavailable\":0}},\"unit\":{{\"failed\":{unit_failed},\"passed\":{unit_passed},\"selected\":{unit_selected},\"skipped\":{unit_skipped}}}}},\"target\":{{\"inputSha256\":{},\"kind\":\"{}\",\"members\":[{members}],\"root\":{}}},\"tool\":{{\"jet\":{},\"proofProducer\":\"jet-prove\",\"targetTriple\":{}}}}}", diagnostics.join(","), if failed == 0 && unit_failed == 0 { if unit_unavailable > 0 { "pass_incomplete" } else { "pass" } } else { "fail" }, items.len(), json(&target.input_sha256), target.kind, json(&target.root), json(env!("CARGO_PKG_VERSION")), json(std::env::consts::ARCH))
    .replace(
        "\"deterministicBudget\":{\"failed\":0,\"met\":0,\"selected\":0,\"skipped\":0,\"unavailable\":0}",
        &format!("\"deterministicBudget\":{{\"failed\":{deterministic_failed},\"met\":{deterministic_met},\"selected\":{deterministic_selected},\"skipped\":0,\"unavailable\":{deterministic_unavailable}}}"),
    )
    .replace(
        "\"statisticalBudget\":{\"failed\":0,\"met\":0,\"selected\":0,\"skipped\":0,\"unavailable\":0}",
        &format!("\"statisticalBudget\":{{\"failed\":{statistical_failed},\"met\":{statistical_met},\"selected\":{statistical_selected},\"skipped\":0,\"unavailable\":{statistical_unavailable}}}"),
    )
    .replace(
        "\"contract\":{\"declared\":0,\"failed\":0,\"notObserved\":0,\"observed\":0,\"passed\":0,\"selected\":0,\"skipped\":0}",
        &format!("\"contract\":{{\"declared\":{contract_selected},\"failed\":{contract_failed},\"notObserved\":0,\"observed\":{contract_selected},\"passed\":{contract_passed},\"selected\":{contract_selected},\"skipped\":0}}"),
    )
    .replace(
        "\"property\":{\"failed\":0,\"generatedCases\":0,\"passed\":0,\"selected\":0,\"shrunkFailures\":0,\"skipped\":0}",
        &format!("\"property\":{{\"failed\":{property_failed},\"generatedCases\":{property_cases},\"passed\":{property_passed},\"selected\":{},\"shrunkFailures\":{property_failed},\"skipped\":0}}", property_passed + property_failed),
    )
    .replace(
        "\"doctest\":{\"failed\":0,\"passed\":0,\"selected\":0,\"skipped\":0}",
        &format!("\"doctest\":{{\"failed\":{doctest_failed},\"passed\":{doctest_passed},\"selected\":{},\"skipped\":0}}", doctest_passed + doctest_failed),
    )
    .replace(
        "\"solver\":{\"disproved\":0,\"proved\":0,\"selected\":0,\"unavailable\":0,\"unknown\":0}",
        &format!(
            "\"solver\":{{\"disproved\":{solver_disproved},\"proved\":{solver_proved},\"selected\":{solver_selected},\"unavailable\":{solver_unavailable},\"unknown\":{solver_unknown}}}"
        ),
    )
    .replace(
        "\"result\":\"pass\"",
        if exit_code != ExitCodes::OK || contract_failed > 0 {
            "\"result\":\"fail\""
        } else {
            "\"result\":\"pass\""
        },
    )
}

fn diagnostic_json(path: &str, source: &str, d: &Diagnostic) -> String {
    let severity = if matches!(d.severity, Severity::Error) { "error" } else { "warning" };
    let span = d.span.map(|s| { let (sl, sc) = span_line_col(source, s.start); let (el, ec) = span_line_col(source, s.end); format!("{{\"endColumn\":{ec},\"endLine\":{el},\"path\":{},\"sourceLine\":null,\"startColumn\":{sc},\"startLine\":{sl}}}", json(path)) }).unwrap_or_else(|| "null".into());
    format!("{{\"caret\":null,\"code\":{},\"context\":[],\"frames\":[],\"message\":{},\"notes\":[{},{}],\"origin\":{{\"producer\":\"jet-sema\",\"stage\":\"front_end\"}},\"safeLocals\":[],\"severity\":\"{severity}\",\"span\":{span},\"type\":\"front_end\"}}", json(&d.code), json(&d.what), json(&format!("Why: {}", d.why)), json(&format!("Fix: {}", d.fix)))
}

fn json(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch { '\\' => out.push_str("\\\\"), '"' => out.push_str("\\\""), '\n' => out.push_str("\\n"), '\r' => out.push_str("\\r"), '\t' => out.push_str("\\t"), c if c < ' ' => out.push_str(&format!("\\u{:04x}", c as u32)), c => out.push(c) }
    }
    out.push('"');
    out
}

/// D-JPROOF1=A: write `.jet/proofs/<kind>/<name>/<first-16-report_id>.jetproof`.
/// Identical existing bytes are left unchanged; differing bytes refuse.
fn write_jetproof(target: &Target, proof_report: &str) -> Result<(), String> {
    let report_id = jet::SHA256::sha256_hex(proof_report.as_bytes());
    let kind = target.kind;
    let name = {
        let root = Path::new(&target.root);
        let stem = root
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(kind)
            .replace('.', "_");
        if stem.is_empty() { kind.to_string() } else { stem }
    };
    let rel = format!(
        ".jet/proofs/{kind}/{name}/{}.jetproof",
        &report_id[..16.min(report_id.len())]
    );
    let path = PathBuf::from(&rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let envelope = format!(
        "{{\"schema\":\"jet.jproof\",\"version\":1,\"report_id\":{},\"artifact\":{{\"path\":{}}},\"privacy\":{{\"absolute_paths\":\"omitted\",\"argv\":\"omitted\",\"environment\":\"omitted\",\"full_source\":\"omitted\",\"producer_transcripts\":\"omitted\",\"safe_locals\":\"redacted_by_D-OBS2\"}},\"proofReport\":{proof_report}}}\n",
        json(&report_id),
        json(&rel)
    );
    if path.exists() {
        let existing = fs::read(&path).map_err(|e| e.to_string())?;
        if existing == envelope.as_bytes() {
            return Ok(());
        }
        return Err(format!(
            "refusing to overwrite differing .jetproof at {rel}"
        ));
    }
    let tmp = path.with_extension(format!(
        "jetproof.tmp.{}.{}",
        std::process::id(),
        &report_id[..8]
    ));
    fs::write(&tmp, envelope.as_bytes()).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(())
}
