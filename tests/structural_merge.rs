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
fn run_in(dir: &Path, args: &[&str]) -> Output { Command::new(jet()).current_dir(dir).args(args).output().unwrap() }
fn git(dir: &Path, args: &[&str]) -> Output { Command::new("git").current_dir(dir).args(args).output().unwrap() }

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
    assert!(String::from_utf8_lossy(&output.stderr).contains("overlapping_edit"));
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
    assert!(git(&root, &["init"]).status.success());
    assert!(git(&root, &["config", "merge.jetstruct.name", "stale"]).status.success());
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

#[test]
fn identical_bilateral_additions_merge_trivia_once() {
    let root = dir("bilateral_additions");
    let base = write(&root, "base.jet", "fn run() {}\n");
    let ours = write(&root, "ours.jet", "// ours note\nfn helper() -> Int { return 1 }\nfn run() {}\n");
    let theirs = write(&root, "theirs.jet", "fn helper() -> Int { return 1 }\nfn run() {}\n");
    let merged = root.join("merged.jet");
    let output = run(&[
        "merge", "--structural", base.to_str().unwrap(), ours.to_str().unwrap(),
        theirs.to_str().unwrap(), "--out", merged.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let source = fs::read_to_string(&merged).unwrap();
    assert_eq!(source.matches("fn helper").count(), 1, "{source}");
    assert_eq!(source.matches("ours note").count(), 1, "{source}");

    let ours = write(&root, "ours_distinct.jet", "// ours note\nfn helper() -> Int { return 1 }\nfn run() {}\n");
    let theirs = write(&root, "theirs_distinct.jet", "// theirs note\nfn helper() -> Int { return 1 }\nfn run() {}\n");
    let conflict_out = root.join("must-not-exist.jet");
    let conflict = run(&[
        "merge", "--structural", base.to_str().unwrap(), ours.to_str().unwrap(),
        theirs.to_str().unwrap(), "--out", conflict_out.to_str().unwrap(), "--report", "json",
    ]);
    assert_eq!(conflict.status.code(), Some(1));
    assert!(!conflict_out.exists());
    assert!(String::from_utf8_lossy(&conflict.stderr).contains("inter_item_trivia"));
}

#[test]
fn inter_item_trivia_three_way_merges_and_conflicts_honestly() {
    let root = dir("inter_item_trivia");
    let base = write(&root, "base.jet", BASE);
    let ours = write(
        &root,
        "ours.jet",
        "fn left() -> Int { return 10 }\n\n// ownership note\nfn right() -> Int { return 2 }\n\nfn run() { print(left() + right()) }\n",
    );
    let theirs = write(
        &root,
        "theirs.jet",
        "fn left() -> Int { return 1 }\n\nfn right() -> Int { return 20 }\n\nfn run() { print(left() + right()) }\n",
    );
    let merged = root.join("merged.jet");
    let output = run(&[
        "merge", "--structural", base.to_str().unwrap(), ours.to_str().unwrap(),
        theirs.to_str().unwrap(), "--out", merged.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let source = fs::read_to_string(&merged).unwrap();
    assert!(source.contains("ownership note"), "{source}");
    assert!(source.contains("return 10"), "{source}");
    assert!(source.contains("return 20"), "{source}");

    let theirs_trivia = write(
        &root,
        "theirs_trivia.jet",
        "fn left() -> Int { return 1 }\n\n// conflicting note\nfn right() -> Int { return 2 }\n\nfn run() { print(left() + right()) }\n",
    );
    let conflict_out = root.join("trivia-conflict.jet");
    let conflict = run(&[
        "merge", "--structural", base.to_str().unwrap(), ours.to_str().unwrap(),
        theirs_trivia.to_str().unwrap(), "--out", conflict_out.to_str().unwrap(), "--report", "json",
    ]);
    assert_eq!(conflict.status.code(), Some(1));
    assert!(!conflict_out.exists());
    assert!(String::from_utf8_lossy(&conflict.stderr).contains("inter_item_trivia"));
}

#[test]
fn nonexistent_relative_out_uses_its_real_import_root() {
    let root = dir("relative_out_imports");
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    write(&project, "support.jet", "pub fn value() -> Int { return 7 }\n");
    let source = "use support.value\nfn run() { print(value()) }\n";
    let base = write(&project, "base.jet", source);
    let ours = write(&project, "ours.jet", source);
    let theirs = write(&project, "theirs.jet", source);
    let output = run_in(&project, &[
        "merge", "--structural", base.to_str().unwrap(), ours.to_str().unwrap(),
        theirs.to_str().unwrap(), "--out", "merged.jet",
    ]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(project.join("merged.jet").exists());
}

#[test]
fn signature_classification_and_move_do_not_depend_on_body_or_basename() {
    let root = dir("signature_and_move_identity");
    let old = root.join("old");
    let new = root.join("new");
    fs::create_dir_all(&old).unwrap();
    fs::create_dir_all(&new).unwrap();
    let before = write(&old, "alpha.jet", "fn score(n: Int) -> Int { return n + 1 }\nfn run() { print(score(1)) }\n");
    let after = write(&new, "omega.jet", "fn score(n: Float) -> Float { return n + 2.0 }\nfn run() { print(score(1.0)) }\n");
    let output = run(&["diff", "--structural", before.to_str().unwrap(), after.to_str().unwrap(), "--report", "json"]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let report = String::from_utf8_lossy(&output.stdout);
    assert!(report.contains("\"kind\":\"signature_changed\""), "{report}");
    assert!(report.contains("\"kind\":\"moved\""), "{report}");
}

#[test]
fn ambiguous_same_shape_and_cross_delete_rename_edit_never_guess() {
    let root = dir("ambiguous_cross_edits");
    let base = write(&root, "base.jet", "fn a() -> Int { return 1 }\nfn b() -> Int { return 1 }\nfn run() { print(a() + b()) }\n");
    let ours = write(&root, "ours.jet", "fn c() -> Int { return 1 }\nfn d() -> Int { return 1 }\nfn run() { print(c() + d()) }\n");
    let theirs = write(&root, "theirs.jet", "fn a() -> Int { return 2 }\nfn run() { print(a()) }\n");
    let merged = root.join("must-not-exist.jet");
    let output = run(&[
        "merge", "--structural", base.to_str().unwrap(), ours.to_str().unwrap(),
        theirs.to_str().unwrap(), "--out", merged.to_str().unwrap(), "--report", "json",
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert!(!merged.exists());
    let report = String::from_utf8_lossy(&output.stderr);
    assert!(report.contains("ambiguous_identity"), "{report}");

    let delete_base = write(&root, "delete_base.jet", "fn a() -> Int { return 1 }\nfn b() -> Int { return 1 }\nfn run() { print(b()) }\n");
    let deleted = write(&root, "deleted.jet", "fn b() -> Int { return 1 }\nfn run() { print(b()) }\n");
    let edited = write(&root, "edited.jet", "fn a() -> Int { return 9 }\nfn b() -> Int { return 1 }\nfn run() { print(b()) }\n");
    let delete_out = root.join("delete-edit-must-not-exist.jet");
    let delete_edit = run(&[
        "merge", "--structural", delete_base.to_str().unwrap(), deleted.to_str().unwrap(),
        edited.to_str().unwrap(), "--out", delete_out.to_str().unwrap(), "--report", "json",
    ]);
    assert_eq!(delete_edit.status.code(), Some(1));
    assert!(!delete_out.exists());
    assert!(String::from_utf8_lossy(&delete_edit.stderr).contains("delete_edit"));
}

#[test]
fn git_driver_repairs_exact_keys_and_supports_gitdir_indirection() {
    let root = dir("driver_gitdir");
    let repo = root.join("repo");
    let worktree = root.join("linked");
    fs::create_dir_all(&repo).unwrap();
    assert!(git(&repo, &["init"]).status.success());
    assert!(git(&repo, &["config", "user.email", "jet-test@example.invalid"]).status.success());
    assert!(git(&repo, &["config", "user.name", "Jet Test"]).status.success());
    assert!(git(&repo, &["config", "merge.jetstruct.name", "stale"]).status.success());
    assert!(git(&repo, &["config", "merge.other.driver", "keep-me"]).status.success());
    fs::write(repo.join("seed"), "seed\n").unwrap();
    assert!(git(&repo, &["add", "seed"]).status.success());
    assert!(git(&repo, &["commit", "-m", "seed"]).status.success());
    let added = git(&repo, &["worktree", "add", "-b", "linked-test", worktree.to_str().unwrap()]);
    assert!(added.status.success(), "{}", String::from_utf8_lossy(&added.stderr));

    let output = run(&["merge", "install-driver", "--repo", worktree.to_str().unwrap()]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    for (key, expected) in [
        ("merge.jetstruct.name", "Jet structural merge"),
        ("merge.jetstruct.driver", "jet merge --structural %O %A %B --out %A"),
        ("merge.other.driver", "keep-me"),
    ] {
        let readback = git(&worktree, &["config", "--local", "--get", key]);
        assert!(readback.status.success(), "{}", String::from_utf8_lossy(&readback.stderr));
        assert_eq!(String::from_utf8_lossy(&readback.stdout).trim(), expected);
    }
    assert!(worktree.join(".git").is_file(), "test must exercise real linked-worktree indirection");
}

#[test]
fn structural_commands_have_specific_help() {
    for (command, needles) in [
        ("diff", &["--structural", "--report"] as &[&str]),
        ("merge", &["--structural", "--out", "install-driver", "--repo"] as &[&str]),
    ] {
        for args in [[command, "--help"], ["help", command]] {
            let output = run(&args);
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            let help = String::from_utf8_lossy(&output.stdout);
            for needle in needles { assert!(help.contains(needle), "{command} help missing {needle}: {help}"); }
        }
    }
}
