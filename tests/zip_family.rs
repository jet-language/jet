mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use std::fs;

use jet::Interpreter::{dev_iteration, RunOutcome};
use tir_support::{build_and_run, have_rustc};

const SOURCE: &str = r#"fn run() {
    print(zip().to_list().len())
    print(zip([1, 2, 3]).to_list())
    print([1, 2, 3].zip([10, 20, 30]).unzip().a)
    print([1, 2, 3].zip_short([10, 20]).unzip().b)
    loop row, zip(a: [1, 2], b: ["x", "y"]) {
        print(row.b)
    }
    loop row, zip(a: [1, 2], b: [10, 20], c: [100, 200], d: [1000, 2000]) {
        print(row.d)
    }
    loop row, [1, 2, 3].zip_pad([10, 20]) {
        print(row.b)
    }
    left :: [1, 2, 3].take(3)
    right :: [10, 20].take(2)
    loop row, left.zip_pad(right) {
        print(row.b)
    }
    loop row, [1, 2, 3].zip_pad([10, 20], fill: 0) {
        print(row.b)
    }
    loop row, zip_pad(a: [1, 2, 3], b: [10, 20], fills: (a: 0, b: 9)) {
        print(row.b)
    }
}
"#;

const EXPECTED: &str = "0\n[1, 2, 3]\n[1, 2, 3]\n[10, 20]\nx\ny\n1000\n2000\n10\n20\nnull\n10\n20\nnull\n10\n20\n0\n10\n20\n9\n";

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
            RunOutcome::Problems(diagnostics) => panic!("{tier} rejected zip family: {diagnostics:#?}"),
        }
    }
}
