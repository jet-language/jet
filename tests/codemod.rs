//! D-CODEMOD1 integration tests for replayable semantic rename codemods.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn jet() -> PathBuf {
    std::env::var_os("JET_CODEMOD_TEST_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_jet")))
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("jet_codemod_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn check_stderr(path: &std::path::Path) -> Vec<u8> {
    let output = Command::new(jet())
        .args(["check", path.to_str().unwrap()])
        .output()
        .expect("jet check fixture");
    assert!(!output.status.success(), "fixture must fail");
    let mut rendered = String::from_utf8(output.stderr).unwrap();
    if let Some(at) = rendered.find("\n1 problem found\n") {
        rendered.truncate(at);
        while rendered.ends_with("\n\n") {
            rendered.pop();
        }
        if !rendered.ends_with('\n') {
            rendered.push('\n');
        }
    }
    rendered.into_bytes()
}

#[test]
fn codemod_rename_dry_run_apply_and_undo() {
    let dir = temp_dir("rename");
    let source = dir.join("main.jet");
    fs::write(
        &source,
        "fn report() {\n    print(\"ok\")\n}\n\nfn run() {\n    report()\n}\n",
    )
    .unwrap();
    let object = dir.join("rename.codemod.json");
    fs::write(
        &object,
        format!(
            "{{\"name\":\"RenameReport\",\"entry\":\"{}\",\"operation\":\"rename\",\"from\":\"report\",\"to\":\"summarize\"}}\n",
            source.display()
        ),
    )
    .unwrap();

    let dry = Command::new(jet())
        .args(["inspect", "codemod", "dry-run", object.to_str().unwrap()])
        .output()
        .expect("jet inspect codemod dry-run");
    assert!(
        dry.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&dry.stderr)
    );
    let dry_text = String::from_utf8_lossy(&dry.stdout);
    assert!(dry_text.contains("RenameReport"));
    assert!(dry_text.contains("report -> summarize"));
    assert!(
        fs::read_to_string(&source).unwrap().contains("fn report"),
        "dry run must not write"
    );

    let apply = Command::new(jet())
        .args(["inspect", "codemod", "apply", object.to_str().unwrap()])
        .output()
        .expect("jet inspect codemod apply");
    assert!(
        apply.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&apply.stderr)
    );
    let changed = fs::read_to_string(&source).unwrap();
    assert!(changed.contains("fn summarize"));
    assert!(changed.contains("summarize()"));

    let log = dir.join(".jet/codemods/RenameReport.log.json");
    assert!(log.exists(), "apply should write replay log");
    let log_text = fs::read_to_string(&log).unwrap();
    assert!(log_text.contains("inverse_from"));
    assert!(log_text.contains("after_hash"));
    assert!(log_text.contains("inverse_edits"));

    let undo = Command::new(jet())
        .args(["inspect", "codemod", "undo", log.to_str().unwrap()])
        .output()
        .expect("jet inspect codemod undo");
    assert!(
        undo.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&undo.stderr)
    );
    let restored = fs::read_to_string(&source).unwrap();
    assert!(restored.contains("fn report"));
    assert!(restored.contains("report()"));
}

