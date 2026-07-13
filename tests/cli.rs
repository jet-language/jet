//! E2-M3 Wave A — developer command UX golden tests.
//!
//! Pins:
//!   - the stable exit-code table (0/1/2/70/101);
//!   - human *and* `--json` diagnostic output for check/build/test;
//!   - CI determinism: output is byte-identical and ANSI-free under `NO_COLOR`
//!     and when piped (not a TTY);
//!   - `jet explain <CODE>` resolves for EVERY registered diagnostic code
//!     (closing the I4 loop: no code without an explain).
//!
//! Snapshots live in `tests/cli/*.txt`; bless with `UPDATE_EXPECT=1`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

fn jet() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

fn cli_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/cli")
}

/// Compare `actual` against `tests/cli/<name>`; bless on `UPDATE_EXPECT=1`.
fn check_snapshot(name: &str, actual: &str) {
    let path = cli_dir().join(name);
    if std::env::var("UPDATE_EXPECT").is_ok() {
        fs::create_dir_all(cli_dir()).unwrap();
        fs::write(&path, actual).unwrap();
        return;
    }
    let expected = fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "missing snapshot {}; run UPDATE_EXPECT=1 cargo test",
            path.display()
        )
    });
    assert_eq!(actual, expected, "snapshot mismatch for {}", name);
}

/// Write a tiny source file with a known error and return its path. Each test
/// passes a unique `tag` so concurrent tests never share a path — `fs::write`
/// truncates-then-writes, so a shared path would let one test's write race a
/// sibling's `jet check` read (seeing a momentarily-empty file).
fn bad_file(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("jet_cli_bad_{tag}.jet"));
    fs::write(&p, "fn run() {\n    pirnt(\"hi\");\n}\n").unwrap();
    p
}

/// Replace machine-specific temp paths so snapshots are portable.
fn scrub(s: &str, file: &Path) -> String {
    s.replace(&file.display().to_string(), "BAD.jet")
}

/// A private cwd for a `jet run`/`build`/`bench`/`test` subprocess.
///
/// `jet` writes compiled output to `build/<stem>.rs` + `build/<stem>` *relative
/// to its own cwd* (Source/CmdCompile.rs `bin_path`/`stem`/`build`), keyed only
/// by the source file's stem — not its full path. Two concurrent `jet`
/// processes compiling different files that happen to share a stem (e.g. two
/// `main.jet` fixtures) race on that shared `build/` path if both inherit the
/// test harness's cwd (the repo root). Giving each such test its own cwd
/// removes the shared namespace entirely, regardless of stem.
fn isolated_cwd(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("jet_cli_cwd_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn budget_project(tag: &str, limit: u64) -> PathBuf {
    let dir = isolated_cwd(tag);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("pkg.jet"),
        "payload: { name: \"app\", version: \"0.1.0\" }\n",
    ).unwrap();
    fs::write(
        dir.join("src/main.jet"),
        format!(r#"module perf.package {{
    budgets: [Budget.{{
        name: "public-api",
        scope: .Package,
        metric: .PublicApiItems,
        comparison: .Absolute,
        limit: .AtMost({limit}),
    }}],
}}
pub fn api() {{}}
fn run() {{}}
"#),
    ).unwrap();
    dir
}

fn artifact_budget_project(tag: &str, limit: u64) -> PathBuf {
    let dir = isolated_cwd(tag);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("pkg.jet"),
        "payload: { name: \"app\", version: \"0.1.0\" }\n",
    ).unwrap();
    fs::write(
        dir.join("src/main.jet"),
        format!(r#"module perf.package {{
    budgets: [Budget.{{
        name: "binary",
        scope: .Package,
        metric: .BinarySize,
        comparison: .Absolute,
        limit: .AtMost({limit}B),
    }}],
}}
fn run() {{
    print("tiny")
}}
"#),
    ).unwrap();
    dir
}

fn mixed_budget_project(tag: &str) -> PathBuf {
    let dir = isolated_cwd(tag);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("pkg.jet"),
        "payload: { name: \"app\", version: \"0.1.0\" }\n",
    ).unwrap();
    fs::write(
        dir.join("src/main.jet"),
        r#"module perf.package {
    budgets: [
        Budget.{
            name: "binary",
            scope: .Package,
            metric: .BinarySize,
            comparison: .Absolute,
            limit: .AtMost(100000000B),
        },
        Budget.{
            name: "public-api",
            scope: .Package,
            metric: .PublicApiItems,
            comparison: .Absolute,
            limit: .AtMost(10),
        },
    ],
}

pub fn api() {}
fn run() {}
"#,
    ).unwrap();
    dir
}

fn benchmark_budget_project(tag: &str) -> PathBuf {
    let dir = isolated_cwd(tag);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("pkg.jet"), "payload: { name: \"app\", version: \"0.1.0\" }\n").unwrap();
    fs::write(dir.join("src/main.jet"), r#"module perf.package {
    budgets: [Budget.{
        name: "parse",
        scope: .Bench("parse"),
        metric: .BenchTime(.P50),
        provider: .BenchMeasurement("parse"),
        comparison: .RelativeTo("ci/linux"),
        limit: .RegressionAtMost(100pct),
        enforcement: .Warn,
    }],
}
#Bench("parse") {
    total := 0
    loop value in 0..100 { total = total + value }
    require_eq(total, 4950)
}
fn run() {}
"#).unwrap();
    dir
}

#[test]
fn budget_usage_and_preflight_fail_without_artifacts() {
    let dir = budget_project("budget_no_artifact", 10);
    for argv in [
        vec!["budget", "check", "--unknown"],
        vec!["budget", "update", "--baseline", "ci/linux", "--reason", "no gate"],
        vec!["budget", "report"],
        vec!["budget", "check", "--json", "--unknown"],
        vec!["budget", "check", "--unknown", "--json"],
        vec!["budget", "check", "--json", "--json"],
        vec!["budget", "check", "--annotations", "gitlab"],
        vec!["budget", "update", "--baseline", "CI/Linux"],
        vec!["budget", "update", "--baseline", "ci/linux", "--bootstrap", "--accept-regression", "--reason", "invalid"],
        vec!["budget", "update", "--baseline", "ci/linux", "--yes", "-y"],
    ] {
        let out = Command::new(jet()).args(argv).current_dir(&dir).output().unwrap();
        assert_eq!(out.status.code(), Some(2));
        assert!(out.stdout.is_empty());
        assert!(!dir.join(".jet").exists(), "usage failure created an artifact");
    }
    fs::write(dir.join("src/main.jet"), "fn run( {\n").unwrap();
    let out = Command::new(jet()).args(["budget", "check"]).current_dir(&dir).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(!dir.join(".jet").exists(), "compiler preflight created an artifact");
}

#[test]
fn budget_check_uses_real_compiler_fact_and_writes_verified_report() {
    let dir = budget_project("budget_check", 10);
    let out = Command::new(jet()).args(["budget", "check", "--json"]).current_dir(&dir).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(out.stderr.is_empty());
    let value = jet_foundation::PerformanceBudget::CanonicalJson::parse_canonical(&out.stdout).unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("\"schema\":\"jet.budget-command\""));
    assert!(text.contains("\"budget_id\":\"package:public-api\""));
    assert!(text.contains("\"num\":1"), "public API count must be measured: {text}");
    let reports = fs::read_dir(dir.join(".jet/perf/reports")).unwrap().collect::<Result<Vec<_>,_>>().unwrap();
    assert_eq!(reports.len(), 1);
    let bytes = fs::read(reports[0].path()).unwrap();
    jet_foundation::PerformanceBudget::verify_budget_report(&bytes).unwrap();
    let command = match &value { jet_foundation::PerformanceBudget::CanonicalJson::Object(value) => value, _ => panic!("command JSON is not an object") };
    let report = match &command["report"] { jet_foundation::PerformanceBudget::CanonicalJson::Object(value) => value, _ => panic!("report is not an object") };
    let content = match &report["content"] { jet_foundation::PerformanceBudget::CanonicalJson::Object(value) => value, _ => panic!("content is not an object") };
    let tool = match &content["toolchain"] { jet_foundation::PerformanceBudget::CanonicalJson::Object(value) => value, _ => panic!("toolchain is not an object") };
    for key in ["compiler_build_id", "stdlib_id", "runner_id"] {
        let jet_foundation::PerformanceBudget::CanonicalJson::String(id) = &tool[key] else { panic!("{key} is not text") };
        assert_eq!(id.len(), 64, "{key} must identify real executable bytes");
        assert!(id.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert!(!matches!(id.as_str(), "jet" | "stdlib" | "compiler"));
    }
    let subject = match &content["subject"] { jet_foundation::PerformanceBudget::CanonicalJson::Object(value) => value, _ => panic!("subject is not an object") };
    let jet_foundation::PerformanceBudget::CanonicalJson::String(triple) = &subject["target_triple"] else { panic!("target triple is not text") };
    assert!(triple.split('-').count() >= 3, "target triple must be canonical: {triple}");
    let jet_foundation::PerformanceBudget::CanonicalJson::String(measured_start) = &subject["measured_start"] else { panic!("measurement start is not text") };
    let jet_foundation::PerformanceBudget::CanonicalJson::String(measured_end) = &subject["measured_end"] else { panic!("measurement end is not text") };
    assert!(measured_start < measured_end, "measurement must cover preflight and evidence: {measured_start}..{measured_end}");
    let measurements = match &content["measurements"] { jet_foundation::PerformanceBudget::CanonicalJson::Array(value) => value, _ => panic!("measurements is not an array") };
    let measurement = match &measurements[0] { jet_foundation::PerformanceBudget::CanonicalJson::Object(value) => value, _ => panic!("measurement is not an object") };
    let provider = match &measurement["provider"] { jet_foundation::PerformanceBudget::CanonicalJson::Object(value) => value, _ => panic!("provider is not an object") };
    for key in ["cpu_model", "kernel", "power_governor"] {
        let jet_foundation::PerformanceBudget::CanonicalJson::String(value) = &provider[key] else { panic!("{key} is not text") };
        assert!(!value.is_empty() && !matches!(value.as_str(), "compiler" | "unknown"));
    }
}

#[test]
fn budget_build_artifact_measures_real_selected_binary() {
    use jet_foundation::PerformanceBudget::CanonicalJson;
    let dir = artifact_budget_project("budget_build_artifact", 100_000_000);
    let out = Command::new(jet()).args(["budget", "check", "--json"]).current_dir(&dir).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "stdout: {}\nstderr: {}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.stderr.is_empty());
    let CanonicalJson::Object(command) = CanonicalJson::parse_canonical(&out.stdout).unwrap() else { panic!("command object") };
    let CanonicalJson::Object(report) = &command["report"] else { panic!("report object") };
    let CanonicalJson::Object(content) = &report["content"] else { panic!("content object") };
    let CanonicalJson::Object(subject) = &content["subject"] else { panic!("subject object") };
    let CanonicalJson::Object(artifact) = &subject["artifact"] else { panic!("artifact identity") };
    let CanonicalJson::Integer(bytes) = &artifact["bytes"] else { panic!("artifact byte count") };
    let CanonicalJson::String(digest) = &artifact["sha256"] else { panic!("artifact digest") };
    let artifact_path = dir.join("build/main");
    let metadata = fs::metadata(&artifact_path).unwrap();
    assert_eq!(bytes, &metadata.len().to_string());
    assert_eq!(digest, &jet::SHA256::sha256_file_hex(&artifact_path).unwrap());
    let CanonicalJson::Array(measurements) = &content["measurements"] else { panic!("measurements") };
    let CanonicalJson::Object(measurement) = &measurements[0] else { panic!("measurement") };
    let CanonicalJson::Array(samples) = &measurement["samples"] else { panic!("samples") };
    let CanonicalJson::Object(sample) = &samples[0] else { panic!("sample") };
    assert_eq!(sample["num"], CanonicalJson::Integer(metadata.len().to_string()));
    let CanonicalJson::Object(provider) = &measurement["provider"] else { panic!("provider") };
    assert_eq!(provider["kind"], CanonicalJson::String("BuildArtifact".into()));
    assert_eq!(measurement["unit"], CanonicalJson::String("Bytes".into()));
}

