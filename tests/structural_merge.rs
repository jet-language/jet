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
fn structural_merge_composes_disjoint_edits_and_rechecks_output() {
    let root = dir("disjoint");
    let base = write(&root, "base.jet", BASE);
    let ours = write(&root, "ours.jet", "fn left() -> Int { return 10 }\nfn right() -> Int { return 2 }\nfn run() { print(left() + right()) }\n");
    let theirs = write(&root, "theirs.jet", "fn left() -> Int { return 1 }\nfn right() -> Int { return 20 }\nfn run() { print(left() + right()) }\n");
    let merged = root.join("merged.jet");
    let output = run(&["merge", "--structural", base.to_str().unwrap(), ours.to_str().unwrap(), theirs.to_str().unwrap(), "--out", merged.to_str().unwrap()]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let source = fs::read_to_string(&merged).unwrap();
    assert!(source.contains("return 10"), "{source}");
    assert!(source.contains("return 20"), "{source}");
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