#[test]
fn codemod_undo_refuses_changed_file() {
    let dir = temp_dir("stale");
    let source = dir.join("main.jet");
    fs::write(
        &source,
        "fn report() {\n    print(\"ok\")\n}\n\nfn run() {\n    report()\n}\n",
    )
    .unwrap();
    let object = dir.join("rename.codemod.json");
    fs::write(
        &object,
        format!(
            "{{\"name\":\"StaleRename\",\"entry\":\"{}\",\"operation\":\"rename\",\"from\":\"report\",\"to\":\"summarize\"}}\n",
            source.display()
        ),
    )
    .unwrap();

    let apply = Command::new(jet())
        .args(["inspect", "codemod", "apply", object.to_str().unwrap()])
        .output()
        .expect("jet inspect codemod apply");
    assert!(
        apply.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&apply.stderr)
    );
    fs::write(
        &source,
        "fn summarize() {\n    print(\"changed\")\n}\n\nfn run() {\n    summarize()\n}\n",
    )
    .unwrap();

    let log = dir.join(".jet/codemods/StaleRename.log.json");
    let undo = Command::new(jet())
        .args(["inspect", "codemod", "undo", log.to_str().unwrap()])
        .output()
        .expect("jet inspect codemod undo");
    assert!(!undo.status.success());
    let stderr = String::from_utf8_lossy(&undo.stderr);
    assert!(stderr.contains("checkpoint mismatch"), "stderr: {stderr}");
    assert!(fs::read_to_string(&source).unwrap().contains("changed"));
}