#[test]
fn budget_report_collects_mixed_providers_measurement_locally() {
    use jet_foundation::PerformanceBudget::CanonicalJson;
    let dir = mixed_budget_project("budget_mixed_providers");
    let out = Command::new(jet()).args(["budget", "check", "--json"]).current_dir(&dir).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "stdout: {}\nstderr: {}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.stderr.is_empty());
    let CanonicalJson::Object(command) = CanonicalJson::parse_canonical(&out.stdout).unwrap() else { panic!("command object") };
    let CanonicalJson::Object(report) = &command["report"] else { panic!("report object") };
    let CanonicalJson::Object(content) = &report["content"] else { panic!("content object") };
    let CanonicalJson::Array(measurements) = &content["measurements"] else { panic!("measurements") };
    assert_eq!(measurements.len(), 2);
    let mut providers = std::collections::BTreeMap::new();
    for measurement in measurements {
        let CanonicalJson::Object(measurement) = measurement else { panic!("measurement object") };
        let CanonicalJson::String(id) = &measurement["budget_id"] else { panic!("budget id") };
        let CanonicalJson::Object(provider) = &measurement["provider"] else { panic!("provider") };
        let CanonicalJson::String(kind) = &provider["kind"] else { panic!("provider kind") };
        let CanonicalJson::Array(samples) = &measurement["samples"] else { panic!("samples") };
        assert_eq!(samples.len(), 1, "{id} must own its provider sample");
        providers.insert(id.clone(), kind.clone());
    }
    assert_eq!(providers.get("package:binary").map(String::as_str), Some("BuildArtifact"));
    assert_eq!(providers.get("package:public-api").map(String::as_str), Some("CompilerFacts"));
    let CanonicalJson::Object(subject) = &content["subject"] else { panic!("subject") };
    let CanonicalJson::Object(artifact) = &subject["artifact"] else { panic!("shared artifact provenance") };
    let CanonicalJson::Integer(bytes) = &artifact["bytes"] else { panic!("artifact bytes") };
    assert_eq!(bytes, &fs::metadata(dir.join("build/main")).unwrap().len().to_string());
    let report_path = fs::read_dir(dir.join(".jet/perf/reports")).unwrap().next().unwrap().unwrap().path();
    jet_foundation::PerformanceBudget::verify_budget_report(&fs::read(report_path).unwrap()).unwrap();
}

#[test]
fn build_enforces_deterministic_fail_budgets_and_reuses_relevant_identity() {
    use jet_foundation::PerformanceBudget::CanonicalJson;
    let dir = mixed_budget_project("build_budget_gates");
    let source_path = dir.join("src/main.jet");
    let passing = fs::read_to_string(&source_path).unwrap();
    let failing = passing.replace(".AtMost(10)", ".AtMost(0)");
    fs::write(&source_path, &failing).unwrap();

    let failed = Command::new(jet()).args(["build", "src/main.jet"]).current_dir(&dir).output().unwrap();
    assert_eq!(failed.status.code(), Some(1), "stdout: {}\nstderr: {}", String::from_utf8_lossy(&failed.stdout), String::from_utf8_lossy(&failed.stderr));
    assert!(!String::from_utf8_lossy(&failed.stdout).contains("built:"), "failed budget claimed build success");
    assert!(String::from_utf8_lossy(&failed.stderr).contains("Error [E2907]: performance budget public-api regressed"), "{}", String::from_utf8_lossy(&failed.stderr));
    let report_dir = dir.join(".jet/perf/reports");
    assert_eq!(fs::read_dir(&report_dir).unwrap().count(), 1);

    fs::write(&source_path, &passing).unwrap();
    let passed = Command::new(jet()).args(["build", "src/main.jet"]).current_dir(&dir).output().unwrap();
    assert_eq!(passed.status.code(), Some(0), "stdout: {}\nstderr: {}", String::from_utf8_lossy(&passed.stdout), String::from_utf8_lossy(&passed.stderr));
    assert!(String::from_utf8_lossy(&passed.stderr).contains("budgets: 2 budgets passed · report "));
    assert_eq!(fs::read_dir(&report_dir).unwrap().count(), 2, "source/spec change must refresh evidence");

    let reused = Command::new(jet()).args(["build", "src/main.jet"]).current_dir(&dir).output().unwrap();
    assert_eq!(reused.status.code(), Some(0), "{}", String::from_utf8_lossy(&reused.stderr));
    assert_eq!(fs::read_dir(&report_dir).unwrap().count(), 2, "unchanged relevant identity must reuse canonical report");

    let ci = Command::new(jet()).args(["build", "src/main.jet", "--profile=ci"]).current_dir(&dir).output().unwrap();
    assert_eq!(ci.status.code(), Some(0), "stdout: {}\nstderr: {}", String::from_utf8_lossy(&ci.stdout), String::from_utf8_lossy(&ci.stderr));
    assert_eq!(fs::read_dir(&report_dir).unwrap().count(), 3, "CI profile identity must refresh evidence");
    let mut profiles = Vec::new();
    for entry in fs::read_dir(&report_dir).unwrap() {
        let value = CanonicalJson::parse_canonical(&fs::read(entry.unwrap().path()).unwrap()).unwrap();
        let CanonicalJson::Object(report) = value else { panic!("report") };
        let CanonicalJson::Object(content) = &report["content"] else { panic!("content") };
        let CanonicalJson::Object(subject) = &content["subject"] else { panic!("subject") };
        let CanonicalJson::String(profile) = &subject["profile"] else { panic!("profile") };
        profiles.push(profile.clone());
    }
    assert!(profiles.iter().any(|profile| profile == "dev"));
    assert!(profiles.iter().any(|profile| profile == "ci"));
}

#[test]
fn budget_bench_measurement_bootstraps_then_consumes_compatible_history() {
    use jet_foundation::PerformanceBudget::CanonicalJson;
    let dir = benchmark_budget_project("budget_bench_measurement");
    let bootstrap = Command::new(jet()).args(["budget","update","--baseline","ci/linux","--bootstrap","--reason","initial benchmark","--yes","--json"]).current_dir(&dir).output().unwrap();
    assert_eq!(bootstrap.status.code(),Some(0),"stdout: {}\nstderr: {}",String::from_utf8_lossy(&bootstrap.stdout),String::from_utf8_lossy(&bootstrap.stderr));
    let CanonicalJson::Object(first)=CanonicalJson::parse_canonical(&bootstrap.stdout).unwrap() else{panic!("command")};
    let CanonicalJson::Object(report)=&first["report"] else{panic!("report")};let CanonicalJson::Object(content)=&report["content"] else{panic!("content")};let CanonicalJson::Array(measurements)=&content["measurements"] else{panic!("measurements")};let CanonicalJson::Object(measurement)=&measurements[0] else{panic!("measurement")};
    let CanonicalJson::Array(samples)=&measurement["samples"] else{panic!("samples")};assert_eq!(samples.len(),20);assert!(matches!(measurement["statistics"],CanonicalJson::Object(_)));assert!(matches!(measurement["policy"],CanonicalJson::Object(_)));assert_eq!(measurement["history"],CanonicalJson::Null);assert_eq!(measurement["baseline"],CanonicalJson::Null);
    let CanonicalJson::Object(provider)=&measurement["provider"] else{panic!("provider")};assert_eq!(provider["kind"],CanonicalJson::String("BenchMeasurement".into()));assert_eq!(provider["identity"],CanonicalJson::String("parse".into()));
    let first_id=match &report["report_id"]{CanonicalJson::String(value)=>value.clone(),_=>panic!("report id")};

    let check=Command::new(jet()).args(["budget","check","--json"]).current_dir(&dir).output().unwrap();
    assert!(matches!(check.status.code(),Some(0)|Some(1)),"stdout: {}\nstderr: {}",String::from_utf8_lossy(&check.stdout),String::from_utf8_lossy(&check.stderr));
    let CanonicalJson::Object(second)=CanonicalJson::parse_canonical(&check.stdout).unwrap() else{panic!("command")};
    let CanonicalJson::Object(report)=&second["report"] else{panic!("report")};let CanonicalJson::Object(content)=&report["content"] else{panic!("content")};let CanonicalJson::Array(measurements)=&content["measurements"] else{panic!("measurements")};let CanonicalJson::Object(measurement)=&measurements[0] else{panic!("measurement")};let CanonicalJson::Object(history)=&measurement["history"] else{panic!("history")};let CanonicalJson::Array(ids)=&history["report_ids"] else{panic!("ids")};assert_eq!(ids, &vec![CanonicalJson::String(first_id.clone())]);let CanonicalJson::Object(baseline)=&measurement["baseline"] else{panic!("baseline")};let CanonicalJson::Array(pooled)=&baseline["pooled_samples"] else{panic!("pooled")};assert_eq!(pooled.len(),20);let CanonicalJson::Object(decision)=&measurement["decision"] else{panic!("decision")};assert_ne!(decision["evidence"],CanonicalJson::String("unavailable".into()));
    let CanonicalJson::Array(results)=&second["results"] else{panic!("results")};let CanonicalJson::Object(result)=&results[0] else{panic!("result")};
    assert_eq!(result["baseline_report_ids"],CanonicalJson::Array(vec![CanonicalJson::String(first_id)]));
    assert_eq!(result["metric"],measurement["metric"]);
    assert_eq!(result["lower95"],decision["lower95"]);assert_eq!(result["upper95"],decision["upper95"]);assert_eq!(result["trend"],decision["trend"]);assert_eq!(result["reason"],decision["reason"]);
}

#[test]
fn bench_owns_canonical_refresh_and_dossier_only_projects_it() {
    use jet_foundation::PerformanceBudget::CanonicalJson;
    let dir = benchmark_budget_project("bench_owned_budget_refresh");
    let run = || Command::new(jet()).args(["bench", "src/main.jet"]).current_dir(&dir).output().unwrap();
    let first = run();
    assert_eq!(first.status.code(), Some(0), "stdout: {}\nstderr: {}", String::from_utf8_lossy(&first.stdout), String::from_utf8_lossy(&first.stderr));
    assert!(String::from_utf8_lossy(&first.stderr).contains("report "));
    let reports = dir.join(".jet/perf/reports");
    let report_paths = || fs::read_dir(&reports).unwrap().map(|entry| entry.unwrap().path()).collect::<Vec<_>>();
    let initial = report_paths();
    assert_eq!(initial.len(), 1);
    let bytes = fs::read(&initial[0]).unwrap();
    jet_foundation::PerformanceBudget::verify_budget_report(&bytes).unwrap();
    let CanonicalJson::Object(report) = CanonicalJson::parse_canonical(&bytes).unwrap() else { panic!("report") };
    let CanonicalJson::Object(content) = &report["content"] else { panic!("content") };
    let CanonicalJson::Object(subject) = &content["subject"] else { panic!("subject") };
    assert_eq!(subject["profile"], CanonicalJson::String("bench".into()));

    let second = run();
    assert_eq!(second.status.code(), Some(0));
    assert!(!String::from_utf8_lossy(&second.stdout).contains("ns/iter"), "unchanged relevant identity reran measurement harness");
    assert_eq!(report_paths(), initial, "unchanged relevant identity must reuse report");

    let before = fs::read_dir(&reports).unwrap().map(|entry| { let path=entry.unwrap().path();(path.clone(),fs::metadata(&path).unwrap().modified().unwrap()) }).collect::<Vec<_>>();
    let dossier = Command::new(jet()).args(["inspect", "dossier", "src/main.jet", "run", "--json"]).current_dir(&dir).output().unwrap();
    assert_eq!(dossier.status.code(), Some(0), "{}", String::from_utf8_lossy(&dossier.stderr));
    let dossier = String::from_utf8(dossier.stdout).unwrap();
    assert!(dossier.contains("\"performance_budgets\":{\"mode\":\"read_only\""), "{dossier}");
    assert!(dossier.contains("\"budget_id\":\"package:parse\""), "{dossier}");
    let after = fs::read_dir(&reports).unwrap().map(|entry| { let path=entry.unwrap().path();(path.clone(),fs::metadata(&path).unwrap().modified().unwrap()) }).collect::<Vec<_>>();
    assert_eq!(before, after, "dossier projection must not rewrite reports");

    fs::OpenOptions::new().append(true).open(dir.join("src/main.jet")).unwrap().write_all(b"\n// relevant source digest change\n").unwrap();
    let third = run();
    assert_eq!(third.status.code(), Some(0), "{}", String::from_utf8_lossy(&third.stderr));
    assert_eq!(report_paths().len(), 2, "source digest change must refresh canonical report");
}

