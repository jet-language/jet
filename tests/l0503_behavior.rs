//! Focused #2416 proof for L0503's registered wording and runtime behavior.
//!
//! The UI snapshot owns the rendered advisory. This test keeps its structured
//! subject/span/What/Why/Fix facts next to a tier-complete execution witness,
//! without making the runtime proof depend on warning text emitted by one CLI
//! adapter.

mod common;
mod tir_support;

use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

use jet::Interpreter::RunOutcome;

const SOURCE: &str = include_str!("ui_lint/prefer_compound_assign.jet");
const EXPECTED_STDOUT: &str = "80\n";
const WHY: &str = "compound assignment updates a place in one step without restating it";

const EXPECTED_L0503: &[(&str, usize, usize, &str, &str)] = &[
    (
        "p.hp",
        5,
        43,
        "prefer `p.hp -= …` instead of repeating the left side",
        "write `p.hp -= …`",
    ),
    (
        "p.hp",
        7,
        41,
        "prefer `p.hp += …` instead of repeating the left side",
        "write `p.hp += …`",
    ),
    (
        "n",
        11,
        7,
        "prefer `n += …` instead of repeating the left side",
        "write `n += …`",
    ),
    (
        "n",
        15,
        7,
        "prefer `n += …` instead of repeating the left side",
        "write `n += …`",
    ),
];

#[test]
fn l0503_fixture_keeps_exact_subject_span_and_policy() {
    let path = "tests/ui_lint/prefer_compound_assign.jet";
    let compiled = jet::compile_with_path(SOURCE, path)
        .unwrap_or_else(|diags| panic!("L0503 fixture must compile: {diags:?}"));
    let mut l0503 = compiled
        .lints
        .iter()
        .filter(|diagnostic| diagnostic.code == "L0503")
        .collect::<Vec<_>>();
    l0503.sort_by_key(|diagnostic| diagnostic.span.map_or(usize::MAX, |span| span.start));

    assert_eq!(l0503.len(), EXPECTED_L0503.len(), "L0503 policy changed");
    assert_eq!(compiled.lints.len(), EXPECTED_L0503.len(), "fixture gained an incidental lint");

    for (diagnostic, (subject, expected_line, expected_column, expected_what, expected_fix)) in
        l0503.iter().zip(EXPECTED_L0503)
    {
        assert_eq!(diagnostic.what, *expected_what);
        assert_eq!(diagnostic.why, WHY);
        assert_eq!(diagnostic.fix, *expected_fix);
        let span = diagnostic.span.expect("L0503 must identify its assignment operator");
        assert_eq!(&SOURCE[span.start..span.end], "=");
        let line_start = SOURCE[..span.start].rfind('\n').map_or(0, |offset| offset + 1);
        let line = SOURCE[..span.start].bytes().filter(|byte| *byte == b'\n').count() + 1;
        assert_eq!(line, *expected_line);
        assert_eq!(span.start - line_start + 1, *expected_column);
        let assignment_line = SOURCE[line_start..]
            .lines()
            .next()
            .expect("L0503 assignment line");
        assert!(
            assignment_line.contains(&format!("{subject} = {subject}"))
                || assignment_line.contains(&format!("{subject} = ({subject}")),
            "L0503 subject disappeared from source line: {assignment_line}"
        );
    }

    // S17's quiet cases remain quiet: an already-compound update and an
    // indexed expansion, which is reserved for E0164, must not gain L0503.
    assert!(SOURCE.contains("n += 1"));
    assert!(SOURCE.contains("xs[0] = xs[0] + 1"));
}

static DEV_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn run_dev(force_interpreter: bool) -> (i32, String, String) {
    let id = DEV_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "jet_l0503_dev_{}_{}",
        std::process::id(),
        id
    ));
    fs::create_dir_all(&dir).expect("L0503 dev scratch directory");
    let path = dir.join("prefer_compound_assign.jet");
    fs::write(&path, SOURCE).expect("L0503 dev source");
    fs::write(
        dir.join("package.jet"),
        "name: \"l0503_behavior\"\nversion: \"0.1.0\"\nauthority: { holds: { allow: [IO, Mem.Alloc] } }\n",
    )
    .expect("L0503 dev package");

    let outcome = jet::Interpreter::dev_iteration(
        path.to_str().expect("L0503 dev path"),
        false,
        force_interpreter,
    );
    let result = match outcome {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => (exit_code, stdout, stderr),
        RunOutcome::Problems(diags) => panic!(
            "L0503 fixture failed on {} dev tier: {diags:?}",
            if force_interpreter {
                "forced interpreter"
            } else {
                "default"
            }
        ),
    };
    let _ = fs::remove_dir_all(dir);
    result
}

fn assert_runtime_output(tier: &str, result: &(i32, String, String)) {
    assert_eq!(result.0, 0, "{tier} L0503 fixture failed: {}", result.2);
    assert_eq!(result.1, EXPECTED_STDOUT, "{tier} L0503 output drifted");
}

#[test]
fn l0503_fixture_runs_on_aot_default_dev_and_interpreter() {
    let (default_code, default_stdout, default_stderr) =
        tir_support::jit_run("l0503_behavior_default", SOURCE);
    assert_runtime_output(
        "default jet run",
        &(default_code, default_stdout, default_stderr),
    );

    let forced_interpreter = tir_support::interpreter_run("l0503_behavior_interpreter", SOURCE);
    assert_runtime_output("forced interpreter", &forced_interpreter);

    let default_dev = run_dev(false);
    assert_runtime_output("default dev", &default_dev);

    let dev_interpreter = run_dev(true);
    assert_runtime_output("dev forced interpreter", &dev_interpreter);

    if tir_support::have_rustc() {
        let aot = tir_support::build_and_run_full("jet_l0503_aot", "l0503_behavior_aot", SOURCE);
        assert_runtime_output("AOT", &aot);
    }
}
