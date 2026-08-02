use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn jet() -> PathBuf { PathBuf::from(env!("CARGO_BIN_EXE_jet")) }

fn workspace(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("jet_prove_{name}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

fn budget_workspace(name: &str) -> PathBuf {
    let root = workspace(name);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("pkg.jet"), "payload: { name: \"app\", version: \"0.1.0\" }\n").unwrap();
    fs::write(root.join("src/main.jet"), r#"module perf.package {
    budgets: [Budget.{
        name: "public-api",
        scope: .Package,
        metric: .PublicApiItems,
        comparison: .Absolute,
        limit: .AtMost(10),
    }],
}
pub fn api() {}
fn run() {}
"#).unwrap();
    root
}

#[test]
fn prove_projects_compatible_canonical_budget_identity_without_measuring() {
    let root = budget_workspace("budget_projection");
    let checked = Command::new(jet()).current_dir(&root).args(["budget", "check", "--json"]).output().unwrap();
    assert_eq!(checked.status.code(), Some(0), "{}", String::from_utf8_lossy(&checked.stderr));
    let command = String::from_utf8(checked.stdout).unwrap();
    let report_id = command.split("\"report_id\":\"").nth(1).unwrap().split('"').next().unwrap().to_string();
    let before = fs::read_dir(root.join(".jet/perf/reports")).unwrap().count();
    let out = Command::new(jet()).current_dir(&root).args(["prove", "src/main.jet", "--json"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    let proof = String::from_utf8(out.stdout).unwrap();
    assert!(proof.contains("\"producer\":\"jet-budget\""), "{proof}");
    assert!(proof.contains("\"budgetId\":\"package:public-api\""), "{proof}");
    assert!(proof.contains(&format!("\"reportId\":\"{report_id}\"")), "{proof}");
    assert!(proof.contains("\"deterministicBudget\":{\"failed\":0,\"met\":1,\"selected\":1"), "{proof}");
    assert_eq!(fs::read_dir(root.join(".jet/perf/reports")).unwrap().count(), before, "prove measured or wrote a report");
}

#[test]
fn prove_json_is_derived_from_real_front_end_and_sorted_target() {
    let root = workspace("report");
    fs::write(root.join("b.jet"), "fn b() => Int { return 2 }\n").unwrap();
    fs::write(root.join("a.jet"), "fn a() => Int { return 1 }\n").unwrap();
    let out = Command::new(jet()).current_dir(&root).args(["prove", ".", "--json"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    let report = String::from_utf8(out.stdout).unwrap();
    assert!(report.contains("\"kind\":\"workspace\""), "{report}");
    assert!(report.find("a.jet").unwrap() < report.find("b.jet").unwrap(), "{report}");
    assert!(report.contains("\"producer\":\"jet-sema\""), "{report}");
    assert!(report.contains("\"frontEnd\":{\"failed\":0,\"proved\":2,\"selected\":2"), "{report}");
}

#[test]
fn prove_front_end_failure_is_typed_evidence_and_exit_one() {
    let root = workspace("failure");
    fs::write(root.join("bad.jet"), "fn run() { missing() }\n").unwrap();
    let out = Command::new(jet()).current_dir(&root).args(["prove", "bad.jet", "--json"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stderr.is_empty(), "JSON mode leaked stderr: {}", String::from_utf8_lossy(&out.stderr));
    let report = String::from_utf8(out.stdout).unwrap();
    assert!(report.contains("\"result\":\"fail\""), "{report}");
    assert!(report.contains("\"outcome\":\"failed\""), "{report}");
    assert!(report.contains("\"diagnosticIndexes\":[0]"), "{report}");
    assert!(report.contains("\"code\":\"E0102\""), "{report}");
}

#[test]
fn prove_usage_and_missing_target_have_exact_exit_classes() {
    let root = workspace("usage");
    let usage = Command::new(jet()).current_dir(&root).arg("prove").output().unwrap();
    assert_eq!(usage.status.code(), Some(2));
    let missing = Command::new(jet()).current_dir(&root).args(["prove", "missing.jet"]).output().unwrap();
    assert_eq!(missing.status.code(), Some(1));
}

#[test]
fn prove_uses_canonical_package_marker_in_target_identity() {
    let root = workspace("canonical_package");
    fs::write(root.join("package.jet"), "name: \"demo\"\nversion: \"0.1.0\"\n").unwrap();
    fs::write(root.join("main.jet"), "fn run() {}\n").unwrap();
    let out = Command::new(jet())
        .current_dir(&root)
        .args(["prove", ".", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    let report = String::from_utf8(out.stdout).unwrap();
    assert!(report.contains("\"kind\":\"package\""), "{report}");
    assert!(report.contains("package.jet"), "canonical marker missing from identity: {report}");
}

#[test]
fn prove_capture_replay_round_trip_and_corruption_fail_closed() {
    let root = workspace("replay_round_trip");
    fs::write(root.join("main.jet"), "fn run() {}\n").unwrap();
    let artifact = root.join("capture.jetproof-replay");
    let artifact_arg = artifact.file_name().unwrap().to_string_lossy().to_string();
    let captured = Command::new(jet())
        .current_dir(&root)
        .args(["prove", "main.jet", &format!("--capture={artifact_arg}"), "--json"])
        .output()
        .unwrap();
    assert_eq!(
        captured.status.code(),
        Some(0),
        "stderr={} stdout={} artifact={}",
        String::from_utf8_lossy(&captured.stderr),
        String::from_utf8_lossy(&captured.stdout),
        artifact.display()
    );
    assert!(artifact.is_file());

    let replayed = Command::new(jet())
        .current_dir(&root)
        .args(["prove", "main.jet", "--replay", &artifact_arg, "--json"])
        .output()
        .unwrap();
    assert_eq!(replayed.status.code(), Some(0), "{}", String::from_utf8_lossy(&replayed.stderr));
    let report = String::from_utf8(replayed.stdout).unwrap();
    assert!(report.contains("\"facet\":\"replay\""), "{report}");

    let mut corrupt = fs::read(&artifact).unwrap();
    let middle = corrupt.len() / 2;
    corrupt[middle] ^= 1;
    fs::write(&artifact, corrupt).unwrap();
    let rejected = Command::new(jet())
        .current_dir(&root)
        .args(["prove", "main.jet", "--replay", &artifact_arg])
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("Error [E3622]"));
}

#[test]
fn prove_rejects_capture_path_escape_before_writing_an_artifact() {
    let root = workspace("capture_escape");
    fs::write(root.join("main.jet"), "fn run() {}\n").unwrap();
    let out = Command::new(jet())
        .current_dir(&root)
        .args(["prove", "main.jet", "--capture=../escape.jetproof-replay"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("Error [E3629]"));
    assert!(!root.parent().unwrap().join("escape.jetproof-replay").exists());
}

#[test]
fn prove_capture_does_not_finalize_after_front_end_failure() {
    let root = workspace("capture_front_end_failure");
    fs::write(root.join("bad.jet"), "fn run() { missing() }\n").unwrap();
    let artifact = root.join("failed.jetproof-replay");
    let out = Command::new(jet())
        .current_dir(&root)
        .args(["prove", "bad.jet", "--capture=failed.jetproof-replay"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(!artifact.exists(), "front-end failure must not leave a replay artifact");
}

#[test]
fn prove_capture_refuses_reachable_io_before_the_child_runs() {
    let root = workspace("capture_io_preflight");
    fs::write(root.join("main.jet"), "fn run() { print(\"not captured\") }\n").unwrap();
    let artifact = root.join("io.jetproof-replay");
    let out = Command::new(jet())
        .current_dir(&root)
        .args(["prove", "main.jet", "--capture=io.jetproof-replay"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("Error [E3627]"));
    assert!(!artifact.exists(), "preflight refusal must happen before artifact creation");
}

#[cfg(unix)]
#[test]
fn prove_rejects_a_symlink_target() {
    use std::os::unix::fs::symlink;

    let root = workspace("symlink_target");
    fs::write(root.join("real.jet"), "fn run() {}\n").unwrap();
    symlink(root.join("real.jet"), root.join("link.jet")).unwrap();
    let out = Command::new(jet())
        .current_dir(&root)
        .args(["prove", "link.jet"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("must not be a symlink"));
}

#[test]
fn prove_uses_structured_test_evidence_and_continues_after_failure() {
    let root = workspace("test_continuation");
    fs::write(
        root.join("a_fail.jet"),
        "#Test(\"first fails\") {\n    require(false, \"intentional\")\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("b_pass.jet"),
        "#Test(\"later passes\") {\n    require(true)\n}\n",
    )
    .unwrap();
    let out = Command::new(jet())
        .current_dir(&root)
        .args(["prove", ".", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{}", String::from_utf8_lossy(&out.stderr));
    let report = String::from_utf8(out.stdout).unwrap();
    assert!(report.contains("\"unit\":{\"failed\":1,\"passed\":1,\"selected\":2"), "{report}");
    assert!(report.contains("\"outcome\":\"failed\",\"producer\":\"jet-test\""), "{report}");
    assert!(report.contains("\"outcome\":\"passed\",\"producer\":\"jet-test\""), "{report}");
}

#[test]
fn prove_captures_contract_results_and_runtime_panics_structurally() {
    let root = workspace("contract_runtime");
    fs::write(
        root.join("a_contract_pass.jet"),
        "#Pre(value > 0, \"positive\") fn checked(value: Int) => Int { return value }\n#Test(\"contract pass\") { require_eq(checked(1), 1) }\n",
    ).unwrap();
    fs::write(
        root.join("b_contract_fail.jet"),
        "#Pre(value > 0, \"positive\") fn checked(value: Int) => Int { return value }\n#Test(\"contract fail\") { checked(0) }\n",
    ).unwrap();
    fs::write(
        root.join("c_panic.jet"),
        "#Test(\"runtime panic\") { panic(\"structured boom\") }\n",
    ).unwrap();
    fs::write(
        root.join("d_later.jet"),
        "#Test(\"later still runs\") { require(true) }\n",
    ).unwrap();

    let out = Command::new(jet()).current_dir(&root).args(["prove", ".", "--json"]).output().unwrap();
    assert_eq!(out.status.code(), Some(70), "stderr={} stdout={}", String::from_utf8_lossy(&out.stderr), String::from_utf8_lossy(&out.stdout));
    assert!(out.stderr.is_empty(), "JSON prove parsed/leaked terminal stderr");
    let report = String::from_utf8(out.stdout).unwrap();
    assert!(report.contains("\"kind\":\"contract\",\"outcome\":\"passed\""), "{report}");
    assert!(report.contains("\"kind\":\"contract\",\"outcome\":\"failed\""), "{report}");
    assert!(report.contains("\"code\":\"E3005\""), "{report}");
    assert!(report.contains("\"code\":\"E3001\""), "{report}");
    assert!(report.contains("structured boom"), "{report}");
    assert!(report.contains("\"path\":\"./d_later.jet\""), "later producer did not continue: {report}");
}

#[test]
fn prove_reports_real_property_cases_shrinks_and_continues() {
    let root = workspace("properties");
    fs::write(root.join("a_pass.jet"), "#Test fn identity(n: Int) { require_eq(n, n) }\n").unwrap();
    fs::write(root.join("b_shrink.jet"), "#Test fn always_small(n: Int) { require(n < 50) }\n").unwrap();
    fs::write(root.join("c_later.jet"), "#Test(\"later unit\") { require(true) }\n").unwrap();

    let out = Command::new(jet()).current_dir(&root).args(["prove", ".", "--json"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1), "stderr={} stdout={}", String::from_utf8_lossy(&out.stderr), String::from_utf8_lossy(&out.stdout));
    assert!(out.stderr.is_empty());
    let report = String::from_utf8(out.stdout).unwrap();
    assert!(report.contains("\"kind\":\"property\",\"outcome\":\"passed\""), "{report}");
    assert!(report.contains("\"kind\":\"property\",\"outcome\":\"failed\""), "{report}");
    assert!(report.contains("\"generatedCases\":200"), "passing property did not execute 200 cases: {report}");
    assert!(report.contains("\"shrinkTrace\":[{\"name\":\"minimized_inputs\",\"value\":\"n = 50\"}]"), "{report}");
    assert!(report.contains("\"path\":\"./c_later.jet\""), "later producer did not continue: {report}");
}

#[test]
fn prove_reports_real_doctests_and_continues() {
    let root = workspace("doctests");
    fs::write(root.join("a_pass.jet"), "/// ```jet\n/// 2 + 2 // => 4\n/// ```\nfn value() => Int { return 4 }\n").unwrap();
    fs::write(root.join("b_fail.jet"), "/// ```jet\n/// 2 + 2 // => 5\n/// ```\nfn value() => Int { return 4 }\n").unwrap();
    fs::write(root.join("c_later.jet"), "#Test(\"later unit\") { require(true) }\n").unwrap();
    let out = Command::new(jet()).current_dir(&root).args(["prove", ".", "--json"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1), "stderr={} stdout={}", String::from_utf8_lossy(&out.stderr), String::from_utf8_lossy(&out.stdout));
    assert!(out.stderr.is_empty());
    let report = String::from_utf8(out.stdout).unwrap();
    assert!(report.contains("\"kind\":\"doctest\",\"outcome\":\"passed\""), "{report}");
    assert!(report.contains("\"kind\":\"doctest\",\"outcome\":\"failed\""), "{report}");
    assert!(report.contains("\"doctest\":{\"failed\":1,\"passed\":1,\"selected\":2"), "{report}");
    assert!(report.contains("\"path\":\"./c_later.jet\""), "later producer did not continue: {report}");
}

#[test]
fn mandatory_env_init_does_not_pull_optional_core_env_surface() {
    let root = workspace("env_prelude");
    fs::write(root.join("plain.jet"), "fn run() {}\n").unwrap();
    let out = Command::new(jet())
        .current_dir(&root)
        .args(["emit", "--rust", "plain.jet"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    let rust = String::from_utf8(out.stdout).unwrap();
    assert!(rust.contains("fn jet_std_env_init()"), "{rust}");
    assert!(rust.contains("fn main() {\n    jet_std_env_init();"), "{rust}");
    assert!(!rust.contains("fn jet_std_env_get("), "optional Core env accessors leaked into plain program");
}

#[test]
fn unknown_lens_is_exact_e2941_in_human_and_json_modes() {
    let root = workspace("bad_lens");
    fs::write(root.join("plain.jet"), "fn run() {}\n").unwrap();
    let human = Command::new(jet())
        .current_dir(&root)
        .args(["prove", "plain.jet", "--lens", "test"])
        .output()
        .unwrap();
    assert_eq!(human.status.code(), Some(2));
    assert!(human.stdout.is_empty());
    assert_eq!(
        String::from_utf8(human.stderr).unwrap(),
        "Error [E2941]: unknown proof lens `test`\n Why: `jet prove` accepts all, refinements, effects, taint, contracts, tests, budgets, replay, solver\n Fix: try `jet prove plain.jet --lens tests`\n"
    );

    let machine = Command::new(jet())
        .current_dir(&root)
        .args(["prove", "plain.jet", "--lens=test", "--json"])
        .output()
        .unwrap();
    assert_eq!(machine.status.code(), Some(2));
    assert!(machine.stderr.is_empty());
    let json = String::from_utf8(machine.stdout).unwrap();
    assert!(json.contains("\"code\":\"E2941\""), "{json}");
    assert!(!json.contains("\"evidence\""), "malformed CLI emitted a ProofReport: {json}");
}

#[test]
fn prove_lens_shows_failed_unselected_evidence() {
    let root = workspace("lens_projection");
    fs::write(
        root.join("main.jet"),
        "#Test(\"outside\") { require(false, \"lens failure\") }\n",
    )
    .unwrap();
    let out = Command::new(jet())
        .current_dir(&root)
        .args(["prove", "main.jet", "--lens", "contracts"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("OUTSIDE SELECTED LENSES"), "{stdout}");
    assert!(stdout.contains("unit outside"), "{stdout}");
}

#[test]
fn prove_solver_lens_emits_checked_certificate_evidence() {
    let root = workspace("solver_lens");
    fs::write(
        root.join("checked.jet"),
        "#[Pre(value > 0, \"positive\"), Post(result == value, \"unchanged\")] fn checked(value: Int) => Int { return value }\n",
    )
    .unwrap();
    let out = Command::new(jet())
        .current_dir(&root)
        .args(["prove", "checked.jet", "--lens", "solver", "--json"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    let report = String::from_utf8(out.stdout).unwrap();
    assert!(report.contains("\"facet\":\"solver\""), "{report}");
    assert!(report.contains("\"status\":\"proved\""), "{report}");
    assert!(report.contains("\"solver\":{\"disproved\":0,\"proved\":1"), "{report}");
}
