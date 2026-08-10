mod common;

mod tir_support;

use jet::Interpreter::{dev_iteration, RunOutcome};
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use tir_support::{build_and_run_full, build_and_run_multi, have_rustc};

static INTERPRETER_SEQ: AtomicU64 = AtomicU64::new(0);
static JIT_TRACE_LOCK: Mutex<()> = Mutex::new(());

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

fn write_program(name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
    let sequence = INTERPRETER_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "jet_numeric_widen_{}_{}_{}",
        name,
        std::process::id(),
        sequence
    ));
    fs::create_dir_all(&dir).unwrap();
    for (relative, source) in files {
        let path = dir.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, source).unwrap();
    }
    dir.join("main.jet")
}

fn run_in_process(
    name: &str,
    files: &[(&str, &str)],
    force_interpreter: bool,
) -> RunOutcome {
    let path = write_program(name, files);
    dev_iteration(path.to_str().unwrap(), false, force_interpreter)
}

fn run_interpreter(name: &str, source: &str) -> (i32, String, String) {
    match run_in_process(name, &[("main.jet", source)], true) {
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

fn run_resident(name: &str, files: &[(&str, &str)]) -> RunOutcome {
    let _trace_guard = JIT_TRACE_LOCK.lock().unwrap();
    jet_jit::reset_jit_trace_for_test();
    let outcome = run_in_process(name, files, false);
    assert!(
        jet_jit::jit_executed_for_test(),
        "{name} did not execute resident JIT code"
    );
    assert!(
        !jet_jit::fallback_invoked_for_test(),
        "{name} invoked the fallback tier"
    );
    assert!(
        !jet_jit::deopt_invoked_for_test(),
        "{name} deoptimized into the interpreter"
    );
    outcome
}

fn assert_all_tiers(name: &str, source: &str, expected_code: i32, expected_stdout: &str) {
    if have_rustc() {
        let (code, stdout, stderr) =
            build_and_run_full("jet_numeric_widen_aot", name, source);
        assert_eq!(code, expected_code, "AOT stderr:\n{stderr}");
        assert_eq!(stdout, expected_stdout, "AOT stderr:\n{stderr}");
    }

    match run_resident(name, &[("main.jet", source)]) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            assert_eq!(exit_code, expected_code, "resident-JIT stderr:\n{stderr}");
            assert_eq!(stdout, expected_stdout, "resident-JIT stderr:\n{stderr}");
        }
        RunOutcome::Problems(diagnostics) => {
            panic!("resident JIT rejected numeric widening: {diagnostics:#?}")
        }
    }

    let (code, stdout, stderr) = run_interpreter(name, source);
    assert_eq!(code, expected_code, "interpreter stderr:\n{stderr}");
    assert_eq!(stdout, expected_stdout, "interpreter stderr:\n{stderr}");
}

fn assert_all_tiers_multi(
    name: &str,
    files: &[(&str, &str)],
    expected_code: i32,
    expected_stdout: &str,
) {
    if have_rustc() {
        let (code, stdout) = build_and_run_multi(name, "main.jet", files);
        assert_eq!(code, expected_code, "multi-file AOT exit drift");
        assert_eq!(stdout, expected_stdout, "multi-file AOT output drift");
    }

    match run_resident(name, files) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            assert_eq!(exit_code, expected_code, "resident-JIT stderr:\n{stderr}");
            assert_eq!(stdout, expected_stdout, "resident-JIT stderr:\n{stderr}");
        }
        RunOutcome::Problems(diagnostics) => {
            panic!("resident JIT rejected imported numeric widening: {diagnostics:#?}")
        }
    }

    match run_in_process(name, files, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            assert_eq!(exit_code, expected_code, "interpreter stderr:\n{stderr}");
            assert_eq!(stdout, expected_stdout, "interpreter stderr:\n{stderr}");
        }
        RunOutcome::Problems(diagnostics) => {
            panic!("interpreter rejected imported numeric widening: {diagnostics:#?}")
        }
    }
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

    match run_resident(name, &[("main.jet", source)]) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            assert_eq!(exit_code, 70, "resident-JIT stderr:\n{stderr}");
            assert!(stdout.is_empty(), "resident-JIT stdout:\n{stdout}");
            assert!(stderr.contains(MESSAGE), "resident-JIT stderr:\n{stderr}");
        }
        RunOutcome::Problems(diagnostics) => assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "E0953"
                    && diagnostic.why.contains(MESSAGE)),
            "resident-JIT diagnostics: {diagnostics:#?}"
        ),
    }

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

    assert!(output.rust.contains("fn __jet_increment"));
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
fn parenthesized_approximate_crossings_match_aot_jit_and_interpreter() {
    let source = r#"
fn take_float(value: Float) => Float {
    return value
}

fn return_float(value: Int) => Float {
    return ((approx(value)))
}

fn run() {
    lossy :: Int.{9007199254740993}
    expected :: 9007199254740992.0

    print((((approx(lossy))) + 0.0) == expected)
    print(take_float(((approx(lossy)))) == expected)

    assigned := Float.{0.0}
    assigned = ((approx(lossy)))
    print(assigned == expected)

    print(return_float(lossy) == expected)
}
"#;

    assert_all_tiers(
        "numeric_widen_parenthesized_approx",
        source,
        0,
        "true\ntrue\ntrue\ntrue\n",
    );
}

