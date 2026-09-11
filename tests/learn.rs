//! `jet learn` acceptance: source-bound feedback stays offline and resumes from
//! the same canonical task identity as its packaged curriculum.

mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn jet(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .env("NO_COLOR", "1")
        .output()
        .unwrap_or_else(|error| panic!("jet {args:?} should start: {error}"))
}

fn combined(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn feedback_is_offline_and_source_bound() {
    let scratch = common::Scratch::new("learn-feedback");

    let first = jet(&["learn", "--watch=off", "--json"], &scratch.path);
    let first_text = combined(&first);
    assert!(
        !first.status.success(),
        "broken feedback witness unexpectedly passed:\n{first_text}"
    );
    assert!(
        first_text.contains("\"schema\":\"jet.learn/v1\""),
        "machine status envelope missing:\n{first_text}"
    );
    assert!(
        first_text.contains("\"task_id\":\"language.tir-stmt.loop.predict\""),
        "canonical task identity missing:\n{first_text}"
    );
    assert!(
        first_text.contains("\"source_identity\"")
            && first_text.contains("\"input_identity\"")
            && first_text.contains("\"oracle\""),
        "source-bound learning contract missing:\n{first_text}"
    );

    let source = scratch.path.join(".jet/learn/feedback/loop.jet");
    assert!(source.is_file(), "learn did not materialize the loop witness");
    assert!(
        fs::read_to_string(&source)
            .unwrap()
            .contains("0..<2"),
        "materialized witness is not the packaged broken source"
    );
    assert!(
        scratch
            .path
            .join(".jet/learn/feedback/progress/language_tir_stmt_loop_predict.json")
            .is_file(),
        "learn did not persist resumable progress"
    );

    let checked = jet(&["learn", "--check", "--json"], &scratch.path);
    let checked_text = combined(&checked);
    assert!(
        checked.status.success(),
        "packaged feedback check failed:\n{checked_text}"
    );
    assert!(
        checked_text.contains("\"action\":\"learn.check\"")
            && checked_text.contains("\"witnesses\""),
        "feedback check did not report its canonical witness envelope:\n{checked_text}"
    );
}