fn age_budget_baseline(dir: &Path, baseline: &str) -> (String, String) {
    use jet_foundation::PerformanceBudget::{stable_id, CanonicalJson};
    let path = dir.join(format!(".jet/perf/baselines/names/{baseline}.json"));
    let mut manifest = CanonicalJson::parse_canonical(&fs::read(&path).unwrap()).unwrap();
    let CanonicalJson::Object(wrapper) = &mut manifest else { panic!("manifest") };
    let report_id = {
        let CanonicalJson::Object(content) = &wrapper["content"] else { panic!("content") };
        let CanonicalJson::String(id) = &content["head_report_id"] else { panic!("head") };
        id.clone()
    };
    {
        let CanonicalJson::Object(content) = wrapper.get_mut("content").unwrap() else { panic!("content") };
        let CanonicalJson::Array(generations) = content.get_mut("generations").unwrap() else { panic!("generations") };
        let CanonicalJson::Object(generation) = &mut generations[0] else { panic!("generation") };
        let CanonicalJson::Object(audit) = generation.get_mut("audit").unwrap() else { panic!("audit") };
        audit.insert("accepted_at".into(), CanonicalJson::String("2000-01-01T00:00:00.000000000Z".into()));
        let mut body = audit.clone();
        body.remove("audit_id").unwrap();
        audit.insert("audit_id".into(), CanonicalJson::String(stable_id(&CanonicalJson::Object(body))));
    }
    let state_id = stable_id(&wrapper["content"]);
    wrapper.insert("manifest_id".into(), CanonicalJson::String(state_id.clone()));
    fs::write(path, manifest.bytes()).unwrap();
    (report_id, state_id)
}

#[test]
fn budget_stale_history_is_persisted_rendered_and_bootstrap_appends() {
    use jet_foundation::PerformanceBudget::CanonicalJson;
    let dir = benchmark_budget_project("budget_stale_history");
    let source = fs::read_to_string(dir.join("src/main.jet")).unwrap().replace("enforcement: .Warn", "enforcement: .Fail");
    fs::write(dir.join("src/main.jet"), source).unwrap();
    let first = Command::new(jet()).args(["budget","update","--baseline","ci/linux","--bootstrap","--reason","initial benchmark","--yes","--json"]).current_dir(&dir).output().unwrap();
    assert_eq!(first.status.code(),Some(0),"stdout: {}\nstderr: {}",String::from_utf8_lossy(&first.stdout),String::from_utf8_lossy(&first.stderr));
    let (first_id, stale_state_id) = age_budget_baseline(&dir, "ci/linux");

    let check = Command::new(jet()).args(["budget","check","--json"]).current_dir(&dir).output().unwrap();
    assert_eq!(check.status.code(),Some(1),"stdout: {}\nstderr: {}",String::from_utf8_lossy(&check.stdout),String::from_utf8_lossy(&check.stderr));
    let CanonicalJson::Object(check) = CanonicalJson::parse_canonical(&check.stdout).unwrap() else { panic!("command") };
    assert_eq!(check["status"],CanonicalJson::String("stale".into()));
    assert_eq!(check["failure_kind"],CanonicalJson::String("evidence".into()));
    let CanonicalJson::Array(results)=&check["results"] else { panic!("results") };
    let CanonicalJson::Object(result)=&results[0] else { panic!("result") };
    assert_eq!(result["stale"],CanonicalJson::Bool(true));
    assert_eq!(result["status"],CanonicalJson::String("stale".into()));
    assert_eq!(result["evidence"],CanonicalJson::String("unavailable".into()));
    assert_eq!(result["baseline_report_ids"],CanonicalJson::Array(vec![CanonicalJson::String(first_id.clone())]));
    let CanonicalJson::String(reason)=&result["reason"] else { panic!("reason") };
    assert!(reason.contains("compatible history is stale"),"{reason}");
    assert!(reason.contains("policy limit is 2592000 seconds"),"{reason}");
    let CanonicalJson::Object(report)=&check["report"] else { panic!("report") };
    jet_foundation::PerformanceBudget::verify_budget_report(&CanonicalJson::Object(report.clone()).bytes()).unwrap();
    let CanonicalJson::Object(content)=&report["content"] else { panic!("content") };
    let CanonicalJson::Array(measurements)=&content["measurements"] else { panic!("measurements") };
    let CanonicalJson::Object(measurement)=&measurements[0] else { panic!("measurement") };
    let CanonicalJson::Object(history)=&measurement["history"] else { panic!("history") };
    assert_eq!(history["state_id"],CanonicalJson::String(stale_state_id.clone()));
    assert_eq!(history["report_ids"],CanonicalJson::Array(vec![CanonicalJson::String(first_id.clone())]));
    assert_eq!(measurement["baseline"],CanonicalJson::Null,"stale samples must not be pooled");
    let CanonicalJson::Object(decision)=&measurement["decision"] else { panic!("decision") };
    let CanonicalJson::Object(trend)=&decision["trend"] else { panic!("trend") };
    assert_eq!(trend["report_ids"],CanonicalJson::Array(vec![CanonicalJson::String(first_id.clone())]));

    let human = Command::new(jet()).args(["budget","check","--annotations","none"]).current_dir(&dir).output().unwrap();
    assert_eq!(human.status.code(),Some(1));
    let human = String::from_utf8(human.stderr).unwrap();
    assert!(human.contains("Error [E2906]: performance budget parse has no usable evidence"),"{human}");
    assert!(human.contains("compatible history is stale"),"{human}");
    assert!(human.contains("budgets stale: 1 baseline stale · report "),"{human}");

    let bootstrap = Command::new(jet()).args(["budget","update","--baseline","ci/linux","--bootstrap","--reason","refresh stale benchmark","--yes","--json"]).current_dir(&dir).output().unwrap();
    assert_eq!(bootstrap.status.code(),Some(0),"stdout: {}\nstderr: {}",String::from_utf8_lossy(&bootstrap.stdout),String::from_utf8_lossy(&bootstrap.stderr));
    let CanonicalJson::Object(bootstrap)=CanonicalJson::parse_canonical(&bootstrap.stdout).unwrap() else { panic!("bootstrap") };
    assert_eq!(bootstrap["applied"],CanonicalJson::Bool(true));
    assert_eq!(bootstrap["status"],CanonicalJson::String("stale".into()));
    let CanonicalJson::Object(report)=&bootstrap["report"] else { panic!("report") };
    let CanonicalJson::String(second_id)=&report["report_id"] else { panic!("report id") };
    let manifest = CanonicalJson::parse_canonical(&fs::read(dir.join(".jet/perf/baselines/names/ci/linux.json")).unwrap()).unwrap();
    let CanonicalJson::Object(wrapper)=manifest else { panic!("manifest") };
    let CanonicalJson::Object(content)=&wrapper["content"] else { panic!("content") };
    let CanonicalJson::Array(generations)=&content["generations"] else { panic!("generations") };
    assert_eq!(generations.len(),2);
    let CanonicalJson::Object(second)=&generations[1] else { panic!("generation") };
    let CanonicalJson::Object(audit)=&second["audit"] else { panic!("audit") };
    assert_eq!(audit["kind"],CanonicalJson::String("bootstrap".into()));
    assert_eq!(audit["prior_state_id"],CanonicalJson::String(stale_state_id));
    assert_eq!(audit["prior_head_report_id"],CanonicalJson::String(first_id));
    assert_eq!(second["report_id"],CanonicalJson::String(second_id.clone()));
}

#[test]
fn budget_effect_count_uses_solved_effects_not_import_count() {
    let dir = budget_project("budget_effect_truth", 10);
    fs::write(dir.join("src/main.jet"), r#"use core.files as files
module perf.package {
    budgets: [Budget.{ name: "effects", scope: .Package, metric: .EffectCount, comparison: .Absolute, limit: .AtMost(0) }],
}
fn run() {}
"#).unwrap();
    let out = Command::new(jet()).args(["budget", "check", "--json"]).current_dir(&dir).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    let json = String::from_utf8(out.stdout).unwrap();
    assert!(json.contains("\"budget_id\":\"package:effects\""));
    assert!(json.contains("\"point\":{\"den\":1,\"num\":0}"), "unused core import must not fabricate an effect: {json}");
}

#[test]
fn budget_generated_unsafe_rejects_proxy_before_artifact() {
    let dir = budget_project("budget_unsafe_truth", 10);
    fs::write(dir.join("src/main.jet"), r#"use core.mem as mem
module perf.package {
    budgets: [Budget.{ name: "unsafe", scope: .Package, metric: .GeneratedUnsafe, comparison: .Absolute, limit: .AtMost(0) }],
}
fn run() {}
"#).unwrap();
    let out = Command::new(jet()).args(["budget", "check"]).current_dir(&dir).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("has no exact checked front-end fact; refusing proxy measurement"), "{stderr}");
    assert!(!dir.join(".jet").exists(), "unsupported fact emitted an artifact");
}

#[test]
#[cfg(unix)]
fn budget_path_tools_cannot_forge_provenance() {
    use std::os::unix::fs::PermissionsExt;
    let dir = budget_project("budget_hostile_path", 10);
    let fake = dir.join("fake-bin");
    fs::create_dir(&fake).unwrap();
    for (name, body) in [
        ("rustc", "#!/bin/sh\necho 'host: fake-forged-triple'\n"),
        ("sha256sum", "#!/bin/sh\necho 'ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff  fake'\n"),
        ("shasum", "#!/bin/sh\necho 'ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff  fake'\n"),
    ] {
        let path = fake.join(name);
        fs::write(&path, body).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let out = Command::new(jet()).args(["budget", "check", "--json"]).env("PATH", &fake).current_dir(&dir).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    let json = String::from_utf8(out.stdout).unwrap();
    assert!(!json.contains("fake-forged-triple"), "PATH rustc forged target identity");
    assert!(!json.contains("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"), "PATH digest tool forged compiler identity");
}

#[test]
#[cfg(unix)]
fn budget_unreadable_compiler_identity_rejects_before_artifact() {
    use std::os::unix::fs::PermissionsExt;
    let dir = budget_project("budget_missing_compiler_identity", 10);
    let copied = dir.join("jet-unreadable");
    fs::copy(jet(), &copied).unwrap();
    fs::set_permissions(&copied, fs::Permissions::from_mode(0o111)).unwrap();
    let out = Command::new(&copied).args(["budget", "check"]).current_dir(&dir).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("cannot hash running compiler executable"), "{stderr}");
    assert!(!dir.join(".jet").exists(), "missing compiler identity emitted an artifact");
}

#[test]
fn budget_failure_has_human_github_projection_and_exit_one() {
    let dir = budget_project("budget_failure", 0);
    let out = Command::new(jet()).args(["budget", "check", "--annotations", "github"]).current_dir(&dir).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("Error [E2907]: performance budget public-api regressed"), "{stderr}");
    assert!(stderr.contains("::error file=src/main.jet"), "{stderr}");
    assert!(stderr.contains("performance budget public-api regressed%0AWhy: measured estimator"), "{stderr}");
    assert!(stderr.contains("%0AFix: improve the measured behavior, inspect `jet budget check --verbose`, or record an explicit exception"), "{stderr}");
    assert!(stderr.contains("budgets failed: 1 budget failed · report "), "{stderr}");
}

#[test]
fn budget_imported_declaration_reports_owning_module_location() {
    let dir = budget_project("budget_imported_source", 10);
    fs::write(dir.join("src/main.jet"), "module perf_defs;\nfn run() {}\n").unwrap();
    fs::write(dir.join("src/perf_defs.jet"), r#"module perf.package {
    budgets: [Budget.{ name: "imported-api", scope: .Package, metric: .PublicApiItems, comparison: .Absolute, limit: .AtMost(0) }],
}
pub fn imported() {}
"#).unwrap();
    let out = Command::new(jet()).args(["budget", "check", "--annotations", "github"]).current_dir(&dir).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains(" --> src/perf_defs.jet:2:15"), "{stderr}");
    assert!(stderr.contains("::error file=src/perf_defs.jet,line=2,col=15,title=Jet E2907::"), "{stderr}");
    let report = fs::read_dir(dir.join(".jet/perf/reports")).unwrap().next().unwrap().unwrap().path();
    let report = fs::read_to_string(report).unwrap();
    assert!(report.contains("\"source\":\"src/perf_defs.jet:2\""), "{report}");
}