#[test]
fn batch_rules_reindex_across_clean_and_fixture_roots_then_undo_exactly() {
    let project = temp_dir("batch_chain");
    let example = project.join("examples/report.jet");
    let fixture = project.join("tests/ui/report_type.jet");
    let fixture_stderr = fixture.with_extension("stderr");
    let migrations = project.join("migrations");
    fs::create_dir_all(example.parent().unwrap()).unwrap();
    fs::create_dir_all(fixture.parent().unwrap()).unwrap();
    fs::create_dir_all(&migrations).unwrap();

    let clean_before = "fn legacy_parse(text: String) -> Int { return 42 }\nfn parse_int(text: String, base: Int) -> Int { return 42 }\nfn report(value: Int) { print(value) }\nfn run() { report(legacy_parse(\"42\")) }\n";
    let clean_after = "fn legacy_parse(text: String) -> Int { return 42 }\nfn parse_int(text: String, base: Int) -> Int { return 42 }\nfn summarize(value: Int) { print(value) }\nfn run() { summarize(parse_int(\"42\", base: 10)) }\n";
    let fixture_before = "fn legacy_parse(text: String) -> Int { return 42 }\nfn parse_int(text: String, base: Int) -> Int { return 42 }\nfn report(value: Int) {}\nfn run() { report(legacy_parse(true)) }\n";
    let fixture_after = "fn legacy_parse(text: String) -> Int { return 42 }\nfn parse_int(text: String, base: Int) -> Int { return 42 }\nfn summarize(value: Int) {}\nfn run() { summarize(parse_int(true, base: 10)) }\n";
    fs::write(&example, clean_before).unwrap();
    fs::write(&fixture, fixture_before).unwrap();
    let stderr_before = check_stderr(&fixture);
    fs::write(&fixture_stderr, &stderr_before).unwrap();
    fs::write(&fixture, fixture_after).unwrap();
    let stderr_after = check_stderr(&fixture);
    fs::write(migrations.join("report_type.after.stderr"), &stderr_after).unwrap();
    fs::write(&fixture, fixture_before).unwrap();

    let object = migrations.join("batch.codemod.json");
    fs::write(
        &object,
        r#"{
  "version": 2,
  "name": "ReportV2",
  "project": "..",
  "roots": [
    {"path": "examples/report.jet", "validate": "clean"},
    {"path": "tests/ui/report_type.jet", "validate": "fixture"}
  ],
  "rules": [
    {"id": "rename-report", "kind": "symbol_rename", "from": {"name": "report", "symbol_kind": "function"}, "to": "summarize", "matches": 4},
    {"id": "parse-needs-base", "kind": "ast_rewrite", "node": "expr", "match": "legacy_parse($input)", "replace": "parse_int($input, base: 10)", "matches": 2}
  ],
  "snapshot_after": {"tests/ui/report_type.jet": "migrations/report_type.after.stderr"}
}
"#,
    )
    .unwrap();

    let dry = Command::new(jet())
        .args(["inspect", "codemod", "dry-run", object.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        dry.status.success(),
        "{}",
        String::from_utf8_lossy(&dry.stderr)
    );
    let dry_out = String::from_utf8_lossy(&dry.stdout);
    assert!(dry_out.contains("parse-needs-base: 2 matches"), "{dry_out}");
    assert_eq!(fs::read_to_string(&example).unwrap(), clean_before);
    assert_eq!(fs::read(&fixture_stderr).unwrap(), stderr_before);
    assert!(!project.join(".jet/codemods/ReportV2.log.json").exists());

    let apply = Command::new(jet())
        .args([
            "inspect",
            "codemod",
            "apply",
            object.to_str().unwrap(),
            "--yes",
        ])
        .output()
        .unwrap();
    assert!(
        apply.status.success(),
        "{}",
        String::from_utf8_lossy(&apply.stderr)
    );
    assert_eq!(fs::read_to_string(&example).unwrap(), clean_after);
    assert_eq!(fs::read_to_string(&fixture).unwrap(), fixture_after);
    assert_eq!(fs::read(&fixture_stderr).unwrap(), stderr_after);

    let log = project.join(".jet/codemods/ReportV2.log.json");
    let undo = Command::new(jet())
        .args(["inspect", "codemod", "undo", log.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        undo.status.success(),
        "{}",
        String::from_utf8_lossy(&undo.stderr)
    );
    assert_eq!(fs::read_to_string(&example).unwrap(), clean_before);
    assert_eq!(fs::read_to_string(&fixture).unwrap(), fixture_before);
    assert_eq!(fs::read(&fixture_stderr).unwrap(), stderr_before);
}

fn simple_batch(project: &std::path::Path, matches: usize) -> PathBuf {
    let example = project.join("examples/a.jet");
    fs::create_dir_all(example.parent().unwrap()).unwrap();
    fs::write(
        &example,
        "fn report() { print(\"ok\") }\nfn run() { report() }\n",
    )
    .unwrap();
    let object = project.join("rename.codemod.json");
    fs::write(
        &object,
        format!(
            "{{\"version\":2,\"name\":\"BatchRename\",\"project\":\".\",\"roots\":[{{\"path\":\"examples/a.jet\",\"validate\":\"clean\"}}],\"rules\":[{{\"id\":\"rename\",\"kind\":\"symbol_rename\",\"from\":{{\"name\":\"report\",\"symbol_kind\":\"function\"}},\"to\":\"summarize\",\"matches\":{matches}}}]}}\n"
        ),
    )
    .unwrap();
    object
}

#[test]
fn batch_refuses_declared_count_and_unknown_fields_without_writes() {
    let project = temp_dir("batch_refuse");
    let object = simple_batch(&project, 3);
    let source = project.join("examples/a.jet");
    let before = fs::read(&source).unwrap();
    let count = Command::new(jet())
        .args(["inspect", "codemod", "dry-run", object.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!count.status.success());
    assert!(String::from_utf8_lossy(&count.stderr).contains("matched 2, expected 3"));
    assert_eq!(fs::read(&source).unwrap(), before);

    let raw = fs::read_to_string(&object).unwrap();
    fs::write(
        &object,
        raw.replacen("\"name\":", "\"mystery\":1,\"name\":", 1),
    )
    .unwrap();
    let unknown = Command::new(jet())
        .args(["inspect", "codemod", "dry-run", object.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!unknown.status.success());
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("unknown field"));
    assert_eq!(fs::read(&source).unwrap(), before);
}

#[test]
fn interrupted_batch_recovers_before_the_next_plan() {
    let project = temp_dir("batch_recovery");
    let object = simple_batch(&project, 2);
    let source = project.join("examples/a.jet");
    let source_b = project.join("examples/b.jet");
    fs::write(
        &source_b,
        "fn report() { print(\"b\") }\nfn run() { report() }\n",
    )
    .unwrap();
    let object_text = fs::read_to_string(&object).unwrap();
    fs::write(
        &object,
        object_text
            .replace("examples/a.jet", "examples")
            .replace("\"matches\":2", "\"matches\":4"),
    )
    .unwrap();
    let before = fs::read(&source).unwrap();
    let before_b = fs::read(&source_b).unwrap();
    let crashed = Command::new(jet())
        .env("JET_CODEMOD_CRASH_AFTER_RENAME", "1")
        .args([
            "inspect",
            "codemod",
            "apply",
            object.to_str().unwrap(),
            "--yes",
        ])
        .output()
        .unwrap();
    assert_eq!(crashed.status.code(), Some(86));
    assert!(project.join(".jet/codemods/transaction.journal").exists());
    assert_ne!(fs::read(&source).unwrap(), before);
    assert_eq!(fs::read(&source_b).unwrap(), before_b);

    let recovered = Command::new(jet())
        .args(["inspect", "codemod", "dry-run", object.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        recovered.status.success(),
        "{}",
        String::from_utf8_lossy(&recovered.stderr)
    );
    assert_eq!(fs::read(&source).unwrap(), before);
    assert_eq!(fs::read(&source_b).unwrap(), before_b);
    assert!(!project.join(".jet/codemods/transaction.journal").exists());
}

#[test]
fn all_after_crash_completes_log_then_undo_restores_every_file() {
    let project = temp_dir("batch_complete_recovery");
    let object = simple_batch(&project, 2);
    let source = project.join("examples/a.jet");
    let before = fs::read(&source).unwrap();
    let crashed = Command::new(jet())
        .env("JET_CODEMOD_CRASH_AFTER_RENAME", "1")
        .args([
            "inspect",
            "codemod",
            "apply",
            object.to_str().unwrap(),
            "--yes",
        ])
        .output()
        .unwrap();
    assert_eq!(crashed.status.code(), Some(86));
    let log = project.join(".jet/codemods/BatchRename.log.json");
    assert!(!log.exists());
    let undo = Command::new(jet())
        .args(["inspect", "codemod", "undo", log.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        undo.status.success(),
        "{}",
        String::from_utf8_lossy(&undo.stderr)
    );
    assert_eq!(fs::read(&source).unwrap(), before);
    assert!(!project.join(".jet/codemods/transaction.journal").exists());
}

#[cfg(unix)]
#[test]
fn batch_rejects_symlink_and_parent_escape_roots() {
    use std::os::unix::fs::symlink;
    let project = temp_dir("batch_paths");
    fs::create_dir_all(project.join("examples")).unwrap();
    let outside = project.parent().unwrap().join("codemod-outside.jet");
    fs::write(&outside, "fn run() {}\n").unwrap();
    symlink(&outside, project.join("examples/link.jet")).unwrap();
    let object = project.join("bad.codemod.json");
    for root in ["examples/link.jet", "examples/../tests/ui/x.jet"] {
        fs::write(&object, format!("{{\"version\":2,\"name\":\"Bad\",\"project\":\".\",\"roots\":[{{\"path\":\"{root}\",\"validate\":\"clean\"}}],\"rules\":[{{\"id\":\"x\",\"kind\":\"symbol_rename\",\"from\":{{\"name\":\"run\",\"symbol_kind\":\"function\"}},\"to\":\"start\",\"matches\":1}}]}}\n")).unwrap();
        let output = Command::new(jet())
            .args(["inspect", "codemod", "dry-run", object.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(!output.status.success(), "root {root} must fail");
    }
}

#[test]
fn schema_one_inverse_edit_log_remains_readable() {
    let dir = temp_dir("schema_one_log");
    let source = dir.join("main.jet");
    let before = "fn report() {}\nfn run() { report() }\n";
    let after = "fn summarize() {}\nfn run() { summarize() }\n";
    fs::write(&source, after).unwrap();
    let hash = |s: &str| format!("sha256-{}", jet::SHA256::sha256_hex(s.as_bytes()));
    let log = dir.join("old.log.json");
    fs::write(&log, format!("{{\"name\":\"Old\",\"files\":[{{\"path\":\"{}\",\"before_hash\":\"{}\",\"after_hash\":\"{}\",\"inverse_edits\":[{{\"start\":3,\"end\":12,\"new_text\":\"report\"}},{{\"start\":29,\"end\":38,\"new_text\":\"report\"}}]}}]}}\n", source.display(), hash(before), hash(after))).unwrap();
    let undo = Command::new(jet())
        .args(["inspect", "codemod", "undo", log.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        undo.status.success(),
        "{}",
        String::from_utf8_lossy(&undo.stderr)
    );
    assert_eq!(fs::read_to_string(source).unwrap(), before);
}

#[test]
fn batch_refuses_collision_invalid_binding_and_overlapping_ast_nodes() {
    let project = temp_dir("batch_semantic_refusals");
    let source = project.join("examples/a.jet");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    let body = "fn report(value: Int) { print(value) }\nfn summarize(value: Int) { print(value) }\nfn run() { report(report(1)) }\n";
    fs::write(&source, body).unwrap();
    let object = project.join("bad.codemod.json");

    fs::write(&object, "{\"version\":2,\"name\":\"Collision\",\"project\":\".\",\"roots\":[{\"path\":\"examples/a.jet\",\"validate\":\"clean\"}],\"rules\":[{\"id\":\"rename\",\"kind\":\"symbol_rename\",\"from\":{\"name\":\"report\",\"symbol_kind\":\"function\"},\"to\":\"summarize\",\"matches\":3}]}\n").unwrap();
    let collision = Command::new(jet())
        .args(["inspect", "codemod", "dry-run", object.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!collision.status.success());
    assert!(String::from_utf8_lossy(&collision.stderr).contains("destination `summarize`"));

    fs::write(&object, "{\"version\":2,\"name\":\"Binding\",\"project\":\".\",\"roots\":[{\"path\":\"examples/a.jet\",\"validate\":\"clean\"}],\"rules\":[{\"id\":\"binding\",\"kind\":\"ast_rewrite\",\"node\":\"expr\",\"match\":\"report($value)\",\"replace\":\"missing($value)\",\"matches\":2}]}\n").unwrap();
    let binding = Command::new(jet())
        .args(["inspect", "codemod", "dry-run", object.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!binding.status.success());
    assert!(String::from_utf8_lossy(&binding.stderr).contains("does not resolve"));

    fs::write(&object, "{\"version\":2,\"name\":\"Overlap\",\"project\":\".\",\"roots\":[{\"path\":\"examples/a.jet\",\"validate\":\"clean\"}],\"rules\":[{\"id\":\"overlap\",\"kind\":\"ast_rewrite\",\"node\":\"expr\",\"match\":\"report($value)\",\"replace\":\"summarize($value)\",\"matches\":2}]}\n").unwrap();
    let overlap = Command::new(jet())
        .args(["inspect", "codemod", "dry-run", object.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!overlap.status.success());
    assert!(
        String::from_utf8_lossy(&overlap.stderr).contains("overlapping edits"),
        "{}",
        String::from_utf8_lossy(&overlap.stderr)
    );
    assert_eq!(fs::read_to_string(source).unwrap(), body);
}

#[test]
fn semantic_rename_uses_resolved_reference_identity_not_spelling() {
    let project = temp_dir("batch_identity");
    let source = project.join("examples/a.jet");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    let before = "fn report() { print(\"function\") }\nfn run() { report :: 7\nprint(report) }\n";
    fs::write(&source, before).unwrap();
    let object = project.join("rename.codemod.json");
    fs::write(&object, "{\"version\":2,\"name\":\"Identity\",\"project\":\".\",\"roots\":[{\"path\":\"examples/a.jet\",\"validate\":\"clean\"}],\"rules\":[{\"id\":\"rename\",\"kind\":\"symbol_rename\",\"from\":{\"name\":\"report\",\"symbol_kind\":\"function\"},\"to\":\"summarize\",\"matches\":1}]}\n").unwrap();

    let output = Command::new(jet())
        .args(["inspect", "codemod", "apply", object.to_str().unwrap(), "--yes"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        fs::read_to_string(source).unwrap(),
        "fn summarize() { print(\"function\") }\nfn run() { report :: 7\nprint(report) }\n"
    );
}

#[test]
fn typed_ast_rewrite_matches_only_compiler_owned_node_class() {
    let project = temp_dir("batch_node_class");
    let source = project.join("examples/a.jet");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(
        &source,
        "fn helper(value: Int) {}\nfn run() { print(\"Int stays text\") }\n",
    )
    .unwrap();
    let object = project.join("types.codemod.json");
    fs::write(&object, "{\"version\":2,\"name\":\"Types\",\"project\":\".\",\"roots\":[{\"path\":\"examples/a.jet\",\"validate\":\"clean\"}],\"rules\":[{\"id\":\"type\",\"kind\":\"ast_rewrite\",\"node\":\"type\",\"match\":\"Int\",\"replace\":\"Float\",\"matches\":1}]}\n").unwrap();

    let output = Command::new(jet())
        .args(["inspect", "codemod", "apply", object.to_str().unwrap(), "--yes"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        fs::read_to_string(source).unwrap(),
        "fn helper(value: Float) {}\nfn run() { print(\"Int stays text\") }\n"
    );
}

#[test]
fn recovery_rejects_hostile_journal_paths_without_touching_outside_file() {
    let project = temp_dir("batch_hostile_journal");
    let object = simple_batch(&project, 2);
    let outside = project.parent().unwrap().join(format!(
        "{}-outside.jet",
        project.file_name().unwrap().to_string_lossy()
    ));
    fs::write(&outside, b"outside bytes\n").unwrap();
    let dir = project.join(".jet/codemods");
    fs::create_dir_all(&dir).unwrap();
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let log = dir.join("Hostile.log.json");
    let journal = format!(
        "{{\"schema\":2,\"tx\":\"hostile\",\"completed\":0,\"log_path\":\"{}\",\"log\":\"\",\"files\":[{{\"path\":\"{}\",\"temp\":\"{}\",\"before\":\"{}\",\"after\":\"{}\"}}]}}\n",
        log.display(),
        outside.display(),
        outside.with_extension("tmp").display(),
        hex(b"outside bytes\n"),
        hex(b"owned\n")
    );
    let journal_path = dir.join("transaction.journal");
    fs::write(&journal_path, journal).unwrap();

    let output = Command::new(jet())
        .args(["inspect", "codemod", "dry-run", object.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("escapes project"));
    assert_eq!(fs::read(&outside).unwrap(), b"outside bytes\n");
    assert!(journal_path.exists(), "hostile journal must remain for inspection");
}

#[test]
fn dry_run_diff_preserves_eof_newline_truth() {
    let project = temp_dir("batch_eof_diff");
    let source = project.join("examples/a.jet");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, b"fn report() {}\nfn run() { report() }").unwrap();
    let object = project.join("rename.codemod.json");
    fs::write(&object, "{\"version\":2,\"name\":\"Eof\",\"project\":\".\",\"roots\":[{\"path\":\"examples/a.jet\",\"validate\":\"clean\"}],\"rules\":[{\"id\":\"rename\",\"kind\":\"symbol_rename\",\"from\":{\"name\":\"report\",\"symbol_kind\":\"function\"},\"to\":\"summarize\",\"matches\":2}]}\n").unwrap();

    let output = Command::new(jet())
        .args(["inspect", "codemod", "dry-run", object.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.matches("\\ No newline at end of file").count(), 2, "{stdout}");
    assert_eq!(fs::read(source).unwrap(), b"fn report() {}\nfn run() { report() }");
}
