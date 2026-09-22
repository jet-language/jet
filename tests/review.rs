use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn scratch(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("jet_review_{name}_{}", std::process::id()))
}

fn write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, text).unwrap();
}
const REVIEW_SOURCE: &str = "fn run() {}\n";

fn project(root: &Path, side: &str, allow: &str, source: &str) -> PathBuf {
    let dir = root.join(side);
    let manifest = if allow.is_empty() {
        "name: \"review_fixture\"\nversion: \"0.1.0\"\nedition: \"2026\"\n".to_string()
    } else {
        format!(
            "name: \"review_fixture\"\nversion: \"0.1.0\"\nedition: \"2026\"\nauthority: {{ holds: {{ allow: [{allow}] }} }}\n"
        )
    };
    write(&dir.join("package.jet"), &manifest);
    write(&dir.join("run.jet"), source);
    dir
}

fn review_command(
    base: &Path,
    head: &Path,
    base_receipt: Option<&Path>,
    head_receipt: Option<&Path>,
) -> std::process::Output {
    let mut args = vec![
        "review".to_string(),
        base.join("run.jet").to_str().unwrap().to_string(),
        head.join("run.jet").to_str().unwrap().to_string(),
        "--json".to_string(),
    ];
    if let Some(path) = base_receipt {
        args.extend(["--base-receipt".to_string(), path.to_str().unwrap().to_string()]);
    }
    if let Some(path) = head_receipt {
        args.extend(["--receipt".to_string(), path.to_str().unwrap().to_string()]);
    }
    Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .unwrap()
}

fn diagnostic_review_command(base: &Path, head: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_jet"))
        .args([
            "review",
            base.join("run.jet").to_str().unwrap(),
            head.join("run.jet").to_str().unwrap(),
            "--diagnostics",
            "--json",
        ])
        .env("NO_COLOR", "1")
        .output()
        .unwrap()
}

fn checked_derivation(
    id: &str,
    build: &str,
    run: &str,
    target: &str,
    event: &str,
) -> String {
    format!(
        r#"{{"id":"{id}","subject":"value","claim":"value:contract","producer":"test","method":"formal_proof","rule":"same-inputs","premises":["schedule:v1"],"identity":{{"source":"review-source","build":"{build}","run":"{run}","target":"{target}"}}, "assumptions":["finite-input"],"disposition":"current","observation":{{"event":"{event}","counterexample":null}}}}"#
    )
}

fn proof_evidence(
    claim_id: &str,
    derivation_id: &str,
    outcome: &str,
    state: &str,
    target: &str,
    argument: i64,
    schedule: &str,
) -> String {
    format!(
        r#"{{"id":"{claim_id}","claimId":"{claim_id}","kind":"contract","facet":"contracts","producer":"test","outcome":"{outcome}","state":"{state}","contract":{{"marker":"review","observation":"reached"}},"inputs":{{"arg":{argument}}},"environment":{{"target":"{target}"}}, "premises":["{schedule}"],"derivation":{{"id":"{derivation_id}"}}}}"#
    )
}

fn proof_receipt(derivations: &[String], evidence: &[String]) -> String {
    format!(
        r#"{{"proofReport":{{"derivations":[{}],"evidence":[{}]}}}}"#,
        derivations.join(","),
        evidence.join(",")
    )
}

fn run_receipt_case(
    name: &str,
    base_receipt: Option<&str>,
    head_receipt: Option<&str>,
    base_allow: &str,
    head_allow: &str,
    expected: &[&str],
) {
    let root = scratch(name);
    let _ = fs::remove_dir_all(&root);
    let base = project(&root, "base", base_allow, REVIEW_SOURCE);
    let head = project(&root, "head", head_allow, REVIEW_SOURCE);
    let base_receipt_path = base_receipt.map(|_| root.join("base.jetproof"));
    let head_receipt_path = head_receipt.map(|_| root.join("head.jetproof"));
    if let (Some(path), Some(contents)) = (&base_receipt_path, base_receipt) {
        write(path, contents);
    }
    if let (Some(path), Some(contents)) = (&head_receipt_path, head_receipt) {
        write(path, contents);
    }
    let output = review_command(
        &base,
        &head,
        base_receipt_path.as_deref(),
        head_receipt_path.as_deref(),
    );
    let diagnostic = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.status.success(),
        "review fixture `{name}` failed: {diagnostic}"
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    for needle in expected {
        assert!(
            stdout.contains(needle),
            "review fixture `{name}` omitted `{needle}`:\n{stdout}"
        );
    }
    let _ = fs::remove_dir_all(root);
}

