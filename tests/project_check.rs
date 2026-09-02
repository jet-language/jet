//! Project-check witnesses for card #2389.
//!
//! These cases drive the real `jet check` command against frozen projects.  The
//! project proof rows are snapshotted from stdout, while induced failures use
//! stderr snapshots.  Paths are scrubbed because the fixture checkout location
//! is machine-specific; proof details and content-derived Core fingerprints
//! remain byte-exact.

use jet_foundation::JSON::{json_get, json_str, parse, JSONValue};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Instant;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/project_check/fixtures")
}

fn fixture(name: &str) -> PathBuf {
    fixture_root().join(name)
}

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

fn run_check(name: &str, args: &[&str]) -> Output {
    let dir = fixture(name);
    Command::new(jet_bin())
        .args(args)
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .output()
        .unwrap_or_else(|error| panic!("jet check {name} failed to start: {error}"))
}

fn run_uncached(name: &str, args: &[&str]) -> Output {
    let dir = fixture(name);
    Command::new(jet_bin())
        .args(args)
        .current_dir(&dir)
        .env("JET_RECEIPT_BYPASS", "1")
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .output()
        .unwrap_or_else(|error| panic!("jet {} {name} failed to start: {error}", args[0]))
}

fn scratch_root(name: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_nanos();
    let base = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".cache/jet-test-scratch"))
        .unwrap_or_else(|| PathBuf::from(".cache/jet-test-scratch"));
    let root = base.join(format!(
        "project-check-{name}-{}-{stamp}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create project-check scratch root");
    root
}

fn copy_receipt_fixture(root: &Path) {
    for relative in [
        "workspace.jet",
        ".jet/lock",
        "packages/app/package.jet",
        "packages/app/entry.jet",
        "packages/app/app/main.jet",
        "packages/app/.jet/lock",
        "packages/app/.jet/generated/input.jet",
    ] {
        let source = fixture("receipt_invalidation").join(relative);
        let destination = root.join(relative);
        fs::create_dir_all(
            destination
                .parent()
                .unwrap_or_else(|| panic!("fixture destination has no parent")),
        )
        .expect("create copied fixture parent");
        fs::copy(&source, &destination).unwrap_or_else(|error| {
            panic!(
                "copy receipt fixture {} -> {}: {error}",
                source.display(),
                destination.display()
            )
        });
    }
}

fn run_project_check(dir: &Path, receipt_dir: &Path, args: &[&str]) -> Output {
    Command::new(jet_bin())
        .args(args)
        .current_dir(dir)
        .env("JET_RECEIPT_DIR", receipt_dir)
        .env_remove("JET_RECEIPT_BYPASS")
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .output()
        .unwrap_or_else(|error| panic!("jet check in {} failed to start: {error}", dir.display()))
}

fn assert_project_check_passed(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn receipt_replayed(output: &Output) -> bool {
    String::from_utf8_lossy(&output.stderr).contains("ok: check current")
}

fn machine_rows(output: &Output) -> Vec<JSONValue> {
    assert_project_check_passed(output, "machine project check");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let report = parse(stdout.trim()).unwrap_or_else(|error| {
        panic!("project check --json emitted invalid JSON: {error}\n{stdout}")
    });
    assert_eq!(
        json_get(&report, "contract").and_then(json_str),
        Some("jet.check/v1"),
        "project check machine contract changed"
    );
    assert!(
        matches!(json_get(&report, "elapsed_ms"), Some(JSONValue::Number(milliseconds)) if *milliseconds >= 0),
        "project check machine result must record elapsed_ms"
    );
    match json_get(&report, "rows") {
        Some(JSONValue::Array(rows)) => rows.clone(),
        other => panic!("project check machine rows must be an array, got {other:?}"),
    }
}

fn scrub_fixture_path(text: &str, dir: &Path) -> String {
    let absolute = dir
        .canonicalize()
        .unwrap_or_else(|error| panic!("fixture {} is not canonical: {error}", dir.display()));
    text.replace(&*absolute.to_string_lossy(), "<fixture>")
}

fn proof_snapshot(output: &Output, dir: &Path) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut snapshot = stdout
        .lines()
        .filter(|line| line.starts_with("proof: "))
        .map(|line| scrub_fixture_path(line, dir))
        .collect::<Vec<_>>()
        .join("\n");
    if !snapshot.is_empty() {
        snapshot.push('\n');
    }
    snapshot
}

