//! D-DEVR-TRY1=A integration tests for overlay-first speculative edits.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

fn jet() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jet-try-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("examples")).unwrap();
    dir
}

fn fixture(label: &str, replacement: &str) -> (PathBuf, PathBuf, Vec<u8>, Vec<u8>) {
    let dir = temp_dir(label);
    let source_path = dir.join("examples/main.jet");
    let before = b"fn target() {\n    print(\"one\")\n}\n\nfn untouched() {\n    print(\"stable\")\n}\n\nfn run() {\n    target()\n}\n".to_vec();
    let start = before
        .windows(3)
        .position(|window| window == b"one")
        .unwrap();
    let mut after = before.clone();
    after.splice(start..start + 3, replacement.as_bytes().iter().copied());
    fs::write(&source_path, &before).unwrap();
    let plan_path = dir.join("try.json");
    fs::write(
        &plan_path,
        format!(
            "{{\"name\":\"{label}\",\"entry\":\"examples/main.jet\",\"edits\":[{{\"path\":\"examples/main.jet\",\"start\":{start},\"end\":{},\"new_text\":\"{replacement}\"}}]}}\n",
            start + 3
        ),
    )
    .unwrap();
    (dir, plan_path, before, after)
}

#[test]
fn default_try_rechecks_overlay_then_rolls_back_byte_for_byte() {
    let (dir, plan, before, _) = fixture("rollback", "two");
    let output = Command::new(jet()).arg("try").arg(&plan).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("claims re-checked"), "{stdout}");
    assert!(stdout.contains("claims reused"), "{stdout}");
    assert!(stdout.contains("rolled back (default)"), "{stdout}");
    assert_eq!(fs::read(dir.join("examples/main.jet")).unwrap(), before);
    assert!(
        !dir.join(".jet").exists(),
        "default try must not create a transaction directory"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn keep_lands_the_same_bytes_as_the_staged_edit() {
    let (dir, plan, _, after) = fixture("keep", "two");
    let output = Command::new(jet())
        .args(["try", plan.to_str().unwrap(), "--keep"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("kept"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(fs::read(dir.join("examples/main.jet")).unwrap(), after);
    assert!(dir.join(".jet/codemods").is_dir());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn failed_verdict_rolls_back_without_keep() {
    let (dir, plan, before, _) = fixture("failed", "one");
    // Add a second closing quote to the string while preserving valid plan JSON.
    fs::write(
        &plan,
        format!(
            "{{\"name\":\"failed\",\"entry\":\"examples/main.jet\",\"edits\":[{{\"path\":\"examples/main.jet\",\"start\":{},\"end\":{},\"new_text\":\"one\\\"\"}}]}}\n",
            before.windows(3).position(|window| window == b"one").unwrap(),
            before.windows(3).position(|window| window == b"one").unwrap() + 3
        ),
    )
    .unwrap();
    let output = Command::new(jet()).arg("try").arg(&plan).output().unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("verdict failed"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read(dir.join("examples/main.jet")).unwrap(), before);
    let _ = fs::remove_dir_all(dir);
}
