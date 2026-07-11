//! D-CODEMOD1 integration tests for replayable semantic rename codemods.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn jet() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("jet_codemod_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
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
