mod tir_support;

use jet::Interpreter::{dev_iteration, RunOutcome};
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use tir_support::{build_and_run_full, have_rustc, run_default_multi};

static INTERPRETER_SEQ: AtomicU64 = AtomicU64::new(0);

fn compile_ok(source: &str) -> jet::CompileOutput {
    jet::compile(source).unwrap_or_else(|diagnostics| {
        panic!(
            "numeric widening fixture was rejected:\n{}",
            jet::render_diagnostics("numeric_widening.jet", source, &diagnostics)
        )
    })
}

fn compile_error(source: &str) -> String {
    let diagnostics = jet::compile(source).expect_err("numeric fixture unexpectedly compiled");
    jet::render_diagnostics("numeric_widening.jet", source, &diagnostics)
}

fn run_interpreter(name: &str, source: &str) -> (i32, String, String) {
    let sequence = INTERPRETER_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "jet_numeric_widen_interpreter_{}_{}_{}",
        name,
        std::process::id(),
        sequence
    ));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.jet");
    fs::write(&path, source).unwrap();
    match dev_iteration(path.to_str().unwrap(), false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => (exit_code, stdout, stderr),
        RunOutcome::Problems(diagnostics) => {
            panic!("forced interpreter rejected numeric widening: {diagnostics:#?}")
        }
    }
}

fn assert_all_tiers(name: &str, source: &str, expected_code: i32, expected_stdout: &str) {
    if have_rustc() {
        let (code, stdout, stderr) =
            build_and_run_full("jet_numeric_widen_aot", name, source);
        assert_eq!(code, expected_code, "AOT stderr:\n{stderr}");
        assert_eq!(stdout, expected_stdout, "AOT stderr:\n{stderr}");
    }

    let (code, stdout, stderr) =
        run_default_multi(name, "main.jet", &[("main.jet", source)]);
    assert_eq!(code, expected_code, "default-tier stderr:\n{stderr}");
    assert_eq!(stdout, expected_stdout, "default-tier stderr:\n{stderr}");
    assert!(
        !stderr.contains("tier0 interp"),
        "default tier silently deoptimized:\n{stderr}"
    );

    let (code, stdout, stderr) = run_interpreter(name, source);
    assert_eq!(code, expected_code, "interpreter stderr:\n{stderr}");
    assert_eq!(stdout, expected_stdout, "interpreter stderr:\n{stderr}");
}

fn assert_trap_all_tiers(name: &str, source: &str) {
    const MESSAGE: &str = "whole number cannot cross into the decimal without losing precision";

    if have_rustc() {
        let (code, stdout, stderr) =
            build_and_run_full("jet_numeric_widen_aot", name, source);
        assert_eq!(code, 70, "AOT stderr:\n{stderr}");
        assert!(stdout.is_empty(), "AOT stdout:\n{stdout}");
        assert!(stderr.contains(MESSAGE), "AOT stderr:\n{stderr}");
    }

    let (code, stdout, stderr) =
        run_default_multi(name, "main.jet", &[("main.jet", source)]);
    assert_eq!(code, 1, "default-tier stderr:\n{stderr}");
    assert!(stdout.is_empty(), "default-tier stdout:\n{stdout}");
    assert!(
        stderr.contains("[E0953]") && stderr.contains(MESSAGE),
        "default-tier stderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("tier0 interp"),
        "default tier silently deoptimized:\n{stderr}"
    );

    let sequence = INTERPRETER_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "jet_numeric_widen_interpreter_trap_{}_{}_{}",
        name,
        std::process::id(),
        sequence
    ));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.jet");
    fs::write(&path, source).unwrap();
    match dev_iteration(path.to_str().unwrap(), false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            assert_eq!(exit_code, 70, "interpreter stderr:\n{stderr}");
            assert!(stdout.is_empty(), "interpreter stdout:\n{stdout}");
            assert!(stderr.contains(MESSAGE), "interpreter stderr:\n{stderr}");
        }
        RunOutcome::Problems(diagnostics) => assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "E0953"
                    && diagnostic.why.contains(MESSAGE)),
            "interpreter diagnostics: {diagnostics:#?}"
        ),
    }
}

#[test]
fn exact_widening_is_symmetric_at_operators_and_works_for_arguments() {
    let output = compile_ok(
        r#"
fn take_i16(value: I16) => I16 {
    return value
}

fn return_i16(value: I8) => I16 {
    return value
}

fn increment(value: I32) => I32 {
    return value + 1
}

fn run() {
    small :: I8.{1}
    wide :: I16.{2}
    _ :: small + wide
    _ :: 1 + wide
    _ :: wide + 1
    _ :: take_i16(small)
    _ :: return_i16(small)
    assigned := I16.{0}
    assigned = small
    narrow_decimal :: F32.{1.5}
    wide_decimal :: 2.5
    _ :: narrow_decimal + wide_decimal
}
"#,
    );

    assert!(output.rust.contains("fn user_increment"));
}

#[test]
fn checked_and_approximate_crossings_match_aot_jit_and_interpreter() {
    let success = r#"
fn take_float(value: Float) => Float {
    return value
}

fn return_float(value: Int) => Float {
    return value
}

fn run() {
    exact :: Int.{9007199254740992}
    assigned := Float.{0.0}
    assigned = exact
    print(take_float(exact) == 9007199254740992.0)
    print(return_float(exact) == 9007199254740992.0)
    print(assigned == 9007199254740992.0)
    print((exact + 0.0) == 9007199254740992.0)

    lossy :: Int.{9007199254740993}
    print((approx(lossy) + 0.0) == 9007199254740992.0)
}
"#;
    assert_all_tiers("numeric_widen_success", success, 0, "true\ntrue\ntrue\ntrue\ntrue\n");

    for (name, target, value) in [
        ("numeric_widen_float_trap", "Float", "9007199254740993"),
        ("numeric_widen_f32_trap", "F32", "16777217"),
    ] {
        let source = format!(
            r#"
fn accept(value: {target}) {{}}

fn run() {{
    lossy :: Int.{{{value}}}
    accept(lossy)
}}
"#
        );
        assert_trap_all_tiers(name, &source);
    }
}

#[test]
fn widening_does_not_search_for_a_third_type_or_narrow() {
    let no_join = compile_error(
        r#"
fn run() {
    unsigned :: U8.{1}
    signed :: I8.{1}
    _ :: unsigned + signed
}
"#,
    );
    assert!(
        no_join.contains("[E0109]")
            && no_join.contains("neither U8 contains every value of I8"),
        "{no_join}"
    );

    let narrowing = compile_error(
        r#"
fn take_i8(value: I8) {}
fn run() {
    wide :: I16.{1}
    take_i8(wide)
}
"#,
    );
    assert!(narrowing.contains("[E0112]"), "{narrowing}");
}

#[test]
fn contextual_numerals_still_fail_at_the_destination_limit() {
    for expression in ["byte + 256", "256 + byte"] {
        let source = format!(
            r#"
fn run() {{
    byte :: U8.{{1}}
    _ :: {expression}
}}
"#
        );
        let rendered = compile_error(&source);
        assert!(
            rendered.contains("[E1003]")
                && rendered.contains("U8")
                && rendered.contains("255"),
            "{rendered}"
        );
    }
}