#[test]
fn numeric_arguments_widen_at_every_user_call_seam() {
    let source = r#"
trait NumericSink {
    fn accept(self, value: Float) => Float
}

struct Holder {
    seed: Int
}

impl Holder.NumericSink {
    fn accept(self, value: Float) => Float {
        return value
    }
}

impl Holder {
    fn instance(self, value: Float) => Float {
        return value
    }

    fn static(value: Float) => Float {
        return value
    }
}

module numeric_helpers {
    pub fn accept(value: Float) => Float {
        return value
    }
}

fn accept_float(value: Float) => Float {
    return value
}

fn run() {
    narrow :: I32.{7}
    holder :: Holder.{seed: 0}
    callback :: accept_float

    print(holder.instance(narrow) == 7.0)
    print(Holder.static(narrow) == 7.0)
    print(callback(narrow) == 7.0)
    print(numeric_helpers.accept(narrow) == 7.0)
    print(holder.accept(narrow) == 7.0)
    widened :: [accept_float(narrow)]
    print(widened[0] == 7.0)
}
"#;

    assert_all_tiers(
        "numeric_widen_user_call_seams",
        source,
        0,
        "true\ntrue\ntrue\ntrue\ntrue\ntrue\n",
    );
}

#[test]
fn numeric_arguments_widen_across_imported_call_seams() {
    let files = [
        (
            "main.jet",
            r#"
use "./helper" as helper

fn run() {
    narrow :: I32.{7}
    print(helper.accept(narrow) == 7.0)
}
"#,
        ),
        (
            "helper.jet",
            r#"
pub fn accept(value: Float) => Float {
    return value
}
"#,
        ),
    ];
    assert_all_tiers_multi("numeric_widen_imported_call", &files, 0, "true\n");
}

#[test]
fn numeric_multi_producer_joins_widen_every_producer_across_tiers() {
    let source = r#"
fn run() {
    small :: U8.{1}
    wide :: U16.{2}
    choose_first :: true

    small_first :: if choose_first -> small else -> wide
    wide_first :: if choose_first -> wide else -> small
    print(small_first == U16.{1})
    print(wide_first == U16.{2})

    small_first_list :: [small, wide]
    wide_first_list :: [wide, small]
    print(small_first_list[0] == U16.{1})
    print(small_first_list[1] == U16.{2})
    print(wide_first_list[0] == U16.{2})
    print(wide_first_list[1] == U16.{1})

    exact :: Int.{9007199254740992}
    decimal :: Float.{1.0}
    exact_if :: if choose_first -> exact else -> decimal
    exact_list :: [exact, decimal]
    print(exact_if == 9007199254740992.0)
    print(exact_list[0] == 9007199254740992.0)

    lossy :: Int.{9007199254740993}
    rounded :: 9007199254740992.0
    approx_if :: if choose_first -> approx(lossy) else -> decimal
    approx_list :: [approx(lossy), decimal]
    print(approx_if == rounded)
    print(approx_list[0] == rounded)
}
"#;

    assert_all_tiers(
        "numeric_multi_producer_joins",
        source,
        0,
        "true\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\n",
    );
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

    let spread_widening = compile_error(
        r#"
fn run() {
    small :: [U8.{1}]
    _ :: [...small, U16.{2}]
}
"#,
    );
    assert!(
        spread_widening.contains("[E0504]")
            && spread_widening.contains("list resolves to `U16`"),
        "{spread_widening}"
    );
}

#[test]
fn bare_whole_literals_take_the_minimal_width_before_operator_join() {
    let source = r#"
fn run() {
    byte :: U8.{1}
    signed :: I8.{1}
    left :: byte + 256
    right :: 256 + byte
    print(left == U16.{257})
    print(right == U16.{257})
    print((signed + 1) == I8.{2})
    print((1 + signed) == I8.{2})
    print((byte + 1) == U8.{2})
    print((1 + byte) == U8.{2})
    print((I8.{0} + 127) == I8.{127})
    print((I16.{0} + 128) == I16.{128})
    print((I8.{0} + -128) == I8.{-128})
    print((I16.{0} + -129) == I16.{-129})
    print((U64.{0} + 9223372036854775807) == U64.{9223372036854775807})
}
"#;
    assert_all_tiers(
        "numeric_minimal_literal_join",
        source,
        0,
        "true\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\n",
    );

    let rendered = compile_error("fn run() { _ :: U8.{256} }");
    assert!(
        rendered.contains("[E1003]") && rendered.contains("U8") && rendered.contains("255"),
        "{rendered}"
    );

    for value in [
        "9223372036854775808",
        "18446744073709551615",
        "18446744073709551616",
    ] {
        let rendered = compile_error(&format!("fn run() {{ _ :: U64.{{{value}}} }}"));
        assert!(
            rendered.contains("[E0007]")
                && rendered.contains("numbers currently top out at 9223372036854775807"),
            "{rendered}"
        );
    }
    let rendered = compile_error("fn run() { _ :: -9223372036854775808 + I64.{0} }");
    assert!(
        rendered.contains("[E0007]")
            && rendered.contains("numbers currently top out at 9223372036854775807"),
        "{rendered}"
    );
}