fn stderr_snapshot(output: &Output, dir: &Path) -> String {
    scrub_fixture_path(&String::from_utf8_lossy(&output.stderr), dir)
}

fn expected(name: &str, extension: &str) -> PathBuf {
    fixture(name).join(format!("expected.{extension}"))
}

fn assert_snapshot(name: &str, extension: &str, actual: &str) {
    let path = expected(name, extension);
    if std::env::var_os("UPDATE_EXPECT").is_some() {
        fs::write(&path, actual)
            .unwrap_or_else(|error| panic!("write snapshot {}: {error}", path.display()));
        return;
    }
    let want = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read snapshot {}: {error}", path.display()));
    assert_eq!(actual, want, "snapshot mismatch for {}", path.display());
}

fn assert_project_proof_rows(fixture_name: &str, proof: &str) {
    let rows = proof.lines().collect::<Vec<_>>();
    assert_eq!(
        rows.len(),
        4,
        "project check {fixture_name} must expose one row for each proof class:\n{proof}"
    );
    for (label, code) in [
        ("entry resolution", "E2389"),
        ("module graph", "E2390"),
        ("Core closure", "E2391"),
        ("tier lowering", "E2392"),
    ] {
        assert!(
            rows.iter()
                .any(|row| row.contains(label) && row.contains(code) && row.contains("[proven]")),
            "project check {fixture_name} did not prove {label} ({code}):\n{proof}"
        );
    }
    assert!(
        !proof.contains("absent from the checked module graph"),
        "clean project check reported an unresolved output:\n{proof}"
    );
}

