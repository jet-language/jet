mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{build_and_run_full, have_rustc, interpreter_run, jit_run};

const PRINTING_FIXTURE: &str = r#"
fn run() {
    print("core-call-witness")
}
"#;

fn assert_not_silent(label: &str, code: i32, stdout: &str, stderr: &str) {
    assert!(
        code != 0 || !stdout.is_empty(),
        "{label} returned exit 0 with empty stdout; stderr: {stderr}"
    );
    assert_eq!(code, 0, "{label} failed: {stderr}");
    assert_eq!(stdout, "core-call-witness\n", "{label} output: {stderr}");
}

#[test]
fn non_corpus_printing_fixture_cannot_succeed_silently() {
    let (jit_code, jit_stdout, jit_stderr) = jit_run("core_call_witness_jit", PRINTING_FIXTURE);
    assert_not_silent("default JIT", jit_code, &jit_stdout, &jit_stderr);

    let (interpreter_code, interpreter_stdout, interpreter_stderr) =
        interpreter_run("core_call_witness_interpreter", PRINTING_FIXTURE);
    assert_not_silent(
        "forced interpreter",
        interpreter_code,
        &interpreter_stdout,
        &interpreter_stderr,
    );

    if have_rustc() {
        let (aot_code, aot_stdout, aot_stderr) = build_and_run_full(
            "jet_core_call_witness",
            "core_call_witness_aot",
            PRINTING_FIXTURE,
        );
        assert_not_silent("AOT", aot_code, &aot_stdout, &aot_stderr);
    }
}
