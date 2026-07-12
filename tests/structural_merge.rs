//! D-MERGE-* (Tower #143): hostile structural diff/merge product proof.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn jet() -> PathBuf { PathBuf::from(env!("CARGO_BIN_EXE_jet")) }
fn dir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("jet_structural_merge_{}_{}", std::process::id(), name));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}
fn write(dir: &Path, name: &str, source: &str) -> PathBuf { let path = dir.join(name); fs::write(&path, source).unwrap(); path }
fn run(args: &[&str]) -> Output { Command::new(jet()).args(args).output().unwrap() }

const BASE: &str = "fn left() -> Int {\n    return 1\n}\n\nfn right() -> Int {\n    return 2\n}\n\nfn run() {\n    print(left() + right())\n}\n";

#[test]
fn structural_diff_classifies_body_and_rename_with_stable_ids() {
    let root = dir("diff");
    let before = write(&root, "before.jet", BASE);
    let after = write(&root, "after.jet", "fn first() -> Int { return 1 }\nfn right() -> Int { return 3 }\nfn run() { print(first() + right()) }\n");
    let output = run(&["diff", "--structural", before.to_str().unwrap(), after.to_str().unwrap(), "--report", "json"]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let report = String::from_utf8_lossy(&output.stdout);
    assert!(report.contains("\"kind\":\"renamed\""), "{report}");
    assert!(report.contains("\"kind\":\"body_changed\""), "{report}");
    assert!(report.contains("\"stable_id\":\"def:"), "{report}");
}

#[test]
fn structural_diff_ignores_format_comments_and_reports_signature_and_move() {
    let root = dir("diff_hostile");
    let old_dir = root.join("old");
    let new_dir = root.join("new");
    fs::create_dir_all(&old_dir).unwrap();
    fs::create_dir_all(&new_dir).unwrap();
    let before = write(&old_dir, "same.jet", "// old comment\nfn score(n: Int) -> Int { return n }\nfn run() { print(score(1)) }\n");
    let after = write(&new_dir, "same.jet", "// new comment\nfn score(n: Int, bonus: Int) -> Int {\n    return n + bonus\n}\nfn run() { print(score(1, 2)) }\n");
    let output = run(&["diff", "--structural", before.to_str().unwrap(), after.to_str().unwrap(), "--report", "json"]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let report = String::from_utf8_lossy(&output.stdout);
    assert!(report.contains("\"kind\":\"signature_changed\""), "{report}");
    assert!(report.contains("\"kind\":\"moved\""), "{report}");

    let same = write(&root, "format.jet", "fn score(n: Int) -> Int {\n    // only comment\n    return n\n}\nfn run() { print(score(1)) }\n");
    let no_churn = run(&["diff", "--structural", before.to_str().unwrap(), same.to_str().unwrap()]);
    assert!(no_churn.status.success());
    assert_eq!(String::from_utf8_lossy(&no_churn.stdout), "no structural changes\n");
}

#[test]
fn structural_diff_add_remove_reorder_rename_edit_is_deterministic() {
    let root = dir("diff_matrix");
    let before = write(&root, "before.jet", "fn only(value: Int) -> Int { return value }\nfn gone() -> Int { return 1 }\nfn run() { print(only(7)) }\n");
    let after = write(&root, "after.jet", "fn run() { print(renamed(7)) }\nfn added() -> Bool { return true }\nfn renamed(value: Int) -> Int { return 9 }\n");
    let args = ["diff", "--structural", before.to_str().unwrap(), after.to_str().unwrap(), "--report", "json"];
    let first = run(&args);
    let second = run(&args);
    assert!(first.status.success(), "{}", String::from_utf8_lossy(&first.stderr));
    assert_eq!(first.stdout, second.stdout);
    let report = String::from_utf8_lossy(&first.stdout);
    for kind in ["added", "removed", "renamed", "body_changed"] {
        assert!(report.contains(&format!("\"kind\":\"{kind}\"")), "{report}");
    }
}

#[test]
fn structural_merge_composes_disjoint_edits_and_rechecks_output() {
    let root = dir("disjoint");
    let base = write(&root, "base.jet", BASE);
    let ours = write(&root, "ours.jet", "// retained file header\nfn left() -> Int { return 10 }\nfn right() -> Int { return 2 }\nfn run() { print(left() + right()) }\n");
    let theirs = write(&root, "theirs.jet", "fn left() -> Int { return 1 }\nfn right() -> Int { return 20 }\nfn run() { print(left() + right()) }\n");
    let merged = root.join("merged.jet");
    let output = run(&["merge", "--structural", base.to_str().unwrap(), ours.to_str().unwrap(), theirs.to_str().unwrap(), "--out", merged.to_str().unwrap()]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let source = fs::read_to_string(&merged).unwrap();
    assert!(source.contains("return 10"), "{source}");
    assert!(source.contains("return 20"), "{source}");
    assert!(source.contains("retained file header"), "{source}");
    assert!(run(&["check", merged.to_str().unwrap()]).status.success());
}

#[test]
fn overlapping_edit_conflicts_without_writing_success_output() {
    let root = dir("conflict");
    let base = write(&root, "base.jet", BASE);
    let ours = write(&root, "ours.jet", "fn left() -> Int { return 10 }\nfn right() -> Int { return 2 }\nfn run() { print(left() + right()) }\n");
    let theirs = write(&root, "theirs.jet", "fn left() -> Int { return 11 }\nfn right() -> Int { return 2 }\nfn run() { print(left() + right()) }\n");
    let merged = root.join("must-not-exist.jet");
    let output = run(&["merge", "--structural", base.to_str().unwrap(), ours.to_str().unwrap(), theirs.to_str().unwrap(), "--out", merged.to_str().unwrap(), "--report", "editor"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(!merged.exists());
    let report = String::from_utf8_lossy(&output.stderr);
    assert!(report.contains("\"status\":\"conflict\""), "{report}");
    assert!(report.contains("\"kind\":\"overlapping_edit\""), "{report}");
}

#[test]
fn delete_edit_and_duplicate_stable_identity_never_auto_merge() {
    let root = dir("identity_collision");
    let base = write(&root, "base.jet", "fn a() -> Int { return 1 }\nfn b() -> Int { return 1 }\nfn run() { print(a() + b()) }\n");
    let ours = write(&root, "ours.jet", "fn c() -> Int { return 1 }\nfn b() -> Int { return 1 }\nfn run() { print(c() + b()) }\n");
    let theirs = write(&root, "theirs.jet", "fn a() -> Int { return 2 }\nfn b() -> Int { return 1 }\nfn run() { print(a() + b()) }\n");
    let merged = root.join("must-not-exist.jet");
    let output = run(&["merge", "--structural", base.to_str().unwrap(), ours.to_str().unwrap(), theirs.to_str().unwrap(), "--out", merged.to_str().unwrap(), "--report", "json"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(!merged.exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("delete_edit"));
}

#[test]
fn malformed_input_fails_before_output() {
    let root = dir("malformed");
    let base = write(&root, "base.jet", BASE);
    let bad = write(&root, "bad.jet", "fn run( {");
    let merged = root.join("must-not-exist.jet");
    let output = run(&["merge", "--structural", base.to_str().unwrap(), base.to_str().unwrap(), bad.to_str().unwrap(), "--out", merged.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(!merged.exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("did not pass parser and sema"));
}

#[test]
fn git_driver_install_is_idempotent_and_preserves_config() {
    let root = dir("driver");
    fs::create_dir(root.join(".git")).unwrap();
    fs::write(root.join(".git/config"), "[core]\n\tbare = false\n").unwrap();
    for _ in 0..2 {
        let output = run(&["merge", "install-driver", "--repo", root.to_str().unwrap()]);
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    }
    let config = fs::read_to_string(root.join(".git/config")).unwrap();
    assert!(config.contains("[core]"));
    assert_eq!(config.matches("[merge \"jetstruct\"]").count(), 1);
    let attrs = fs::read_to_string(root.join(".gitattributes")).unwrap();
    assert_eq!(attrs.matches("*.jet merge=jetstruct").count(), 1);
}