fn assert_clean_project(name: &str) {
    let output = run_check(name, &["check", "."]);
    assert!(
        output.status.success(),
        "project check {name} failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let proof = proof_snapshot(&output, &fixture(name));
    assert_project_proof_rows(name, &proof);
    assert_snapshot(name, "stdout", &proof);
}

#[test]
fn explicit_in_package_file_keeps_file_scope() {
    let output = run_check("frozen_dogfood", &["check", "src/cli/main.jet"]);
    assert!(
        output.status.success(),
        "in-package check failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("scope=explicit-file"),
        "explicit in-package check changed scope:\n{stdout}"
    );
    let proof = proof_snapshot(&output, &fixture("frozen_dogfood"));
    let rows = proof.lines().collect::<Vec<_>>();
    assert_eq!(
        rows.len(),
        4,
        "explicit-file check must expose four non-project proof rows:\n{proof}"
    );
    assert!(
        rows.iter()
            .all(|row| row.contains("output=not-applicable") && row.contains("[not applicable]")),
        "explicit-file check exposed project proof:\n{proof}"
    );
}

#[test]
fn bare_project_check_enumerates_outputs_without_writing_artifacts() {
    let dir = fixture("entry_resolution");
    let package = dir.join("package.jet");
    let lock = dir.join(".jet/lock");
    let generated = dir.join(".jet/generated");
    let package_mtime = fs::metadata(&package)
        .unwrap_or_else(|error| panic!("package fixture metadata: {error}"))
        .modified()
        .unwrap_or_else(|error| panic!("package fixture mtime: {error}"));
    let lock_mtime = fs::metadata(&lock).ok().and_then(|metadata| metadata.modified().ok());
    let generated_mtime = fs::metadata(&generated)
        .ok()
        .and_then(|metadata| metadata.modified().ok());

    let output = run_check("entry_resolution", &["check"]);
    assert!(
        output.status.success(),
        "bare project check failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("scope=project"),
        "bare check did not use project scope:\n{stdout}"
    );
    let proof = proof_snapshot(&output, &dir);
    assert_project_proof_rows("entry_resolution bare", &proof);
    assert!(
        proof.lines().all(|row| row.contains("output=release")),
        "bare check did not enumerate the declared runnable output:\n{proof}"
    );

    let after_package_mtime = fs::metadata(&package)
        .unwrap_or_else(|error| panic!("package fixture metadata after check: {error}"))
        .modified()
        .unwrap_or_else(|error| panic!("package fixture mtime after check: {error}"));
    let after_lock_mtime = fs::metadata(&lock).ok().and_then(|metadata| metadata.modified().ok());
    let after_generated_mtime = fs::metadata(&generated)
        .ok()
        .and_then(|metadata| metadata.modified().ok());
    assert_eq!(package_mtime, after_package_mtime, "project check touched package.jet");
    assert_eq!(lock_mtime, after_lock_mtime, "project check touched .jet/lock");
    assert_eq!(
        generated_mtime, after_generated_mtime,
        "project check touched .jet/generated"
    );
}


#[test]
fn frozen_dogfood_and_each_project_proof_witness_are_snapshotted() {
    // The frozen package follows dogfood's package.jet -> entry.jet -> nested
    // source entry shape.  The other fixtures isolate the four proof classes:
    // named output entry resolution, a multi-hop module graph, and a direct
    // Core call.  All four clean checks must expose all named proof rows.
    for name in [
        "frozen_dogfood",
        "entry_resolution",
        "module_graph",
        "core_closure",
    ] {
        assert_clean_project(name);
    }
}

#[test]
fn missing_effect_facts_are_a_project_check_diagnostic() {
    let output = run_check("effect_facts", &["check", ".", "--gate=impure=allow"]);
    assert!(!output.status.success(), "E2391 witness unexpectedly passed");
    let stderr = stderr_snapshot(&output, &fixture("effect_facts"));
    assert!(stderr.contains("Error [E2391]"), "missing E2391:\n{stderr}");
    assert_snapshot("effect_facts", "stderr", &stderr);
}

#[test]
fn unsupported_web_tier_is_not_reported_clean() {
    let output = run_check("tier_lowering", &["check", ".", "--target=web"]);
    assert!(!output.status.success(), "E2392 witness unexpectedly passed");
    let stderr = stderr_snapshot(&output, &fixture("tier_lowering"));
    assert!(stderr.contains("Error [E2392]"), "missing E2392:\n{stderr}");
    assert_snapshot("tier_lowering", "stderr", &stderr);
}

#[test]
fn explicit_file_without_owning_project_context_teaches_missing_context() {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "jet-project-check-missing-context-{}-{stamp}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create isolated missing-context fixture");
    let source = fs::read_to_string(fixture("missing_context").join("orphan.jet"))
        .expect("read missing-context fixture source");
    fs::write(root.join("orphan.jet"), source).expect("write isolated missing-context source");

    let output = Command::new(jet_bin())
        .args(["check", "orphan.jet"])
        .current_dir(&root)
        .env("JET_RECEIPT_BYPASS", "1")
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .output()
        .expect("jet check missing_context failed to start");
    assert!(!output.status.success(), "E2393 witness unexpectedly passed");
    let stderr = stderr_snapshot(&output, &root);
    assert!(stderr.contains("Error [E2393]"), "missing E2393:\n{stderr}");
    assert!(
        stderr.contains("use project.<module>"),
        "missing canonical import teaching:\n{stderr}"
    );
    assert_snapshot("missing_context", "stderr", &stderr);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn clean_project_check_then_default_run_and_aot_build_stay_clean() {
    for name in [
        "frozen_dogfood",
        "entry_resolution",
        "module_graph",
        "core_closure",
    ] {
        let check = run_uncached(name, &["check"]);
        assert!(
            check.status.success(),
            "project check {name} failed:\n{}",
            String::from_utf8_lossy(&check.stderr)
        );

        let run = run_uncached(name, &["run"]);
        assert!(
            run.status.success(),
            "default-tier run after project check {name} failed:\n{}",
            String::from_utf8_lossy(&run.stderr)
        );

        let build = run_uncached(name, &["build"]);
        assert!(
            build.status.success(),
            "AOT build after project check {name} failed:\n{}",
            String::from_utf8_lossy(&build.stderr)
        );
    }
}

#[test]
fn explicit_file_uses_owning_project_context() {
    let output = run_uncached("owning_context", &["check", "src/main.jet"]);
    assert!(
        output.status.success(),
        "explicit in-package project import failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("scope=explicit-file"),
        "explicit owning-context check changed scope:\n{stdout}"
    );
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("E2393"),
        "owning project context was treated as absent:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let proof = proof_snapshot(&output, &fixture("owning_context"));
    assert_snapshot("owning_context", "stdout", &proof);
}

#[test]
fn extension_optional_check_replays_receipt() {
    let dir = fixture("frozen_dogfood");
    let receipt_dir = std::env::temp_dir().join(format!(
        "jet-project-check-replay-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before Unix epoch")
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&receipt_dir);

    let run = || {
        Command::new(jet_bin())
            .args(["check", "src/cli/main"])
            .current_dir(&dir)
            .env("JET_RECEIPT_DIR", &receipt_dir)
            .env_remove("JET_RECEIPT_BYPASS")
            .env("NO_COLOR", "1")
            .output()
            .expect("extension-optional project check failed to start")
    };

    let first = run();
    assert!(
        first.status.success(),
        "extension-optional project check failed:\n{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let second = run();
    assert!(
        second.status.success(),
        "replayed extension-optional project check failed:\n{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(
        first.stdout, second.stdout,
        "receipt replay changed extension-optional check output"
    );
    assert!(
        String::from_utf8_lossy(&second.stderr).contains("ok: check current"),
        "extension-optional invocation did not replay its receipt:\n{}",
        String::from_utf8_lossy(&second.stderr)
    );
    let _ = fs::remove_dir_all(&receipt_dir);
}

#[test]
fn project_check_receipt_tracks_entry_and_authority_changes() {
    let root = scratch_root("receipt-invalidation");
    copy_receipt_fixture(&root);
    let package_root = root.join("packages/app");
    let receipt_dir = root.join("receipts");
    let check = || run_project_check(&package_root, &receipt_dir, &["check"]);

    let first = check();
    assert_project_check_passed(&first, "initial receipt project check");
    let replay = check();
    assert_project_check_passed(&replay, "unchanged receipt project check");
    assert!(
        receipt_replayed(&replay),
        "unchanged project check did not replay its receipt:\n{}",
        String::from_utf8_lossy(&replay.stderr)
    );

    let candidate = package_root.join("run.jet");
    fs::write(&candidate, "fn run() { print(\"candidate\") }\n")
        .expect("write higher-priority entry candidate");
    let candidate_check = check();
    assert_project_check_passed(&candidate_check, "entry-candidate project check");
    assert!(
        !receipt_replayed(&candidate_check),
        "new entry candidate reused stale receipt:\n{}",
        String::from_utf8_lossy(&candidate_check.stderr)
    );
    fs::remove_file(&candidate).expect("remove higher-priority entry candidate");

    let mutations = [
        (
            "workspace",
            root.join("workspace.jet"),
            "module workspace {\n    members: [\"packages/app\"]\n}\n",
        ),
        (
            "output",
            package_root.join("package.jet"),
            "name: \"receipt-probe\"\nversion: \"0.1.0\"\noutputs: { release_alt: .Executable{ entry: app.launch } }\ndefaults: { run: release_alt }\n",
        ),
        ("lock", root.join(".jet/lock"), "workspace-lock-v2\n"),
        (
            "generated",
            package_root.join(".jet/generated/input.jet"),
            "fn generated_input_v2() {}\n",
        ),
    ];
    for (label, path, replacement) in mutations {
        let original = fs::read(&path)
            .unwrap_or_else(|error| panic!("read {label} fixture {}: {error}", path.display()));
        let mutation_receipt_dir = root.join(format!("receipts-{label}"));
        let mutation_check =
            || run_project_check(&package_root, &mutation_receipt_dir, &["check"]);

        let baseline = mutation_check();
        assert_project_check_passed(&baseline, &format!("baseline {label} project check"));
        let baseline_replay = mutation_check();
        assert_project_check_passed(
            &baseline_replay,
            &format!("unchanged baseline {label} project check"),
        );
        assert!(
            receipt_replayed(&baseline_replay),
            "unchanged baseline {label} check did not replay its receipt:\n{}",
            String::from_utf8_lossy(&baseline_replay.stderr)
        );

        fs::write(&path, replacement)
            .unwrap_or_else(|error| panic!("mutate {label} fixture {}: {error}", path.display()));
        let changed = mutation_check();
        assert_project_check_passed(&changed, &format!("changed {label} project check"));
        assert!(
            !receipt_replayed(&changed),
            "{label} change reused stale receipt:\n{}",
            String::from_utf8_lossy(&changed.stderr)
        );

        fs::write(&path, original)
            .unwrap_or_else(|error| panic!("restore {label} fixture {}: {error}", path.display()));
        let _ = fs::remove_dir_all(mutation_receipt_dir);
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn project_check_json_rows_are_versioned_and_warm_stable() {
    let scratch = scratch_root("json-warm");
    let receipt_dir = scratch.join("receipts");
    let dir = fixture("entry_resolution");

    let cold_started = Instant::now();
    let cold = run_project_check(&dir, &receipt_dir, &["check", "--json"]);
    let cold_us = cold_started.elapsed().as_micros();
    let cold_rows = machine_rows(&cold);

    let warm_started = Instant::now();
    let warm = run_project_check(&dir, &receipt_dir, &["check", "--json"]);
    let warm_us = warm_started.elapsed().as_micros();
    let warm_rows = machine_rows(&warm);
    assert!(
        receipt_replayed(&warm),
        "warm project check did not replay its receipt:\n{}",
        String::from_utf8_lossy(&warm.stderr)
    );

    assert_eq!(cold_rows, warm_rows, "warm check changed machine proof rows");
    assert_eq!(cold_rows.len(), 4, "machine output must expose all proof rows");
    for (name, diagnostic) in [
        ("entry resolution", "E2389"),
        ("module graph", "E2390"),
        ("Core closure", "E2391"),
        ("tier lowering", "E2392"),
    ] {
        let row = cold_rows
            .iter()
            .find(|row| json_get(row, "name").and_then(json_str) == Some(name))
            .unwrap_or_else(|| panic!("machine output omitted {name} row"));
        assert_eq!(
            json_get(row, "status").and_then(json_str),
            Some("proven"),
            "machine output did not prove {name}"
        );
        assert_eq!(
            json_get(row, "diagnostic").and_then(json_str),
            Some(diagnostic),
            "machine output changed diagnostic code for {name}"
        );
    }
    assert!(
        cold_us > 0 && warm_us > 0,
        "warm canary must record positive wall-clock cost: cold={cold_us}us warm={warm_us}us"
    );
    eprintln!(
        "project-check warm canary: contract=jet.check/v1 cone=D-DEVR-CONE1 rows={} cold_us={cold_us} warm_us={warm_us} rows_stable=true",
        cold_rows.len()
    );
    let _ = fs::remove_dir_all(scratch);
}

