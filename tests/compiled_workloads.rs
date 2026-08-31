//! Card #1414: the compiled-workload release gate must fail closed.

use std::process::Command;

#[path = "common/mod.rs"]
mod common;

#[test]
fn compiled_workload_gate_self_check() {
    let root = env!("CARGO_MANIFEST_DIR");
    let scratch = std::env::var("TMPDIR")
        .unwrap_or_else(|_| format!("{}/.cache/jet-test-scratch", std::env::var("HOME").unwrap()));
    let output = Command::new("bash")
        .arg("tools/ci/test-compiled-workload-gate.sh")
        .current_dir(root)
        .env("TMPDIR", scratch)
        .output()
        .expect("compiled workload gate self-check must start");
    assert!(
        output.status.success(),
        "compiled workload gate self-check failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn semantic_corpus_policy_runs_with_compiled_workloads() {
    common::corpus_policy::CorpusPolicy::load()
        .expect("corpus manifest")
        .check_gate("workload")
        .expect("compiled workload corpus semantic policy");
}