#[test]
fn budget_update_is_plan_first_and_yes_applies_once() {
    use jet_foundation::PerformanceBudget::CanonicalJson;
    let dir = budget_project("budget_update", 10);
    let args = ["budget", "update", "--baseline", "ci/linux", "--bootstrap", "--reason", "initial evidence", "--json"];
    let plan = Command::new(jet()).args(args).current_dir(&dir).output().unwrap();
    assert_eq!(plan.status.code(), Some(0), "{}", String::from_utf8_lossy(&plan.stderr));
    let CanonicalJson::Object(plan)=jet_foundation::PerformanceBudget::CanonicalJson::parse_canonical(&plan.stdout).unwrap() else{panic!("command")};
    assert_eq!(plan["applied"],CanonicalJson::Bool(false));
    let CanonicalJson::Object(plan)=&plan["plan"] else{panic!("plan")};assert_eq!(plan["requires_confirmation"],CanonicalJson::Bool(false));let CanonicalJson::Array(rows)=&plan["rows"] else{panic!("rows")};assert_eq!(rows.len(),2);let CanonicalJson::Object(report)=&rows[0] else{panic!("report row")};let CanonicalJson::Object(baseline)=&rows[1] else{panic!("baseline row")};assert_eq!(report["operation"],CanonicalJson::String("create".into()));assert_eq!(report["artifact"],CanonicalJson::String("report".into()));assert_eq!(baseline["operation"],CanonicalJson::String("advance".into()));assert_eq!(baseline["artifact"],CanonicalJson::String("baseline".into()));
    assert!(!dir.join(".jet").exists(),"JSON plan-only mutated workspace");

    let applied = Command::new(jet()).args(args).arg("--yes").current_dir(&dir).output().unwrap();
    assert_eq!(applied.status.code(), Some(0), "{}", String::from_utf8_lossy(&applied.stderr));
    let applied = String::from_utf8(applied.stdout).unwrap();
    assert!(applied.contains("\"applied\":true"));
    assert!(dir.join(".jet/perf/baselines/names/ci/linux.json").is_file());
}

#[test]
fn budget_json_projection_is_exact_and_tool_failure_uses_null_report_fields() {
    use jet_foundation::PerformanceBudget::CanonicalJson;
    let dir = budget_project("budget_json_exact", 10);
    let out = Command::new(jet()).args(["budget", "check", "--json"]).current_dir(&dir).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stderr.is_empty());
    let CanonicalJson::Object(command) = CanonicalJson::parse_canonical(&out.stdout).unwrap() else { panic!("command object") };
    assert_eq!(command.keys().map(String::as_str).collect::<Vec<_>>(), ["applied","command","diagnostics","exit_code","failure_kind","plan","report","report_path","results","schema","status","version"]);
    let CanonicalJson::Array(results) = &command["results"] else { panic!("results") };
    let CanonicalJson::Object(result) = &results[0] else { panic!("result") };
    assert_eq!(result.keys().map(String::as_str).collect::<Vec<_>>(), ["baseline_report_ids","budget_id","comparison","diagnostic_code","direction","enforcement","evidence","lower95","metric","point","reason","source","stale","status","trend","unit","upper95"]);
    let CanonicalJson::Object(source) = &result["source"] else { panic!("source") };
    assert_eq!(source.keys().map(String::as_str).collect::<Vec<_>>(), ["column","line","path"]);
    let CanonicalJson::Object(comparison) = &result["comparison"] else { panic!("comparison") };
    assert_eq!(comparison.keys().map(String::as_str).collect::<Vec<_>>(), ["direction","kind","limit"]);

    fs::write(dir.join("src/main.jet"), "fn run( {\n").unwrap();
    let invalid = Command::new(jet()).args(["budget", "check", "--json"]).current_dir(&dir).output().unwrap();
    assert_eq!(invalid.status.code(), Some(1));
    assert!(invalid.stderr.is_empty());
    let CanonicalJson::Object(invalid) = CanonicalJson::parse_canonical(&invalid.stdout).unwrap() else { panic!("compiler failure object") };
    assert_eq!(invalid["failure_kind"], CanonicalJson::String("compiler".into()));
    let CanonicalJson::Array(diagnostics) = &invalid["diagnostics"] else { panic!("diagnostics") };
    let CanonicalJson::Object(diagnostic) = &diagnostics[0] else { panic!("diagnostic") };
    let CanonicalJson::Object(source) = &diagnostic["source"] else { panic!("diagnostic source") };
    assert_eq!(source.keys().map(String::as_str).collect::<Vec<_>>(), ["column","end_column","end_line","line","path"]);
    assert_eq!(source["path"], CanonicalJson::String("src/main.jet".into()));
    assert!(matches!(source["end_line"], CanonicalJson::Integer(_)));
    assert!(matches!(source["end_column"], CanonicalJson::Integer(_)));

    let empty = isolated_cwd("budget_json_tool_failure");
    let failed = Command::new(jet()).args(["budget", "check", "--json"]).current_dir(&empty).output().unwrap();
    assert_eq!(failed.status.code(), Some(1));
    assert!(failed.stderr.is_empty());
    let CanonicalJson::Object(failure) = CanonicalJson::parse_canonical(&failed.stdout).unwrap() else { panic!("failure object") };
    assert_eq!(failure["status"], CanonicalJson::String("fail".into()));
    assert_eq!(failure["failure_kind"], CanonicalJson::String("tool".into()));
    assert_eq!(failure["report"], CanonicalJson::Null);
    assert_eq!(failure["report_path"], CanonicalJson::Null);
    assert_eq!(failure["plan"], CanonicalJson::Null);
}

#[test]
fn budget_non_tty_plan_only_creates_no_artifact_or_baseline() {
    let dir = budget_project("budget_non_tty_cancel", 10);
    let out = Command::new(jet()).args(["budget", "update", "--baseline", "ci/linux", "--bootstrap", "--reason", "initial evidence"]).current_dir(&dir).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("plan only; pass -y or --yes to apply in a non-interactive shell"), "{stderr}");
    assert!(!dir.join(".jet").exists(), "plan-only update mutated workspace");
}

#[cfg(unix)]
fn run_budget_update_pty(dir: &Path, answer: &[u8]) -> (i32, String) {
    use std::fs::File;
    use std::io::{Read, Write};
    use std::os::fd::FromRawFd;
    use std::process::Stdio;
    unsafe extern "C" { fn openpty(master: *mut i32, slave: *mut i32, name: *mut i8, termp: *const u8, winp: *const u8) -> i32; }
    let (mut master_fd, mut slave_fd) = (-1, -1);
    assert_eq!(unsafe { openpty(&mut master_fd, &mut slave_fd, std::ptr::null_mut(), std::ptr::null(), std::ptr::null()) }, 0);
    let mut master = unsafe { File::from_raw_fd(master_fd) };
    let slave = unsafe { File::from_raw_fd(slave_fd) };
    let mut child = Command::new(jet())
        .args(["budget", "update", "--baseline", "ci/linux", "--bootstrap", "--reason", "initial evidence"])
        .current_dir(dir)
        .stdin(Stdio::from(slave.try_clone().unwrap()))
        .stdout(Stdio::from(slave.try_clone().unwrap()))
        .stderr(Stdio::from(slave))
        .spawn().unwrap();
    master.write_all(answer).unwrap();
    let status = child.wait().unwrap();
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 4096];
    loop { match master.read(&mut buffer) { Ok(0) => break, Ok(n) => bytes.extend_from_slice(&buffer[..n]), Err(error) if error.raw_os_error() == Some(5) => break, Err(error) => panic!("PTY read: {error}") } }
    (status.code().unwrap(), String::from_utf8_lossy(&bytes).replace("\r\n", "\n"))
}

#[test]
#[cfg(unix)]
fn budget_tty_confirmation_cancel_and_yes_control_mutation() {
    let cancelled = budget_project("budget_tty_no", 10);
    let (code, transcript) = run_budget_update_pty(&cancelled, b"n\n");
    assert_eq!(code, 0, "{transcript}");
    assert!(transcript.contains("Apply? [y/N]"), "{transcript}");
    assert!(transcript.contains("plan cancelled; no baseline changed"), "{transcript}");
    assert!(!cancelled.join(".jet").exists(), "TTY cancel mutated workspace");

    let applied = budget_project("budget_tty_yes", 10);
    let (code, transcript) = run_budget_update_pty(&applied, b"yes\n");
    assert_eq!(code, 0, "{transcript}");
    assert!(transcript.contains("Apply? [y/N]"), "{transcript}");
    assert_eq!(transcript.matches("+ report ").count(), 1, "plan/apply duplicated report row: {transcript}");
    assert_eq!(transcript.matches("~ baseline ").count(), 1, "plan/apply duplicated baseline row: {transcript}");
    assert!(applied.join(".jet/perf/baselines/names/ci/linux.json").is_file(), "TTY yes did not apply");
    assert!(applied.join(".jet/perf/reports").read_dir().unwrap().next().is_some(), "TTY yes omitted report artifact");
}

#[test]
fn budget_surface_is_generated_into_help_completions_and_man() {
    let help = Command::new(jet()).arg("help").output().unwrap();
    assert!(String::from_utf8(help.stdout).unwrap().contains("budget"));
    let completions = Command::new(jet()).args(["self", "completions", "bash"]).output().unwrap();
    let completions = String::from_utf8(completions.stdout).unwrap();
    assert!(completions.contains("budget"));
    let man = Command::new(jet()).args(["self", "man"]).output().unwrap();
    let man = String::from_utf8(man.stdout).unwrap();
    assert!(man.contains("budget"));
    for flag in ["--annotations","--baseline","--bootstrap","--accept-regression","--reason","--yes","-y"] {
        assert!(completions.contains(flag),"completion omitted {flag}");
        assert!(man.contains(flag),"man page omitted {flag}");
    }
}

// ── Exit-code table ────────────────────────────────────────────────

#[test]
fn exit_code_ok_check() {
    let p = std::env::temp_dir().join("jet_cli_ok.jet");
    fs::write(&p, "fn run() {\n    print(\"hi\");\n}\n").unwrap();
    let out = Command::new(jet()).arg("check").arg(&p).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "clean check should exit 0");
}

#[test]
fn exit_code_user_error_check() {
    let p = bad_file(&line!().to_string());
    let out = Command::new(jet()).arg("check").arg(&p).output().unwrap();
    assert_eq!(out.status.code(), Some(1), "a program error should exit 1");
}

#[test]
fn exit_code_no_args_starts_repl() {
    // c6vz465: bare `jet` starts the REPL — exit 0 after EOF on piped stdin.
    let out = Command::new(jet()).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "no args should start REPL (exit 0)"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("interactive REPL"),
        "bare jet should print REPL banner:\n{}",
        stdout
    );
}

#[test]
fn exit_code_unknown_subcommand_is_usage() {
    // A typo'd subcommand is a usage error (exit 2) and teaches E2101.
    let out = Command::new(jet()).arg("buld").output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "unknown subcommand should exit 2"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E2101"), "should cite E2101:\n{}", stderr);
    assert!(
        stderr.contains("build"),
        "should suggest `build`:\n{}",
        stderr
    );
}

