mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use std::fs;

use jet::Interpreter::{dev_iteration, RunOutcome};
use tir_support::{build_and_run, build_and_run_full, have_rustc, interpreter_run, jit_run};

const SOURCE: &str = r#"fn run() {
    print(zip().to_list().len())
    print(zip([1, 2, 3]).to_list())
    print([1, 2, 3].zip([10, 20, 30]).unzip().a)
    print([1, 2, 3].zip_short([10, 20]).unzip().b)
    loop row in zip(a: [1, 2], b: ["x", "y"]) {
        print(row.b)
    }
    loop row in zip(a: [1, 2], b: [10, 20], c: [100, 200]) {
        print(row.c)
    }
    loop row in zip(a: [1, 2], b: [10, 20], c: [100, 200], d: [1000, 2000]) {
        print(row.d)
    }
    loop row in [1, 2, 3].zip_pad([10, 20]) {
        print(row.b)
    }
    left :: [1, 2, 3].take(3)
    right :: [10, 20].take(2)
    loop row in left.zip_pad(right) {
        print(row.b)
    }
    loop row in [1, 2, 3].zip_pad([10, 20], fill: 0) {
        print(row.b)
    }
    loop row in zip_pad(a: [1, 2, 3], b: [10, 20], fills: (a: 0, b: 9)) {
        print(row.b)
    }
}
"#;

const EXPECTED: &str = "0\n[1, 2, 3]\n[1, 2, 3]\n[10, 20]\nx\ny\n100\n200\n1000\n2000\n10\n20\nnull\n10\n20\nnull\n10\n20\n0\n10\n20\n9\n";

const STRICT_MISMATCH_SOURCE: &str = r#"use core.process as process

fn run() {
    count :: process.args().len()
    left :: [1, 2, 3].take(count)
    right :: [10, 20].take(2)
    loop row in left.zip(right) {
        print(row.a)
    }
}
"#;

#[test]
fn strict_zip_mismatch_reports_e0128_across_tiers() {
    let mut results = vec![
        ("default JIT", jit_run("zip_strict_mismatch", STRICT_MISMATCH_SOURCE)),
        (
            "forced interpreter",
            interpreter_run("zip_strict_mismatch", STRICT_MISMATCH_SOURCE),
        ),
    ];
    if have_rustc() {
        results.push((
            "AOT",
            build_and_run_full(
                "zip_strict_mismatch",
                "zip_strict_mismatch",
                STRICT_MISMATCH_SOURCE,
            ),
        ));
    }

    let mut baseline = None;
    for (tier, (code, stdout, stderr)) in results {
        assert_eq!(code, 70, "{tier} must exit with the registered runtime stop: {stderr}");
        let lower = stderr.to_ascii_lowercase();
        for needle in [
            "stop [e0128]",
            "inputs have different lengths under `strict` policy",
            "strict `zip` requires every input to end on the same row",
            "zip_short",
            "truncate",
            "zip_pad",
            "pad",
            "neither changes strictness automatically",
            "loop row in left.zip(right)",
        ] {
            assert!(lower.contains(needle), "{tier} missing {needle:?}: {stderr}");
        }
        assert!(!stderr.contains("E3001"), "{tier} regressed to E3001: {stderr}");
        assert!(
            !stderr.contains("<core.collections>:0"),
            "{tier} lost the user zip location: {stderr}"
        );
        if let Some((baseline_tier, expected)) = &baseline {
            assert_eq!(
                &(code, stdout.clone(), stderr.clone()),
                expected,
                "{tier} diverged from {baseline_tier}"
            );
        } else {
            baseline = Some((tier, (code, stdout, stderr)));
        }
    }
}

#[test]
fn zip_family_matches_aot_default_and_forced_interpreter() {
    if !have_rustc() {
        return;
    }
    let (code, aot_stdout) = build_and_run("zip_family", SOURCE);
    assert_eq!(code, 0);
    assert_eq!(aot_stdout, EXPECTED);

    let dir = std::env::temp_dir().join(format!("jet_zip_family_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.jet");
    fs::write(&path, SOURCE).unwrap();
    let shown = path.to_string_lossy().into_owned();
    for (tier, force_interpreter) in [("default tier", false), ("forced interpreter", true)] {
        match dev_iteration(&shown, false, force_interpreter) {
            RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => {
                assert_eq!(exit_code, 0, "{tier} exit");
                assert_eq!(stderr, "", "{tier} stderr");
                assert_eq!(stdout, EXPECTED, "{tier} output");
            }
            RunOutcome::Problems(diagnostics) => {
                panic!("{tier} rejected zip family: {diagnostics:#?}")
            }
        }
    }
}