fn assert_receipt_refused(name: &str, receipt: &str, reason: &str) {
    let root = scratch(name);
    let _ = fs::remove_dir_all(&root);
    let base = project(&root, "base", "", REVIEW_SOURCE);
    let head = project(&root, "head", "", REVIEW_SOURCE);
    let receipt_path = root.join("invalid.jetproof");
    write(&receipt_path, receipt);
    let output = review_command(&base, &head, Some(&receipt_path), None);
    assert!(
        !output.status.success(),
        "receipt fixture `{name}` unexpectedly succeeded: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let diagnostic = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        diagnostic.contains("E2105"),
        "receipt fixture `{name}` omitted E2105:\n{diagnostic}"
    );
    assert!(
        diagnostic.contains(reason),
        "receipt fixture `{name}` omitted `{reason}`:\n{diagnostic}"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn review_joins_meaning_authority_and_receipt_changes() {
    let root = scratch("joins");
    let _ = fs::remove_dir_all(&root);
    let base = root.join("base");
    let head = root.join("head");
    let manifest = |allow: &str| {
        format!(
            "name: \"review_fixture\"\nversion: \"0.1.0\"\nedition: \"2026\"\nauthority: {{ holds: {{ allow: [{allow}] }} }}\n"
        )
    };
    write(&base.join("package.jet"), &manifest("FS, IO"));
    write(&head.join("package.jet"), &manifest("FS, IO, Net"));
    write(&base.join("run.jet"), "fn run() { print(\"base\") }\n");
    write(&head.join("run.jet"), "fn run() { print(\"head\") }\n");

    let base_receipt = root.join("base.jetproof");
    let head_receipt = root.join("head.jetproof");
    let base_receipt_value = proof_receipt(
        &[
            checked_derivation("d-retained", "build-1", "run-1", "target-1", "event-retained"),
            checked_derivation("d-lost", "build-1", "run-1", "target-1", "event-lost"),
        ],
        &[
            proof_evidence(
                "claim-retained",
                "d-retained",
                "passed",
                "checked",
                "target-1",
                1,
                "schedule:v1",
            ),
            proof_evidence(
                "claim-lost",
                "d-lost",
                "passed",
                "checked",
                "target-1",
                1,
                "schedule:v1",
            ),
        ],
    );
    let head_receipt_value = proof_receipt(
        &[
            checked_derivation(
                "d-retained",
                "build-1",
                "run-1",
                "target-1",
                "event-retained",
            ),
            checked_derivation("d-gained", "build-1", "run-1", "target-1", "event-gained"),
        ],
        &[
            proof_evidence(
                "claim-retained",
                "d-retained",
                "passed",
                "checked",
                "target-1",
                1,
                "schedule:v1",
            ),
            proof_evidence(
                "claim-gained",
                "d-gained",
                "passed",
                "checked",
                "target-1",
                1,
                "schedule:v1",
            ),
        ],
    );
    write(&base_receipt, &base_receipt_value);
    write(&head_receipt, &head_receipt_value);

    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args([
            "review",
            base.join("run.jet").to_str().unwrap(),
            head.join("run.jet").to_str().unwrap(),
            "--base-receipt",
            base_receipt.to_str().unwrap(),
            "--receipt",
            head_receipt.to_str().unwrap(),
            "--json",
        ])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "review failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        jet_foundation::MachineOutput::read_machine_output(&stdout).unwrap(),
        vec![jet_foundation::MachineOutput::MachineRecord::Status]
    );
    assert!(stdout.starts_with("{\"schema\":\"jet.status/v1\""));
    assert!(stdout.contains("\"kind\":\"review\""));
    assert!(stdout.contains("body_changed"));
    assert!(stdout.contains("\"status\":\"widened\""));
    assert!(stdout.contains("\"status\":\"lost\""));
    assert!(stdout.contains("\"status\":\"gained\""));
    assert!(!stdout.contains("text_diff"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn comment_only_change_has_no_semantic_operation() {
    let root = scratch("comments");
    let _ = fs::remove_dir_all(&root);
    let before = root.join("before.jet");
    let after = root.join("after.jet");
    write(&before, "fn run() { print(\"same\") }\n");
    write(&after, "// review comment\nfn run() { print(\"same\") }\n");

    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args([
            "review",
            before.to_str().unwrap(),
            after.to_str().unwrap(),
            "--json",
        ])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "review failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("\"semantic_ops\":[]"));
    assert!(stdout.contains("\"changes\":[]"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn review_uses_a_recorded_rename_and_ignores_hand_spelling() {
    let root = scratch("recorded_rename");
    let _ = fs::remove_dir_all(&root);
    let base = root.join("base");
    let head = root.join("head");
    let package = "name: \"review_rename\"\nversion: \"0.1.0\"\nedition: \"2026\"\n";
    write(&base.join("package.jet"), package);
    write(&head.join("package.jet"), package);
    let before = "fn report() -> Int {
    return 1
}
";
    let after = "fn summarize() -> Int {
    return 1
}
";
    write(&base.join("run.jet"), before);
    write(&head.join("run.jet"), after);
    let before_hash = jet::SHA256::sha256_hex(before.as_bytes());
    let after_hash = jet::SHA256::sha256_hex(after.as_bytes());
    let receipt_dir = head.join(".jet/codemods");
    fs::create_dir_all(&receipt_dir).unwrap();
    write(
        &receipt_dir.join("rename.log.json"),
        &format!(
            "{{\"schema\":2,\"semantic_ops\":[{{\"kind\":\"rename\",\"from\":\"report\",\"to\":\"summarize\"}}],\"files\":[{{\"path\":\"{}\",\"before_hash\":\"{}\",\"after_hash\":\"{}\"}}]}}",
            head.join("run.jet").display(),
            before_hash,
            after_hash,
        ),
    );
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args([
            "review",
            base.join("run.jet").to_str().unwrap(),
            head.join("run.jet").to_str().unwrap(),
            "--json",
        ])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(output.status.success(), "review failed: {:?}", output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("\"kind\":\"renamed\""), "{stdout}");

    let hand = root.join("hand");
    write(&hand.join("package.jet"), package);
    write(&hand.join("run.jet"), after);
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args([
            "review",
            base.join("run.jet").to_str().unwrap(),
            hand.join("run.jet").to_str().unwrap(),
            "--json",
        ])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(output.status.success(), "review failed: {:?}", output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!stdout.contains("\"kind\":\"renamed\""), "{stdout}");

    let _ = fs::remove_dir_all(root);
}
#[test]
fn review_receipt_fixture_matrix_distinguishes_matching_stale_missing_mismatched_and_changed() {
    let base_derivation = checked_derivation("d-shared", "build-1", "run-1", "target-1", "event-1");
    let matching_evidence =
        proof_evidence("claim-shared", "d-shared", "passed", "checked", "target-1", 1, "schedule:v1");
    let matching = proof_receipt(
        std::slice::from_ref(&base_derivation),
        std::slice::from_ref(&matching_evidence),
    );
    run_receipt_case(
        "matrix-matching",
        Some(&matching),
        Some(&matching),
        "",
        "",
        &[
            "\"base_recorded\":true",
            "\"head_recorded\":true",
            "\"retained\":1",
            "\"preserved_checked\"",
            "\"verdict\":\"reviewable\"",
        ],
    );

    let stale_derivation =
        checked_derivation("d-shared", "build-2", "run-1", "target-1", "event-1");
    let stale = proof_receipt(
        std::slice::from_ref(&stale_derivation),
        std::slice::from_ref(&matching_evidence),
    );
    run_receipt_case(
        "matrix-stale",
        Some(&matching),
        Some(&stale),
        "",
        "",
        &[
            "\"unknown\"",
            "build identity differs",
            "\"verdict\":\"reviewable\"",
        ],
    );

    run_receipt_case(
        "matrix-missing",
        None,
        None,
        "",
        "",
        &[
            "\"base_recorded\":false",
            "\"head_recorded\":false",
            "\"gained\":0",
            "\"lost\":0",
            "\"verdict\":\"reviewable\"",
        ],
    );

    let mismatched_evidence =
        proof_evidence("claim-shared", "d-shared", "passed", "checked", "target-1", 2, "schedule:v1");
    let mismatched = proof_receipt(
        std::slice::from_ref(&base_derivation),
        std::slice::from_ref(&mismatched_evidence),
    );
    run_receipt_case(
        "matrix-mismatched",
        Some(&matching),
        Some(&mismatched),
        "",
        "",
        &[
            "\"unknown\"",
            "declared inputs differ",
            "\"verdict\":\"reviewable\"",
        ],
    );

    let changed_evidence =
        proof_evidence("claim-shared", "d-shared", "failed", "checked", "target-1", 1, "schedule:v1");
    let changed = proof_receipt(
        std::slice::from_ref(&base_derivation),
        std::slice::from_ref(&changed_evidence),
    );
    run_receipt_case(
        "matrix-changed",
        Some(&matching),
        Some(&changed),
        "",
        "",
        &[
            "\"changed\":1",
            "\"first_difference\"",
            "first differing comparable event or result",
            "\"verdict\":\"reviewable\"",
        ],
    );
}

#[test]
fn review_receipt_fixture_matrix_tracks_gained_lost_changed_retained_and_authority_widening() {
    let retained_derivation =
        checked_derivation("d-retained", "build-1", "run-1", "target-1", "event-retained");
    let changed_derivation =
        checked_derivation("d-changed", "build-1", "run-1", "target-1", "event-changed");
    let lost_derivation =
        checked_derivation("d-lost", "build-1", "run-1", "target-1", "event-lost");
    let gained_derivation =
        checked_derivation("d-gained", "build-1", "run-1", "target-1", "event-gained");
    let base = proof_receipt(
        &[
            retained_derivation.clone(),
            changed_derivation.clone(),
            lost_derivation.clone(),
        ],
        &[
            proof_evidence(
                "claim-retained",
                "d-retained",
                "passed",
                "checked",
                "target-1",
                1,
                "schedule:v1",
            ),
            proof_evidence(
                "claim-changed",
                "d-changed",
                "passed",
                "checked",
                "target-1",
                1,
                "schedule:v1",
            ),
            proof_evidence(
                "claim-lost",
                "d-lost",
                "passed",
                "checked",
                "target-1",
                1,
                "schedule:v1",
            ),
        ],
    );
    let head = proof_receipt(
        &[
            retained_derivation,
            changed_derivation,
            gained_derivation,
        ],
        &[
            proof_evidence(
                "claim-retained",
                "d-retained",
                "passed",
                "checked",
                "target-1",
                1,
                "schedule:v1",
            ),
            proof_evidence(
                "claim-changed",
                "d-changed",
                "failed",
                "checked",
                "target-1",
                1,
                "schedule:v1",
            ),
            proof_evidence(
                "claim-gained",
                "d-gained",
                "passed",
                "checked",
                "target-1",
                1,
                "schedule:v1",
            ),
        ],
    );
    run_receipt_case(
        "matrix-gained-lost",
        Some(&base),
        Some(&head),
        "",
        "",
        &[
            "\"gained\":1",
            "\"lost\":1",
            "\"changed\":1",
            "\"retained\":1",
            "\"verdict\":\"proof lost\"",
        ],
    );

    let retained = proof_receipt(
        &[checked_derivation(
            "d-authority",
            "build-1",
            "run-1",
            "target-1",
            "event-authority",
        )],
        &[proof_evidence(
            "claim-authority",
            "d-authority",
            "passed",
            "checked",
            "target-1",
            1,
            "schedule:v1",
        )],
    );
    run_receipt_case(
        "matrix-authority-retained",
        Some(&retained),
        Some(&retained),
        "FS",
        "FS, Net",
        &[
            "\"status\":\"widened\"",
            "\"lost\":0",
            "\"verdict\":\"authority widened\"",
        ],
    );

    let lost = proof_receipt(&[], &[]);
    run_receipt_case(
        "matrix-authority-lost",
        Some(&retained),
        Some(&lost),
        "FS",
        "FS, Net",
        &[
            "\"status\":\"widened\"",
            "\"lost\":1",
            "\"verdict\":\"authority widened and proof lost\"",
        ],
    );
}

#[test]
fn review_receipt_fixture_matrix_refuses_duplicate_claim_and_derivation_ids() {
    let first_derivation =
        checked_derivation("d-first", "build-1", "run-1", "target-1", "event-first");
    let second_derivation =
        checked_derivation("d-second", "build-1", "run-1", "target-1", "event-second");
    let duplicate_claim = proof_receipt(
        &[first_derivation.clone(), second_derivation],
        &[
            proof_evidence(
                "claim-duplicate",
                "d-first",
                "passed",
                "checked",
                "target-1",
                1,
                "schedule:v1",
            ),
            proof_evidence(
                "claim-duplicate",
                "d-first",
                "passed",
                "checked",
                "target-1",
                1,
                "schedule:v1",
            ),
        ],
    );
    assert_receipt_refused(
        "matrix-duplicate-claim",
        &duplicate_claim,
        "duplicate evidence claim",
    );

    let duplicate_derivation = proof_receipt(
        &[first_derivation.clone(), first_derivation],
        &[proof_evidence(
            "claim-duplicate-derivation",
            "d-first",
            "passed",
            "checked",
            "target-1",
            1,
            "schedule:v1",
        )],
    );
    assert_receipt_refused(
        "matrix-duplicate-derivation",
        &duplicate_derivation,
        "duplicate derivation ids",
    );
}

#[test]
fn review_receipt_fixture_matrix_refuses_a_missing_receipt_path() {
    let root = scratch("matrix-missing-path");
    let _ = fs::remove_dir_all(&root);
    let base = project(&root, "base", "", REVIEW_SOURCE);
    let head = project(&root, "head", "", REVIEW_SOURCE);
    let missing = root.join("missing.jetproof");
    let output = review_command(&base, &head, Some(&missing), None);
    assert!(
        !output.status.success(),
        "missing receipt path unexpectedly succeeded: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let diagnostic = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(diagnostic.contains("E2105"), "{diagnostic}");
    assert!(diagnostic.contains("Could not read receipt"), "{diagnostic}");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn review_operation_fixture_matrix_ignores_changed_hash_receipts() {
    let root = scratch("matrix-changed-hash");
    let _ = fs::remove_dir_all(&root);
    let base_source = "fn report() -> Int {\n    return 1\n}\n";
    let head_source = "fn summarize() -> Int {\n    return 1\n}\n";
    let base = project(&root, "base", "", base_source);
    let head = project(&root, "head", "", head_source);
    let after_hash = jet::SHA256::sha256_hex(head_source.as_bytes());
    write(
        &head.join(".jet/codemods/rename.log.json"),
        &format!(
            "{{\"schema\":2,\"semantic_ops\":[{{\"kind\":\"rename\",\"from\":\"report\",\"to\":\"summarize\"}}],\"files\":[{{\"path\":\"{}\",\"before_hash\":\"stale-before-hash\",\"after_hash\":\"{}\"}}]}}",
            head.join("run.jet").display(),
            after_hash,
        ),
    );
    let output = review_command(&base, &head, None, None);
    assert!(
        output.status.success(),
        "changed-hash operation fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        !stdout.contains("\"kind\":\"renamed\""),
        "stale operation receipt was accepted:\n{stdout}"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn review_observation_identity_mismatches_remain_unknown() {
    let derivation = |build: &str, run: &str, target: &str, premise: &str| {
        format!(
            r#"{{"id":"d-shared","subject":"value","claim":"value:contract","producer":"test","method":"formal_proof","rule":"same-inputs","premises":["{premise}"],"identity":{{"source":"review-source","build":"{build}","run":"{run}","target":"{target}"}}, "assumptions":["finite-input"],"disposition":"current","observation":{{"event":"event-1","counterexample":null}}}}"#
        )
    };
    let evidence = |contract: &str, target: &str, premise: &str| {
        format!(
            r#"{{"id":"claim-shared","claimId":"claim-shared","kind":"contract","facet":"contracts","producer":"test","outcome":"passed","state":"checked","contract":{{"marker":"{contract}","observation":"reached"}},"inputs":{{"arg":1}},"environment":{{"target":"{target}"}},"premises":["{premise}"],"derivation":{{"id":"d-shared"}}}}"#
        )
    };
    let run_case = |name: &str,
                    base_derivation: String,
                    head_derivation: String,
                    base_evidence: String,
                    head_evidence: String,
                    reason: &str| {
        let base = proof_receipt(&[base_derivation], &[base_evidence]);
        let head = proof_receipt(&[head_derivation], &[head_evidence]);
        run_receipt_case(name, Some(&base), Some(&head), "", "", &["\"unknown\"", reason]);
    };

    run_case(
        "observation-contract-mismatch",
        derivation("build-1", "run-1", "target-1", "schedule:v1"),
        derivation("build-1", "run-1", "target-1", "schedule:v1"),
        evidence("contract-a", "target-1", "schedule:v1"),
        evidence("contract-b", "target-1", "schedule:v1"),
        "observation contract differs",
    );
    run_case(
        "observation-environment-mismatch",
        derivation("build-1", "run-1", "target-1", "schedule:v1"),
        derivation("build-1", "run-1", "target-1", "schedule:v1"),
        evidence("contract", "target-1", "schedule:v1"),
        evidence("contract", "target-2", "schedule:v1"),
        "relevant environment identity differs",
    );
    run_case(
        "observation-premise-mismatch",
        derivation("build-1", "run-1", "target-1", "schedule:v1"),
        derivation("build-1", "run-1", "target-1", "schedule:v2"),
        evidence("contract", "target-1", "schedule:v1"),
        evidence("contract", "target-1", "schedule:v1"),
        "numerical or scheduling premises differ",
    );
    run_case(
        "observation-build-mismatch",
        derivation("build-1", "run-1", "target-1", "schedule:v1"),
        derivation("build-2", "run-1", "target-1", "schedule:v1"),
        evidence("contract", "target-1", "schedule:v1"),
        evidence("contract", "target-1", "schedule:v1"),
        "build identity differs",
    );
    run_case(
        "observation-run-mismatch",
        derivation("build-1", "run-1", "target-1", "schedule:v1"),
        derivation("build-1", "run-2", "target-1", "schedule:v1"),
        evidence("contract", "target-1", "schedule:v1"),
        evidence("contract", "target-1", "schedule:v1"),
        "run identity differs",
    );
    run_case(
        "observation-target-mismatch",
        derivation("build-1", "run-1", "target-1", "schedule:v1"),
        derivation("build-1", "run-1", "target-2", "schedule:v1"),
        evidence("contract", "target-1", "schedule:v1"),
        evidence("contract", "target-2", "schedule:v1"),
        "target identity differs",
    );
}

#[test]
fn review_aligns_signature_changes_by_checked_identity() {
    let root = scratch("signature-change");
    let _ = fs::remove_dir_all(&root);
    let base = project(
        &root,
        "base",
        "",
        "fn report() -> Int {\n    return 1\n}\n",
    );
    let head = project(
        &root,
        "head",
        "",
        "fn report(value: Int) -> Int {\n    return value\n}\n",
    );

    let output = review_command(&base, &head, None, None);
    assert!(output.status.success(), "signature review failed");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("\"kind\":\"signature_changed\""), "{stdout}");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn review_receipts_bind_to_sides_and_do_not_infer_proof() {
    let derivation = |id: &str, method: &str, disposition: &str| {
        format!(
            r#"{{"id":"{id}","subject":"value","claim":"value:contract","producer":"test","method":"{method}","rule":"same-inputs","premises":["schedule:v1"],"identity":{{"source":"review-source","build":"build-1","run":"run-1","target":"target-1"}}, "assumptions":["finite-input"],"disposition":"{disposition}","observation":{{"event":"event-1","counterexample":null}}}}"#
        )
    };
    let evidence = |claim_id: &str, derivation_id: &str| {
        format!(
            r#"{{"id":"{claim_id}","claimId":"{claim_id}","kind":"contract","facet":"contracts","producer":"test","outcome":"passed","state":"proved","contract":{{"marker":"review","observation":"reached"}},"inputs":{{"arg":1}},"environment":{{"target":"target-1"}},"premises":["schedule:v1"],"derivation":{{"id":"{derivation_id}"}}}}"#
        )
    };

    let root = scratch("receipt-binding");
    let _ = fs::remove_dir_all(&root);
    let base = project(&root, "base", "", REVIEW_SOURCE);
    let head = project(&root, "head", "", REVIEW_SOURCE);
    let base_receipt = root.join("base.jetproof");
    let head_receipt = root.join("head.jetproof");
    write(
        &base_receipt,
        &proof_receipt(
            &[derivation("base-derivation", "formal_proof", "current")],
            &[evidence("base-claim", "base-derivation")],
        ),
    );
    write(
        &head_receipt,
        &proof_receipt(
            &[derivation("head-derivation", "formal_proof", "current")],
            &[evidence("head-claim", "head-derivation")],
        ),
    );
    let output = review_command(&base, &head, Some(&base_receipt), Some(&head_receipt));
    assert!(output.status.success(), "receipt binding failed");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("\"base_recorded\":true"));
    assert!(stdout.contains("\"head_recorded\":true"));
    assert!(stdout.contains("\"gained\":1"));
    assert!(stdout.contains("\"lost\":1"));

    let external_receipt = root.join("external.jetproof");
    write(
        &external_receipt,
        &proof_receipt(
            &[derivation("shared-derivation", "formal_proof", "external")],
            &[evidence("shared-claim", "shared-derivation")],
        ),
    );
    let output = review_command(
        &base,
        &head,
        Some(&external_receipt),
        Some(&external_receipt),
    );
    assert!(output.status.success(), "external receipt review failed");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("\"unproved\""));
    assert!(stdout.contains("not a checked universal contract"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn review_default_rejects_error_bearing_side() {
    let root = scratch("default-error-side");
    let _ = fs::remove_dir_all(&root);
    let base = project(&root, "base", "", REVIEW_SOURCE);
    let head = project(&root, "head", "", "fn run() { missing() }\n");

    let output = review_command(&base, &head, None, None);
    assert!(
        !output.status.success(),
        "default semantic review unexpectedly accepted an error-bearing side: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let diagnostic = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        diagnostic.contains("E"),
        "default semantic review omitted its checker diagnostic:\n{diagnostic}"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn review_diagnostics_mode_compares_error_occurrences_without_receipts() {
    let root = scratch("diagnostics-statuses");
    let _ = fs::remove_dir_all(&root);
    let clean = project(&root, "clean", "", "fn run() {}\n");
    let broken = project(&root, "broken", "", "fn run() { missing() }\n");

    let output = diagnostic_review_command(&clean, &broken);
    assert!(
        output.status.success(),
        "diagnostic review failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with("{\"schema\":\"jet.diagnostic-review/v1\""));
    assert!(stdout.contains("\"base\""));
    assert!(stdout.contains("\"head\""));
    assert!(stdout.contains("\"valid\":true"));
    assert!(stdout.contains("\"valid\":false"));
    assert!(stdout.contains("\"origin\""));
    assert!(stdout.contains("\"cause\""));
    assert!(!stdout.contains("jet.status/v1"));

    let output = diagnostic_review_command(&broken, &clean);
    assert!(output.status.success(), "reverse diagnostic review failed");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("\"status\":\"resolved\""));

    let output = diagnostic_review_command(&broken, &broken);
    assert!(output.status.success(), "existing diagnostic review failed");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("\"status\":\"existing\""));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn review_diagnostics_mode_keeps_moved_same_code_unknown() {
    let root = scratch("diagnostics-moved");
    let _ = fs::remove_dir_all(&root);
    let base = project(&root, "base", "", "fn run() { missing() }\n");
    let head = project(
        &root,
        "head",
        "",
        "fn run() {\n    print(\"x\")\n    missing()\n}\n",
    );

    let output = diagnostic_review_command(&base, &head);
    assert!(output.status.success(), "moved diagnostic review failed");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("\"status\":\"unknown\""));
    assert!(stdout.contains("different location"));
    assert!(!stdout.contains("\"status\":\"resolved\""));
    assert!(!stdout.contains("\"status\":\"new\""));

    let _ = fs::remove_dir_all(root);
}
#[test]
fn review_diagnostics_mode_keeps_repeated_same_code_unknown() {
    let root = scratch("diagnostics-repeated");
    let _ = fs::remove_dir_all(&root);
    let base = project(
        &root,
        "base",
        "",
        "fn run() {\n    missing()\n    missing()\n}\n",
    );
    let head = project(&root, "head", "", "fn run() {\n    missing()\n}\n");

    let output = diagnostic_review_command(&base, &head);
    assert!(
        output.status.success(),
        "repeated diagnostic review failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("\"status\":\"unknown\""));
    assert!(stdout.contains("same-code occurrence has a different location"));
    assert!(!stdout.contains("\"status\":\"resolved\""));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn review_diagnostics_mode_keeps_changed_dependency_unknown() {
    let root = scratch("diagnostics-dependency");
    let _ = fs::remove_dir_all(&root);
    let base = project(
        &root,
        "base",
        "",
        "use project.dep as dep\nfn run() {\n    dep.call()\n}\n",
    );
    let head = project(
        &root,
        "head",
        "",
        "use project.dep as dep\nfn run() {\n    dep.call()\n}\n",
    );
    write(
        &base.join("dep.jet"),
        "module dep {\n    pub fn call() {\n        missing()\n    }\n}\n",
    );
    write(
        &head.join("dep.jet"),
        "module dep {\n    pub fn call() {\n        missing()\n    }\n}\n// dependency changed\n",
    );

    let output = diagnostic_review_command(&base, &head);
    assert!(
        output.status.success(),
        "dependency diagnostic review failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("\"status\":\"unknown\""));
    assert!(stdout.contains("diagnostic source snapshot changed in a dependency scope"));
    assert!(stdout.contains("dep.jet"));

    let _ = fs::remove_dir_all(root);
}


#[test]
fn review_diagnostics_mode_exposes_incomplete_missing_import_coverage() {
    let root = scratch("diagnostics-incomplete");
    let _ = fs::remove_dir_all(&root);
    let base = project(&root, "base", "", "fn run() {}\n");
    let head = project(
        &root,
        "head",
        "",
        "use project.missing\nfn run() {}\n",
    );

    let output = diagnostic_review_command(&base, &head);
    assert!(
        output.status.success(),
        "incomplete diagnostic review failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("\"schema\":\"jet.diagnostic-review/v1\""));
    assert!(stdout.contains("\"complete\":false"));
    assert!(stdout.contains("\"completeness\":\"incomplete\""));
    assert!(stdout.contains("\"status\":\"unknown\""));
    assert!(!stdout.contains("\"status\":\"new\""));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn review_diagnostics_mode_setup_failure_is_nonzero() {
    let root = scratch("diagnostics-setup-failure");
    let _ = fs::remove_dir_all(&root);
    let missing = root.join("missing").join("run.jet");
    let head = project(&root, "head", "", "fn run() {}\n");
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args([
            "review",
            missing.to_str().unwrap(),
            head.join("run.jet").to_str().unwrap(),
            "--diagnostics",
            "--json",
        ])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let diagnostic = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(diagnostic.contains("E2105"));
    let _ = fs::remove_dir_all(root);
}