#[test]
fn frequency_ring_groups_execute_real_handlers() {
    let out = Command::new(jet()).args(["self", "man"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains(".TH JET 1"));

    let out = Command::new(jet()).args(["inspect", "semindex"]).output().unwrap();
    assert_ne!(out.status.code(), Some(2), "group must reach semindex handler");
    assert!(String::from_utf8_lossy(&out.stderr).contains("needs an entry file"));

    let out = Command::new(jet()).args(["hangar", "generations"]).output().unwrap();
    assert_ne!(out.status.code(), Some(2), "existing grouped handler must remain live");
}

#[test]
fn moved_bare_commands_are_teaching_errors_not_aliases() {
    for (verb, replacement) in [
        ("publish", "jet registry publish"),
        ("semindex", "jet inspect semindex"),
        ("doctor", "jet self doctor"),
        ("lsp", "jet self lsp"),
        ("push", "jet os push"),
    ] {
        let out = Command::new(jet()).arg(verb).arg("sentinel").output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{verb} must be rejected");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("E2101"), "{verb}: {stderr}");
        assert!(stderr.contains(replacement), "{verb}: {stderr}");
    }

    let out = Command::new(jet()).args(["lsp", "--json"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stderr.is_empty());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("\"code\":\"E2101\""));
    assert!(stdout.contains("jet self lsp --json"));
}

/// D-CLI-STORE2=A / D-CLI-DEVSERVE1=A / D-CLI-SURFACE3=B: words retired with
/// **no** `jet <group> <same-word>` rename — `teach_retired`'s bespoke path
/// (`RETIRED_BARE` in `Source/CLI.rs`), not the generic `moved_command` one.
#[test]
fn retired_bespoke_words_teach_real_spelling() {
    for (argv, replacement) in [
        (vec!["gc"], "jet clean"),
        (vec!["store", "verify"], "jet hangar verify"),
        (vec!["store", "generations"], "jet hangar generations"),
        (vec!["store", "gc"], "jet clean"),
        (vec!["store", "fetch"], "jet fetch"),
        (vec!["serve", "main.jet"], "jet dev main.jet --swap"),
        (vec!["lock", "stats.jet"], "jet fetch --lock stats.jet"),
    ] {
        let out = Command::new(jet()).args(&argv).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{argv:?} must be rejected");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("E2101"), "{argv:?}: {stderr}");
        assert!(stderr.contains(replacement), "{argv:?}: {stderr}");
    }
}

#[test]
fn every_moved_bare_action_is_e2101_in_human_and_json_modes() {
    for group in jet::CLI::COMMAND_GROUPS {
        for action in group.actions {
            let replacement = format!("jet {} {}", group.name, action.name);
            let out = Command::new(jet()).arg(action.name).arg("sentinel").output().unwrap();
            assert_eq!(out.status.code(), Some(2), "bare {}", action.name);
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert!(stderr.contains("E2101") && stderr.contains(&replacement), "{}: {stderr}", action.name);

            let out = Command::new(jet()).args([action.name, "sentinel\\\"quoted", "--json"]).output().unwrap();
            assert_eq!(out.status.code(), Some(2), "bare {} --json", action.name);
            assert!(out.stderr.is_empty(), "JSON diagnostic leaked stderr for {}", action.name);
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert!(stdout.contains("\"code\":\"E2101\"") && stdout.contains(&replacement), "{}: {stdout}", action.name);
            assert!(stdout.contains("sentinel\\\\\\\"quoted"), "replacement was not JSON escaped: {stdout}");
        }
    }
}

#[test]
fn invalid_nested_action_is_e2101_and_json_escaped() {
    let bad = "bad\\\"action";
    // D-CLI-SURFACE3=B: `os` is not exhaustive (see `CommandGroup::exhaustive`)
    // — an unmodeled subword falls through to the real `jet os` dispatcher,
    // which teaches its own (non-E2101) "not a jetos verb" error, not this
    // registry's generic invalid-action path.
    for group in jet::CLI::COMMAND_GROUPS.iter().filter(|g| g.exhaustive) {
        let out = Command::new(jet()).args([group.name, bad]).output().unwrap();
        assert_eq!(out.status.code(), Some(2));
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("E2101") && stderr.contains(bad), "{stderr}");

        let out = Command::new(jet()).args([group.name, bad, "--json"]).output().unwrap();
        assert_eq!(out.status.code(), Some(2));
        assert!(out.stderr.is_empty());
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("bad\\\\\\\"action"), "invalid JSON escaping: {stdout}");
        assert!(!stdout.contains("`bad\\\"action`"), "raw quote leaked into JSON: {stdout}");
    }
}

#[test]
fn grouped_e2101_human_and_json_goldens() {
    let moved = Command::new(jet()).args(["publish", "sentinel"]).output().unwrap();
    assert_eq!(moved.status.code(), Some(2));
    assert!(moved.stdout.is_empty());
    check_snapshot("moved_bare_e2101_human.txt", &String::from_utf8_lossy(&moved.stderr));

    let moved_json = Command::new(jet()).args(["publish", "sentinel\\\"quoted", "--json"]).output().unwrap();
    assert_eq!(moved_json.status.code(), Some(2));
    assert!(moved_json.stderr.is_empty());
    check_snapshot("moved_bare_e2101_json.txt", &String::from_utf8_lossy(&moved_json.stdout));

    let invalid = Command::new(jet()).args(["inspect", "bad\\\"action"]).output().unwrap();
    assert_eq!(invalid.status.code(), Some(2));
    assert!(invalid.stdout.is_empty());
    check_snapshot("invalid_nested_e2101_human.txt", &String::from_utf8_lossy(&invalid.stderr));

    let invalid_json = Command::new(jet()).args(["inspect", "bad\\\"action", "--json"]).output().unwrap();
    assert_eq!(invalid_json.status.code(), Some(2));
    assert!(invalid_json.stderr.is_empty());
    check_snapshot("invalid_nested_e2101_json.txt", &String::from_utf8_lossy(&invalid_json.stdout));
}

#[test]
fn group_help_and_man_inventory_every_nested_description() {
    let man = Command::new(jet()).args(["self", "man"]).output().unwrap();
    assert_eq!(man.status.code(), Some(0));
    let man = String::from_utf8_lossy(&man.stdout);
    for group in jet::CLI::COMMAND_GROUPS {
        // D-CLI-SURFACE3=B: a non-exhaustive group (`os`) doesn't own its bare
        // `help` output — that stays the real `jet os` dispatcher's, which
        // this registry can't predict — so only the *static* man-page
        // inventory is checked for it. An exhaustive group's `help` is
        // CLI-owned and must list every action.
        if group.exhaustive {
            let out = Command::new(jet()).args([group.name, "help"]).output().unwrap();
            assert_eq!(out.status.code(), Some(0));
            let help = String::from_utf8_lossy(&out.stdout);
            assert!(help.contains(group.summary), "{} help missing summary", group.name);
            for action in group.actions {
                assert!(help.contains(action.name) && help.contains(action.summary), "{} help missing {}", group.name, action.name);
            }
        }
        for action in group.actions {
            assert!(man.contains(&format!(".B {} {}", group.name, action.name)), "man missing {} {}", group.name, action.name);
            assert!(man.contains(action.summary), "man missing summary for {} {}", group.name, action.name);
        }
    }
}

#[test]
fn palette_uses_canonical_nested_routes() {
    for group in jet::CLI::COMMAND_GROUPS {
        for action in group.actions {
            let route = format!("{} {}", group.name, action.name);
            let out = Command::new(jet()).args(["?", &route]).output().unwrap();
            assert_eq!(out.status.code(), Some(0));
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert!(stdout.contains(&route), "palette missing {route}: {stdout}");
            assert!(!stdout.contains(&format!("jet {}   ", action.name)), "palette advertised bare moved action {}", action.name);
        }
    }
}

#[test]
fn jet_install_teaches_jet_fetch() {
    // `jet install` is not a Jet command; the compiler emits E0043 pointing to `jet fetch`.
    let out = Command::new(jet()).arg("install").output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E0043"),
        "`jet install` should emit E0043 teaching error:\n{stderr}"
    );
    assert!(
        stderr.contains("jet fetch"),
        "`jet install` error should mention `jet fetch`:\n{stderr}"
    );
}

#[test]
fn exit_code_explain_unknown() {
    let out = Command::new(jet())
        .arg("explain")
        .arg("E9999")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "unknown code should exit 1");
}

// ── Human + JSON golden for one diagnostic ────────────────────────

#[test]
fn check_human_golden() {
    let p = bad_file(&line!().to_string());
    let out = Command::new(jet())
        .arg("check")
        .arg(&p)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = scrub(&String::from_utf8_lossy(&out.stderr), &p);
    check_snapshot("check_human.txt", &stderr);
}

#[test]
fn check_json_golden() {
    let p = bad_file(&line!().to_string());
    let out = Command::new(jet())
        .arg("check")
        .arg(&p)
        .arg("--json")
        .output()
        .unwrap();
    let stderr = scrub(&String::from_utf8_lossy(&out.stderr), &p);
    check_snapshot("check_json.txt", &stderr);
}

#[test]
fn build_json_golden() {
    let p = bad_file(&line!().to_string());
    let out = Command::new(jet())
        .arg("build")
        .arg(&p)
        .arg("--json")
        .output()
        .unwrap();
    let stderr = scrub(&String::from_utf8_lossy(&out.stderr), &p);
    check_snapshot("build_json.txt", &stderr);
}

#[test]
fn test_json_golden() {
    let p = bad_file(&line!().to_string());
    let out = Command::new(jet())
        .arg("test")
        .arg(&p)
        .arg("--json")
        .output()
        .unwrap();
    let stderr = scrub(&String::from_utf8_lossy(&out.stderr), &p);
    check_snapshot("test_json.txt", &stderr);
}

// ── CI determinism: ANSI-free + identical when piped/NO_COLOR ──────

#[test]
fn ci_output_is_ansi_free_when_piped() {
    let p = bad_file(&line!().to_string());
    // Default (piped, not a TTY): must be plain.
    let piped = Command::new(jet()).arg("check").arg(&p).output().unwrap();
    let s = String::from_utf8_lossy(&piped.stderr);
    assert!(
        !s.contains('\x1b'),
        "piped output must be ANSI-free:\n{}",
        s
    );

    // NO_COLOR explicitly set: also plain.
    let no_color = Command::new(jet())
        .arg("check")
        .arg(&p)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let nc = String::from_utf8_lossy(&no_color.stderr);
    assert!(!nc.contains('\x1b'), "NO_COLOR output must be ANSI-free");

    // And the two must be byte-identical (determinism).
    assert_eq!(s, nc, "piped and NO_COLOR output must match exactly");
}

#[test]
fn color_always_adds_ansi_but_flag_wins_over_no_color() {
    let p = bad_file(&line!().to_string());
    let out = Command::new(jet())
        .arg("check")
        .arg(&p)
        .arg("--color=always")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&out.stderr);
    assert!(
        s.contains('\x1b'),
        "--color=always must win over NO_COLOR and emit ANSI"
    );
}

// ── explain coverage: every registered code resolves ──────────────

#[test]
fn every_registered_code_has_an_explain_entry() {
    let md = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/spec/diagnostics.md"),
    )
    .unwrap();

    // Pull every E####/L#### that appears as the first cell of a table row —
    // i.e. a registered code, not an in-prose mention.
    let mut codes: Vec<String> = Vec::new();
    for line in md.lines() {
        let line = line.trim();
        if !line.starts_with("| E") && !line.starts_with("| L") {
            continue;
        }
        let first = line
            .trim_matches('|')
            .split('|')
            .next()
            .unwrap_or("")
            .trim();
        if is_code(first) && !codes.contains(&first.to_string()) {
            codes.push(first.to_string());
        }
    }
    assert!(
        codes.len() > 150,
        "expected the full code registry, found {}",
        codes.len()
    );

    let index = jet::Explain::index();
    for code in &codes {
        assert!(
            index.contains_key(code),
            "code {} is registered in diagnostics.md but has no explain entry",
            code
        );
        // And `jet explain <code>` must succeed at the CLI for every code.
        let out = Command::new(jet())
            .arg("explain")
            .arg(code)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "`jet explain {}` should succeed",
            code
        );
        assert!(
            String::from_utf8_lossy(&out.stdout).contains(code.as_str()),
            "`jet explain {}` output should name the code",
            code
        );
    }
}

#[test]
fn explain_golden() {
    let out = Command::new(jet())
        .arg("explain")
        .arg("E2001")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    check_snapshot("explain_E2001.txt", &stdout);
}

#[test]
fn jetpack_missing_build_log_golden() {
    let cwd = isolated_cwd(&line!().to_string());
    let root = cwd.join("jetpack-root");
    let out = Command::new(jet())
        .args(["inspect", "logs", "definitely_missing", "--no-color"])
        .current_dir(&cwd)
        .env("JETPACK_ROOT", &root)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "missing log is usage-class error"
    );
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    check_snapshot("e1274_missing_build_log.txt", &stderr);
}

fn is_code(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 5 && (b[0] == b'E' || b[0] == b'L') && b[1..].iter().all(|c| c.is_ascii_digit())
}

// ── Wave B: greeting, did-you-mean, doctor, completions, fix, externals ──

#[test]
fn no_args_repl_banner_golden() {
    let out = Command::new(jet()).env("NO_COLOR", "1").output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    check_snapshot("no_args_repl_banner.txt", &stdout);
}

#[test]
fn question_mark_is_help_golden() {
    let out = Command::new(jet())
        .arg("?")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "`jet ?` should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    check_snapshot("question_mark_help.txt", &stdout);
}

/// D-FE-HELP1=D: `jet ? <query>` (piped, i.e. non-TTY) is the non-interactive
/// floor — best matches for the query, printed once, no raw mode.
#[test]
fn question_mark_query_prints_matches_non_interactively() {
    let out = Command::new(jet())
        .args(["?", "run"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "`jet ? run` should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(stdout.contains("jet run"), "expected a `run` match, got:\n{}", stdout);
}

#[test]
fn question_mark_language_symbol_uses_shared_semantic_index() {
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["?", "List.filter"])
        .env("NO_COLOR", "1")
        .output()
        .expect("run jet ? List.filter");
    assert!(output.status.success(), "status: {:?}", output.status);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("List.filter(f: fn(T) -> Bool) -> List<T>"), "signature missing: {stdout}");
    assert!(stdout.contains("Keeps items where f(item) is true."), "summary missing: {stdout}");
    assert!(stdout.contains("Example:"), "example missing: {stdout}");
    assert!(stdout.contains("core.collections"), "provenance missing: {stdout}");
}

