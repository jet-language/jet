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

#[test]
fn review_joins_meaning_authority_and_receipt_changes() {
    let root = scratch("joins");
    let _ = fs::remove_dir_all(&root);
    let base = root.join("base");
    let head = root.join("head");
    let manifest = |allow: &str| {
        format!(
            "name: \"review_fixture\"\nversion: \"0.1.0\"\nedition: \"2026\"\nauthority: .{{ holds: {{ allow: [{allow}] }} }}\n"
        )
    };
    write(&base.join("package.jet"), &manifest("FS"));
    write(&head.join("package.jet"), &manifest("FS, Net"));
    write(&base.join("run.jet"), "fn run() { print(\"base\") }\n");
    write(&head.join("run.jet"), "fn run() { print(\"head\") }\n");

    let base_receipt = root.join("base.jetproof");
    let head_receipt = root.join("head.jetproof");
    write(
        &base_receipt,
        r#"{"proofReport":{"evidence":[
            {"id":"old-retained","kind":"front_end","facet":"syntax","producer":"test","property":"retained","outcome":"proved","state":"checked"},
            {"id":"old-lost","kind":"front_end","facet":"syntax","producer":"test","property":"lost","outcome":"proved","state":"checked"}
        ]}}"#,
    );
    write(
        &head_receipt,
        r#"{"proofReport":{"evidence":[
            {"id":"new-retained","kind":"front_end","facet":"syntax","producer":"test","property":"retained","outcome":"proved","state":"checked"},
            {"id":"new-gained","kind":"front_end","facet":"syntax","producer":"test","property":"gained","outcome":"passed","state":"checked"}
        ]}}"#,
    );

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
    assert!(stdout.starts_with("{\"schema\":\"jet.report/v1\""));
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
    let before = "fn report() Int -> {\n    return 1\n}\n";
    let after = "fn summarize() Int -> {\n    return 1\n}\n";
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
    assert!(stdout.contains("\"stable_id\":\"def:"), "{stdout}");

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