/// A query that looks like a diagnostic code renders the verbatim I4 essay —
/// byte-identical to `jet explain <CODE>`, since both go through
/// `jet::Explain::render` over the same registry (single source of truth).
#[test]
fn question_mark_code_query_matches_explain_verbatim() {
    let via_help = Command::new(jet())
        .args(["?", "E0102"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let via_explain = Command::new(jet())
        .args(["explain", "E0102"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(via_help.status.code(), Some(0));
    assert_eq!(via_explain.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&via_help.stdout),
        String::from_utf8_lossy(&via_explain.stdout),
        "`jet ? E0102` must render the same verbatim essay as `jet explain E0102` (I4)"
    );
}

/// A multi-word task/outcome phrase still resolves to a real command line —
/// the owner-modified default (2026-07-08): keywords are aliases on command
/// entries, never a separate goal menu, but they must still be findable.
#[test]
fn question_mark_task_phrase_resolves_to_a_real_command() {
    let out = Command::new(jet())
        .args(["?", "add", "a", "dependency"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(stdout.contains("jet add"), "expected `add` to surface, got:\n{}", stdout);
}

#[test]
fn file_sugar_runs_without_run_subcommand() {
    let stem = std::env::temp_dir().join("jet_cli_file_sugar");
    let file = stem.with_extension("jet");
    fs::write(&file, "fn run() {\n    print(\"file-sugar\");\n}\n").unwrap();
    let out = Command::new(jet()).arg(&file).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "jet <file> sugar should exit 0; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("file-sugar"),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn file_sugar_ext_optional() {
    let stem = std::env::temp_dir().join("jet_cli_file_sugar_extopt");
    let file = stem.with_extension("jet");
    fs::write(&file, "fn run() {\n    print(\"ext-sugar\");\n}\n").unwrap();
    let out = Command::new(jet()).arg(&stem).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "jet <stem> sugar should resolve .jet; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("ext-sugar"),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn file_sugar_missing_jet_file_errors() {
    let missing = std::env::temp_dir().join("jet_cli_file_sugar_absent.jet");
    let _ = fs::remove_file(&missing);
    let out = Command::new(jet())
        .arg(&missing)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_ne!(out.status.code(), Some(0));
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(
        combined.contains("jet_cli_file_sugar_absent"),
        "missing file should be named in output: {combined}"
    );
}

#[test]
fn did_you_mean_golden() {
    let out = Command::new(jet())
        .arg("buld")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    check_snapshot("did_you_mean.txt", &stderr);
}

#[test]
fn unknown_flag_is_e2102() {
    let p = std::env::temp_dir().join("jet_cli_ok2.jet");
    fs::write(&p, "fn run() {\n    print(\"hi\");\n}\n").unwrap();
    let out = Command::new(jet())
        .arg("check")
        .arg(&p)
        .arg("--jsn")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "unknown flag should exit 2");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E2102"), "should cite E2102:\n{}", stderr);
    assert!(
        stderr.contains("--json"),
        "should suggest --json:\n{}",
        stderr
    );
}

#[test]
fn doctor_ok_golden() {
    // On a CI/dev box rustc is present; the report is deterministic except for
    // machine-specific paths and the rustc version, which we scrub.
    let out = Command::new(jet())
        .args(["self", "doctor"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&out.stdout).into_owned();
    // Doctor must never emit ANSI when piped.
    assert!(
        !s.contains('\x1b'),
        "doctor output must be ANSI-free when piped"
    );
    // Structural assertions (a full golden would be machine-specific).
    assert!(s.contains("doctor"), "missing header:\n{}", s);
    assert!(s.contains("rustc"), "missing rustc check:\n{}", s);
    assert!(s.contains("pkg-config"), "missing C-FFI section:\n{}", s);
    assert!(s.contains("hangar"), "missing hangar check:\n{}", s);
}

#[test]
fn doctor_failure_is_l2101_snapshot() {
    let out = Command::new(jet())
        .args(["self", "doctor"])
        .env("PATH", "")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let start = stdout.find("Warning [L2101]:").expect("L2101 diagnostic");
    check_snapshot("doctor_l2101.txt", &stdout[start..]);
}

#[test]
fn fetch_without_git_is_e1203_snapshot() {
    let dir = isolated_cwd("fetch_no_git");
    fs::write(
        dir.join("pkg.jet"),
        "payload: { name: \"app\", version: \"0.1.0\", jet: \">=0.1.0\", description: \"\", license: \"MIT\" }\npackages: { app: executable }\ndeps: { tool: { git: \"https://example.invalid/tool.git\", tag: \"v1\" } }\n",
    )
    .unwrap();
    let out = Command::new(jet())
        .args(["fetch"])
        .current_dir(&dir)
        .env("PATH", "")
        .env("HOME", &dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "unexpected stderr:\n{stderr}");
    let start = stderr.find("Error [E1203]:").expect("E1203 diagnostic");
    check_snapshot("fetch_no_git_e1203.txt", &stderr[start..]);
}

#[test]
fn bind_missing_header_is_e3208() {
    let missing = std::env::temp_dir().join("jet_missing_bind_header.h");
    let _ = fs::remove_file(&missing);
    let out = Command::new(jet())
        .args(["inspect", "bind"])
        .arg(&missing)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "unexpected stderr:\n{stderr}");
    assert!(stderr.contains("Error [E3208]:"), "missing bind diagnostic:\n{stderr}");
    assert!(stderr.contains("Why:"), "missing E3208 reason:\n{stderr}");
    assert!(stderr.contains("Fix:"), "missing E3208 fix:\n{stderr}");
    check_snapshot("bind_missing_e3208.txt", &scrub(&stderr, &missing));
}

#[test]
fn fortran_bind_compiles_and_runs_iso_c_binding_scalar() {
    let dir = isolated_cwd("fortran_bind_scalar");
    let source = dir.join("scalar.f90");
    fs::write(
        &source,
        r#"module scalar_math
  use iso_c_binding
contains
  function add_i64(a, b) result(value) bind(C, name="add_i64")
    integer(c_int64_t), value :: a
    integer(c_int64_t), value :: b
    integer(c_int64_t) :: value
    value = a + b
  end function add_i64
end module scalar_math
"#,
    )
    .unwrap();

    let bind = Command::new(jet())
        .args(["inspect", "bind", "fortran"])
        .arg(&source)
        .args(["--pkg", "scalar"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        bind.status.success(),
        "Fortran bind failed:\n{}",
        String::from_utf8_lossy(&bind.stderr)
    );
    assert!(dir.join(".jet/bindings/fortran/scalar.jet").is_file());
    assert!(dir.join(".jet/bindings/fortran/libjet_fortran_scalar.a").is_file());

    fs::write(
        dir.join("main.jet"),
        "use fortran.scalar as scalar\n\nfn run() { print(scalar.add_i64(20, 22)) }\n",
    )
    .unwrap();
    let run = Command::new(jet())
        .args(["run", "main.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "generated Fortran binding did not run:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "42\n");
}

#[test]
fn go_bind_compiles_and_runs_c_archive_scalar() {
    let dir = isolated_cwd("go_bind_scalar");
    let source = dir.join("scalar.go");
    fs::write(
        &source,
        r#"package main

/*
#include <stdint.h>
*/
import "C"

//export add_i64
func add_i64(a int64, b int64) int64 {
    return a + b
}

func main() {}
"#,
    )
    .unwrap();

    let bind = Command::new(jet())
        .args(["inspect", "bind", "go"])
        .arg(&source)
        .args(["--pkg", "scalar"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        bind.status.success(),
        "Go bind failed:\n{}",
        String::from_utf8_lossy(&bind.stderr)
    );
    assert!(dir.join(".jet/bindings/go/scalar.jet").is_file());
    assert!(dir.join(".jet/bindings/go/libjet_go_scalar.a").is_file());

    fs::write(
        dir.join("main.jet"),
        "use go.scalar as scalar\n\nfn run() { print(scalar.add_i64(20, 22)) }\n",
    )
    .unwrap();
    let run = Command::new(jet())
        .args(["run", "main.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "generated Go binding did not run:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "42\n");
}

#[test]
fn go_bind_compiles_and_runs_move_only_cgo_handle() {
    let dir = isolated_cwd("go_bind_handle");
    let source = dir.join("handles.go");
    fs::write(
        &source,
        r#"package main

/*
#include <stdint.h>
*/
import "C"
import "runtime/cgo"

//export new_handle
func new_handle(value int64) uintptr {
    return uintptr(cgo.NewHandle(value))
}

//export consume_handle
func consume_handle(handle uintptr) int64 {
    owned := cgo.Handle(handle)
    value := owned.Value().(int64)
    owned.Delete()
    return value
}

func main() {}
"#,
    )
    .unwrap();

    let bind = Command::new(jet())
        .args(["inspect", "bind", "go"])
        .arg(&source)
        .args(["--pkg", "handles"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        bind.status.success(),
        "Go handle bind failed:\n{}",
        String::from_utf8_lossy(&bind.stderr)
    );
    let generated = fs::read_to_string(dir.join(".jet/bindings/go/handles.jet")).unwrap();
    assert!(generated.contains("pub struct Handle { value: Int }"));
    assert!(generated.contains("pub fn new_handle(value: Int) -> Handle"));
    assert!(generated.contains("pub fn consume_handle(handle: Handle) -> Int"));

    fs::write(
        dir.join("main.jet"),
        "use go.handles as handles\n\nfn run() #(Go, Io) {\n    handle :: handles.new_handle(42)\n    print(handles.consume_handle(handle))\n}\n",
    )
    .unwrap();
    let run = Command::new(jet())
        .args(["run", "main.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "generated Go handle binding did not run:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "42\n");
}

#[test]
fn go_bind_launders_foreign_compiler_failure_as_e3208() {
    let dir = isolated_cwd("go_bind_failure");
    let source = dir.join("broken.go");
    fs::write(
        &source,
        r#"package main

import "C"

//export broken
func broken(a int64) int64 {
    return a +
}

func main() {}
"#,
    )
    .unwrap();

    let output = Command::new(jet())
        .args(["inspect", "bind", "go"])
        .arg(&source)
        .args(["--pkg", "broken"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Error [E3208]:"), "missing Jet diagnostic:\n{stderr}");
    assert!(stderr.contains(" Why:"), "missing reason:\n{stderr}");
    assert!(stderr.contains(" Fix:"), "missing fix:\n{stderr}");
    assert!(!stderr.contains("broken.go:"), "raw Go location leaked:\n{stderr}");
    check_snapshot("bind_go_invalid_e3208.txt", &scrub(&stderr, &source));
}

#[test]
fn java_bind_embeds_jvm_handles_methods_and_exceptions() {
    let dir = isolated_cwd("java_bind_embedded");
    let source = dir.join("Counter.java");
    fs::write(&source, r#"public class Counter {
    private long value;
    public Counter(long value) { this.value = value; }
    public long add(long amount) { value += amount; return value; }
    public long explode(long code) { if (code < 0) throw new IllegalStateException("hidden foreign detail"); return code; }
    public static double twice(double value) { return value * 2.0; }
}
"#).unwrap();
    let bind=Command::new(jet()).args(["inspect","bind","java"]).arg(&source).args(["--pkg","counter"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();
    assert!(bind.status.success(),"Java bind failed:\n{}",String::from_utf8_lossy(&bind.stderr));
    assert!(dir.join(".jet/bindings/java/libjet_java_counter.a").is_file());
    assert!(dir.join(".jet/bindings/java/counter.classes/Counter.class").is_file());
    assert!(dir.join(".jet/bindings/java/counter.provenance").is_file());
    fs::write(dir.join("main.jet"),r#"use java.counter as counter

fn run() #(Java, Io) {
    handle :: counter.new(40) ?? panic("JVM create failed")
    print(counter.add(handle, 2) ?? -1)
    print(counter.twice(2.5) ?? -1.0)
    print(counter.explode(handle, -1) ?? -7)
    counter.close(^handle)
}
"#).unwrap();
    let run=Command::new(jet()).args(["run","main.jet"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();
    assert!(run.status.success(),"embedded JVM binding did not run:\n{}",String::from_utf8_lossy(&run.stderr));
    assert_eq!(String::from_utf8_lossy(&run.stdout),"42\n5.0\n-7\n");
    assert!(!String::from_utf8_lossy(&run.stderr).contains("hidden foreign detail"));
}

#[test]
fn java_bind_launders_javac_failure_as_e3208() {
    let dir=isolated_cwd("java_bind_failure"); let source=dir.join("Broken.java");
    fs::write(&source,"public class Broken { public Broken(long n) { this. = n; } public long value() { return 1; } }\n").unwrap();
    let output=Command::new(jet()).args(["inspect","bind","java"]).arg(&source).args(["--pkg","broken"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();
    assert!(!output.status.success()); let stderr=String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Error [E3208]:")); assert!(stderr.contains(" Why:")); assert!(stderr.contains(" Fix:"));
    assert!(!stderr.contains("Broken.java:"),"raw javac location leaked:\n{stderr}");
    check_snapshot("bind_java_invalid_e3208.txt", &scrub(&stderr, &source));
}

#[test]
fn fortran_bind_launders_foreign_compiler_failure_as_e3208() {
    let dir = isolated_cwd("fortran_bind_failure");
    let source = dir.join("broken.f90");
    fs::write(
        &source,
        r#"module broken_math
  use iso_c_binding
contains
  function broken(a) result(value) bind(C, name="broken")
    integer(c_int64_t), value :: a
    integer(c_int64_t) :: value
    value = a +
  end function broken
end module broken_math
"#,
    )
    .unwrap();

    let output = Command::new(jet())
        .args(["inspect", "bind", "fortran"])
        .arg(&source)
        .args(["--pkg", "broken"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Error [E3208]:"),
        "missing Jet diagnostic:\n{stderr}"
    );
    assert!(stderr.contains(" Why:"), "missing reason:\n{stderr}");
    assert!(stderr.contains(" Fix:"), "missing fix:\n{stderr}");
    assert!(
        !stderr.contains("broken.f90:"),
        "raw gfortran location leaked:\n{stderr}"
    );
    assert!(
        !stderr.contains("    7 |"),
        "raw gfortran source frame leaked:\n{stderr}"
    );
    check_snapshot(
        "bind_fortran_invalid_e3208.txt",
        &scrub(&stderr, &source),
    );
}

#[test]
fn unknown_cross_target_is_e3302() {
    let src = std::env::temp_dir().join("jet_unknown_cross_target.jet");
    fs::write(&src, "fn run() { print(\"target\") }\n").unwrap();
    let out = Command::new(jet())
        .arg("build")
        .arg(&src)
        .arg("--target=definitely-not-a-rust-target")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "unexpected stderr:\n{stderr}");
    assert!(stderr.contains("Error [E3302]:"), "missing target diagnostic:\n{stderr}");
    assert!(stderr.contains("Why:"), "missing E3302 reason:\n{stderr}");
    assert!(stderr.contains("Fix:"), "missing E3302 fix:\n{stderr}");
    check_snapshot("unknown_target_e3302.txt", &stderr);
}

#[test]
fn prove_unknown_lens_is_e2941() {
    let root = std::env::temp_dir().join("jet_cli_prove_unknown_lens");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("plain.jet"), "fn run() {}\n").unwrap();
    let out = Command::new(jet())
        .current_dir(&root)
        .args(["prove", "plain.jet", "--lens", "test"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "unexpected stderr:\n{stderr}");
    assert!(stderr.contains("Error [E2941]:"), "missing lens diagnostic:\n{stderr}");
    assert!(stderr.contains("Why:"), "missing E2941 reason:\n{stderr}");
    assert!(stderr.contains("Fix:"), "missing E2941 fix:\n{stderr}");
    check_snapshot("prove_unknown_lens_e2941.txt", &stderr);
}

#[test]
fn completions_generate_for_every_shell() {
    for shell in ["bash", "zsh", "fish", "powershell"] {
        let out = Command::new(jet())
            .args(["self", "completions"])
            .arg(shell)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "completions {} should exit 0",
            shell
        );
        let s = String::from_utf8_lossy(&out.stdout);
        for flag in ["structural", "out", "report", "repo"] {
            let spelling = if shell == "fish" {
                format!("-l {flag}")
            } else {
                format!("--{flag}")
            };
            assert!(
                s.contains(&spelling),
                "{shell} completion missing {spelling}"
            );
        }
        check_snapshot(&format!("completions_{}.txt", shell), &s);
    }
}

#[test]
fn man_page_golden() {
    let out = Command::new(jet()).args(["self", "man"]).output().unwrap();
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    // Scrub the version so the snapshot is stable across releases.
    s = s.replace(env!("CARGO_PKG_VERSION"), "VERSION");
    for flag in ["--structural", "--out", "--report", "--repo"] {
        assert!(s.contains(flag), "man page missing {flag}");
    }
    check_snapshot("man.txt", &s);
}

#[test]
fn fix_dry_run_does_not_write() {
    // A file with an autofixable diagnostic. S14 teaching fixes are paused, so
    // use the still-live Core habit fix (`println` -> `print`).
    let p = std::env::temp_dir().join("jet_cli_fix.jet");
    let original = "fn run() {\n    println(\"hi\")\n}\n";
    fs::write(&p, original).unwrap();
    let out = Command::new(jet())
        .arg("fix")
        .arg(&p)
        .arg("--dry-run")
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("dry run"), "dry-run should say so:\n{}", s);
    assert!(s.contains("print"), "diff should show the fix:\n{}", s);
    // The file on disk is unchanged.
    assert_eq!(
        fs::read_to_string(&p).unwrap(),
        original,
        "dry-run must not write"
    );

    // And a real fix DOES write.
    let out2 = Command::new(jet()).arg("fix").arg(&p).output().unwrap();
    assert_eq!(out2.status.code(), Some(0));
    assert!(
        fs::read_to_string(&p).unwrap().contains("print(\"hi\")"),
        "fix should rewrite the file"
    );
}

#[test]
fn external_subcommand_is_discovered() {
    // A fake `jet-greet` on a temp PATH should be invokable as `jet greet`.
    let dir = std::env::temp_dir().join("jet_ext_test_bin");
    fs::create_dir_all(&dir).unwrap();
    let script = dir.join("jet-greet");
    fs::write(&script, "#!/bin/sh\necho \"hi from plugin $1\"\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = fs::metadata(&script).unwrap().permissions();
        perm.set_mode(0o755);
        fs::set_permissions(&script, perm).unwrap();
    }
    let path = format!(
        "{}:{}",
        dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = Command::new(jet())
        .arg("greet")
        .arg("world")
        .env("PATH", path)
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        s.contains("hi from plugin world"),
        "external subcommand not forwarded:\n{}",
        s
    );
}

#[test]
fn osc8_hyperlinks_only_when_forced_on() {
    let p = bad_file(&line!().to_string());
    // Piped + NO_COLOR: never an OSC 8 link (existing snapshots stay clean).
    let piped = Command::new(jet())
        .arg("check")
        .arg(&p)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&piped.stderr);
    assert!(
        !s.contains("\x1b]8;;"),
        "piped output must have no OSC 8 links:\n{:?}",
        s
    );
    // The hyperlink layer is gated behind a real TTY; since tests run piped,
    // we exercise the renderer directly to prove the escape appears when asked.
    let src = "fn run() {}\n";
    let d = jet::Diagnostics::Diagnostic::error(
        "E0001",
        "x".into(),
        "y".into(),
        "z".into(),
        Some(jet::Diagnostics::Span::new(3, 7)),
    );
    let linked = d.render_linked("a.jet", src, true, true);
    assert!(
        linked.contains("\x1b]8;;"),
        "render_linked(hyperlinks=true) should emit OSC 8"
    );
    let plain = d.render_linked("a.jet", src, true, false);
    assert!(
        !plain.contains("\x1b]8;;"),
        "render_linked(hyperlinks=false) must not"
    );
}

// ── Ext-optional CLI (no syntax decision; pure CLI behavior) ──────────

#[test]
fn ext_optional_check_resolves_dot_jet() {
    // `jet check <path-without-.jet>` resolves to `<path>.jet` when the bare
    // path does not exist but the .jet file does.
    let stem = std::env::temp_dir().join("jet_cli_extopt_check");
    let file = stem.with_extension("jet");
    fs::write(&file, "fn run() {\n    print(\"ok\");\n}\n").unwrap();
    let out = Command::new(jet())
        .arg("check")
        .arg(&stem)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "ext-optional check should resolve {}.jet and exit 0; stderr: {}",
        stem.display(),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn ext_optional_run_resolves_dot_jet() {
    // Same resolution for `jet run`.
    let stem = std::env::temp_dir().join("jet_cli_extopt_run");
    let file = stem.with_extension("jet");
    fs::write(&file, "fn run() {\n    print(\"hello-extopt\");\n}\n").unwrap();
    let out = Command::new(jet()).arg("run").arg(&stem).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "ext-optional run should exit 0; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("hello-extopt"),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn ext_optional_missing_path_keeps_original_name() {
    // Neither `<path>` nor `<path>.jet` exists: the original name must surface
    // in the file-not-found error (resolution returns it unchanged).
    let stem = std::env::temp_dir().join("jet_cli_extopt_absent_xyz");
    let out = Command::new(jet())
        .arg("check")
        .arg(&stem)
        .output()
        .unwrap();
    assert_ne!(out.status.code(), Some(0));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("jet_cli_extopt_absent_xyz"),
        "error should name the original path; stderr: {err}"
    );
}

// ── D-ILE1: implicit executable inference (no pkg.jet) ───────────────

#[test]
fn simple_exec_runs_without_a_manifest() {
    // A single file with a top-level `fn run` and no pkg.jet runs as an
    // executable with zero ceremony (R9 / D-ILE1).
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/simple_exec/main.jet");
    // Isolated cwd: this fixture's stem is `main`, a common stem other tests
    // and examples also use — see `isolated_cwd`.
    let out = Command::new(jet())
        .arg("run")
        .arg(&path)
        .current_dir(isolated_cwd("simple_exec"))
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("simple exec, no manifest"),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

// ── D-CLI1: `--` separator passthrough (c11) ──────────────────────────────

/// Write a Jet fixture that prints its argument count via `io.args()`.
fn args_fixture(tag: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("jet_cli_args_{tag}.jet"));
    fs::write(
        &p,
        "use core.io as io\nfn run() {\n    args :: io.args()\n    print(args.len())\n}\n",
    )
    .unwrap();
    p
}

#[test]
fn passthrough_forwards_tokens_after_separator() {
    // `jet run file.jet -- --port 8080 x` — program sees 4 args: argv[0] +
    // three forwarded tokens. io.args().len() == 4.
    let p = args_fixture(&line!().to_string());
    let out = Command::new(jet())
        .args(["run", p.to_str().unwrap(), "--", "--port", "8080", "x"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.trim() == "4",
        "expected 4 args (argv[0] + 3 forwarded), got: {stdout}"
    );
}

#[test]
fn bare_separator_gives_empty_passthrough() {
    // `jet run file.jet --` — bare `--` with nothing after; program sees 1 arg.
    let p = args_fixture(&line!().to_string());
    let out = Command::new(jet())
        .args(["run", p.to_str().unwrap(), "--"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.trim() == "1",
        "expected 1 arg (just argv[0]), got: {stdout}"
    );
}

#[test]
fn no_separator_positional_regression() {
    // Plain positional words with no `--` still reach the program (regression
    // guard). `jet run file.jet hello` → len == 2.
    let p = args_fixture(&line!().to_string());
    let out = Command::new(jet())
        .args(["run", p.to_str().unwrap(), "hello"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.trim() == "2",
        "expected 2 args (argv[0] + hello), got: {stdout}"
    );
}

#[test]
fn unknown_flag_before_separator_is_e2102_with_passthrough_hint() {
    // `jet run file.jet --port` (no `--`) — unknown flag before `--` is E2102
    // and the Fix line teaches the `--` form (D-CLI1=A).
    let p = args_fixture(&line!().to_string());
    let out = Command::new(jet())
        .args(["run", p.to_str().unwrap(), "--port"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "unknown flag before -- should exit 2"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E2102"), "should cite E2102:\n{stderr}");
    assert!(
        stderr.contains("--"),
        "Fix should mention `--` separator:\n{stderr}"
    );
}

// ── D-BUILDPROFILE1: --release / --profile=<name> ─────────────────────────────

#[test]
fn profile_unknown_name_emits_e1219() {
    // D-BUILDPROFILE1: `--profile=<unknown>` with no pkg.jet defining that name
    // must emit E1219 and exit 1 (user error).
    let p = std::env::temp_dir().join("jet_cli_profile_test.jet");
    fs::write(&p, "fn run() { print(\"ok\") }\n").unwrap();
    let out = Command::new(jet())
        .args(["build", p.to_str().unwrap(), "--profile=staging"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "unknown --profile should exit 1 (user error)"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E1219"),
        "unknown profile should cite E1219:\n{stderr}"
    );
    assert!(
        stderr.contains("staging"),
        "E1219 should name the unknown profile:\n{stderr}"
    );
}

#[test]
fn profile_release_flag_is_accepted() {
    // `--release` is valid (blessed profile) and must not emit E1219.
    let p = std::env::temp_dir().join("jet_cli_release_test.jet");
    fs::write(&p, "fn run() { print(\"ok\") }\n").unwrap();
    // We can't guarantee rustc is in PATH for the binary build, but `jet check`
    // doesn't accept --release yet, so test that `jet build --release` at least
    // doesn't emit E1219. We check that the exit code is NOT 1-with-E1219.
    let out = Command::new(jet())
        .args(["build", p.to_str().unwrap(), "--release"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("E1219"),
        "--release must not emit E1219 (it's a blessed profile):\n{stderr}"
    );
}

#[test]
fn profile_ci_flag_is_accepted() {
    let p = std::env::temp_dir().join("jet_cli_ci_test.jet");
    fs::write(&p, "fn run() { print(\"ok\") }\n").unwrap();
    let out = Command::new(jet())
        .args(["build", p.to_str().unwrap(), "--profile=ci"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("E1219"),
        "--profile=ci must not emit E1219 (it's a blessed profile):\n{stderr}"
    );
}

#[test]
fn profile_custom_name_from_pkg_jet() {
    let dir = std::env::temp_dir().join(format!(
        "jet_cli_custom_profile_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("pkg.jet"),
        r#"payload: { name: "p", version: "0.1.0" }
build: { staging: Build.{ optimize: basic } }
"#,
    )
    .unwrap();
    let main = dir.join("main.jet");
    fs::write(&main, "fn run() { print(\"ok\") }\n").unwrap();
    // Isolated cwd: this fixture's stem is `main` — see `isolated_cwd`. Also
    // the semantically correct place for `build/` to land, since it's this
    // fixture's own project directory.
    let out = Command::new(jet())
        .args(["build", main.to_str().unwrap(), "--profile=staging"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("E1219"),
        "pkg.jet-defined profile must resolve:\n{stderr}"
    );
}

// ── D-EXPANDCLI1 (card #183): `jet inspect expand` transparency command ────

/// Fixture exercising the `inline` lens: an `@Inline` fn and an
/// `@InlineAlways` method.
fn expand_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/expand_facts.jet")
}

/// Replace the fixture's machine-specific absolute path with a stable token.
fn scrub_fixture(s: &str, fixture: &Path) -> String {
    s.replace(&fixture.display().to_string(), "FIXTURE.jet")
}

#[test]
fn expand_inline_golden() {
    let p = expand_fixture();
    let out = Command::new(jet())
        .args(["inspect", "expand", "--facts", "inline"])
        .arg(&p)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "expand --facts inline should exit 0:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = scrub_fixture(&String::from_utf8_lossy(&out.stdout), &p);
    check_snapshot("expand_inline.txt", &s);
}

#[test]
fn expand_all_golden() {
    let p = expand_fixture();
    // Bare `jet inspect expand <file>`: every lens, grouped, magic default.
    let out = Command::new(jet())
        .args(["inspect", "expand"])
        .arg(&p)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "bare expand should exit 0:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = scrub_fixture(&String::from_utf8_lossy(&out.stdout), &p);
    check_snapshot("expand_all.txt", &s);
}

#[test]
fn expand_unknown_lens_golden() {
    let p = expand_fixture();
    let out = Command::new(jet())
        .args(["inspect", "expand", "--facts", "bogus"])
        .arg(&p)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "unknown lens should exit 1 (USER_ERROR), listing available lenses"
    );
    let s = scrub_fixture(&String::from_utf8_lossy(&out.stderr), &p);
    check_snapshot("expand_unknown_lens.txt", &s);
}

#[test]
fn expand_missing_file_is_user_error() {
    let out = Command::new(jet()).args(["inspect", "expand"]).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "missing entry file is USER_ERROR"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("needs an entry file"),
        "should explain the missing file:\n{}",
        stderr
    );
}

#[test]
fn expand_compile_error_reports_ordinary_diagnostics() {
    let p = bad_file(&line!().to_string());
    let out = Command::new(jet())
        .args(["inspect", "expand"])
        .arg(&p)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "a program that fails to compile can't print facts (USER_ERROR)"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E0102"),
        "should render the ordinary front-end diagnostic:\n{}",
        stderr
    );
}

// ── D-JPK-FILENAME2=B (A2): retired manifest filenames → E1226 ──────

#[test]
fn stale_manifest_name_pack_jet_is_e1226() {
    let dir = isolated_cwd("stale_pack_jet");
    fs::write(
        dir.join("pack.jet"),
        "payload: { name: \"x\", version: \"0.1.0\" }\n",
    )
    .unwrap();
    let out = Command::new(jet())
        .arg("add")
        .arg("dep")
        .arg("--path")
        .arg("../dep")
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E1226"),
        "expected E1226 in stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("pack.jet"),
        "names the found file:\n{stderr}"
    );
    assert!(
        stderr.contains("pkg.jet"),
        "names the fix target:\n{stderr}"
    );
}

#[test]
fn stale_manifest_name_jet_toml_is_e1226() {
    let dir = isolated_cwd("stale_jet_toml");
    fs::write(dir.join("jet.toml"), "").unwrap();
    let out = Command::new(jet())
        .arg("build")
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E1226"),
        "expected E1226 in stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("jet.toml"),
        "names the found file:\n{stderr}"
    );
}

#[test]
fn stale_manifest_name_payload_jet_is_e1226() {
    let dir = isolated_cwd("stale_payload_jet");
    fs::write(dir.join("payload.jet"), "").unwrap();
    let out = Command::new(jet())
        .args(["inspect", "schema"])
        .arg("status")
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E1226"),
        "expected E1226 in stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("payload.jet"),
        "names the found file:\n{stderr}"
    );
}

/// `jetpack.toml` is a different, still-live file (D-JPK-FILES repo
/// metadata) — it must NOT be mistaken for a retired manifest name.
#[test]
fn jetpack_toml_alone_is_not_e1226() {
    let dir = isolated_cwd("jetpacktoml_not_stale");
    fs::write(dir.join("jetpack.toml"), "[repo]\nname = \"x\"\n").unwrap();
    let out = Command::new(jet())
        .arg("build")
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("E1226"),
        "jetpack.toml is a different live file, not a retired manifest name:\n{stderr}"
    );
    assert!(
        stderr.contains("no file given and no `pkg.jet` found") || stderr.contains("E1225"),
        "should fall back to the generic no-manifest message:\n{stderr}"
    );
}

/// D-PLUGIN1=B (c81): a `target: plugin` package is deny-by-default — its own
/// code using any effect (here `core.env`) must fail cleanly at build time
/// (E1258), not defer to a runtime instantiation failure. This check lives in
/// the CLI's post-compile effect-budget pass (`Source/CmdCompile.rs`), so it
/// needs the real subprocess (not the `jet::compile_plugin` library call the
/// `tests/ui` `@plugin_target` harness drives).
#[test]
fn plugin_using_an_effect_is_e1258() {
    let dir = isolated_cwd("plugin_effect_denied");
    fs::write(
        dir.join("main.jet"),
        "use core.env as env\n\npub fn get_secret() -> Int {\n    _ :: env.get(\"SECRET\")\n    return 1\n}\n",
    )
    .unwrap();
    let out = Command::new(jet())
        .arg("build")
        .arg("main.jet")
        .arg("--target=plugin")
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E1258"),
        "expected E1258 (plugin capability-denied) in stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("Env"),
        "should name the offending effect:\n{stderr}"
    );
}

/// D-DEP-WASM1=A (c81): `jet build --target=plugin` shells out to
/// `wasm-tools` to lift the rustc-built core wasm module into a Component. A
/// PATH without `wasm-tools` on it (but with `rustc` still reachable, so the
/// core-module half of the build succeeds) must fail as a clean E1259, never
/// a raw "No such file or directory" panic (I2).
#[test]
fn plugin_missing_wasm_tools_is_e1259() {
    let which = |tool: &str| -> Option<String> {
        Command::new("which")
            .arg(tool)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let (Some(rustc_path), Some(lld_path)) = (which("rustc"), which("lld")) else {
        eprintln!("note: skipping plugin_missing_wasm_tools_is_e1259 (no `rustc`/`lld` on PATH to re-expose)");
        return;
    };

    let dir = isolated_cwd("plugin_no_wasmtools");
    fs::write(
        dir.join("main.jet"),
        "pub fn scale(a: Float, b: Float) -> Float {\n    return a * b\n}\n",
    )
    .unwrap();

    // A minimal PATH exposing only `rustc` + `lld` (via symlinks), so the
    // core-wasm-module half of the build still works but `wasm-tools`
    // resolves to nothing.
    let bin_dir = dir.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let _ = symlink(&rustc_path, bin_dir.join("rustc"));
        let _ = symlink(&lld_path, bin_dir.join("lld"));
    }

    let out = Command::new(jet())
        .arg("build")
        .arg("main.jet")
        .arg("--target=plugin")
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .env("PATH", &bin_dir)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E1259"),
        "expected E1259 (missing wasm-tools) in stderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("panicked"),
        "must never panic, only report a clean diagnostic (I2):\n{stderr}"
    );
}

// ── D-ILE1 / D-CLI-BARE1: bare entry resolution (card #497 verifier bounce) ──
//
// `resolve_bare_entry` (Source/main.rs) delegated to a `find_project_entry`
// that only ever checked `main.jet`/`.jet/main.jet`, never the ratified
// D-ILE1 search order (`src/main.jet` then `<package>.jet`, the package name
// from `pkg.jet`'s `payload.name`). The shipped
// `examples/features/packages/monorepo` fixture (members `hello.jet` /
// `ranker.jet`, neither named `main.jet`) exposed it end to end: bare `jet
// run` at the workspace root couldn't see either member as runnable, `-p
// hello` said "no workspace member named `hello`", and `cd`-ing into a
// member and running bare failed too.

/// Recursively copy a directory tree — sandboxes the shipped monorepo
/// fixture into an isolated cwd so `jet run`'s `build/` output never lands in
/// the checked-in example and concurrent test runs never collide.
fn copy_dir_all(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let dest_path = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir_all(&entry.path(), &dest_path);
        } else {
            fs::copy(entry.path(), &dest_path).unwrap();
        }
    }
}

#[test]
fn monorepo_bare_entry_honors_d_ile1_search_order() {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/features/packages/monorepo");
    let root = isolated_cwd("monorepo_d_ile1");
    fs::remove_dir_all(&root).ok();
    copy_dir_all(&fixture, &root);

    let run = |dir: &Path, extra_args: &[&str]| -> std::process::Output {
        Command::new(jet())
            .arg("run")
            .args(extra_args)
            .current_dir(dir)
            .env("NO_COLOR", "1")
            .output()
            .unwrap()
    };

    // 1. Bare `jet run` at the workspace root: both members resolve via
    //    D-ILE1 (`<package>.jet`, since neither has `src/main.jet`), so the
    //    result is the D-CLI-BARE1 ambiguity error naming both.
    let out = run(&root, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(2),
        "ambiguous bare run is USAGE:\n{stderr}"
    );
    assert!(
        stderr.contains("ambiguous"),
        "expected the D-CLI-BARE1 ambiguity error:\n{stderr}"
    );
    assert!(
        stderr.contains("hello") && stderr.contains("ranker"),
        "ambiguity error should list both runnable members by their real pkg.jet name:\n{stderr}"
    );
    assert!(
        !stderr.contains("hello\"") && !stderr.contains("ranker\""),
        "member names must not carry a stray trailing quote:\n{stderr}"
    );

    // 2. `-p hello` picks the member unambiguously and actually runs it.
    let out = run(&root, &["-p", "hello"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "-p hello should run: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("hello from the monorepo"),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );

    // 3. `-p ranker` likewise.
    let out = run(&root, &["-p", "ranker"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "-p ranker should run: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("ranker: #1 monorepo demo"),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );

    // 4. `cd packages/hello && jet run` (bare, single-package convention):
    //    the member directory's own `pkg.jet` names it `hello`, so D-ILE1
    //    resolves `hello.jet` directly — no workspace ambiguity from inside.
    let member_dir = root.join("packages/hello");
    let out = run(&member_dir, &[]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "bare run inside a member should run its own entry: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("hello from the monorepo"),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );

    // 5. Outside any package or workspace, the bare-form usage error is
    //    unchanged (D-CLI-BARE1: "outside a package the bare form stays the
    //    current usage error").
    let outside = isolated_cwd("monorepo_d_ile1_outside");
    let out = run(&outside, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr.contains("no file given and no `pkg.jet` found"),
        "outside-package bare error text must stay the current usage error:\n{stderr}"
    );
}
