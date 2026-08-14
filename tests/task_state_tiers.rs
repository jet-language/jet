//! Tower #1557: task state and join duty keep one meaning across the tier
//! boundaries that support native task execution.

#[path = "tir_support/mod.rs"]
mod tir_support;
mod common;

use std::fs;
use std::process::Command;

const SOURCE: &str = r#"
fn add(left: Int, right: Int) => Int {
    return left + right
}

fn run() {
    joined :: task add(40, 2)
    print(joined.join() ?? 0)
    detached :: task 0
    detached.detach()
}
"#;

const EXPECTED: &str = "42\n";

#[test]
fn parser_reads_the_canonical_task_surface() {
    let (tokens, diagnostics) = jet::Lexer::lex(SOURCE);
    assert!(diagnostics.is_empty(), "lexer diagnostics: {diagnostics:?}");
    assert!(jet::Parser::parse(&tokens).is_ok(), "task fixture must parse");
}

#[test]
fn sema_tracks_task_state_and_duty_without_a_new_surface() {
    let output = jet::compile(SOURCE).expect("task fixture must pass sema");
    assert!(
        output.lints.iter().all(|diagnostic| diagnostic.code != "L1101"),
        "join and detach must discharge the shared duty: {:?}",
        output.lints
    );
}

#[test]
fn tir_erases_task_state_facts_before_codegen() {
    let output = jet::compile(SOURCE).expect("task fixture must compile");
    assert!(
        !output.rust.contains("Task.State"),
        "compiler-owned task state must not reach generated Rust"
    );
}

#[test]
fn aot_jit_and_interpreter_agree() {
    tir_support::assert_tiers_agree("task_state_tiers", SOURCE, EXPECTED);
}

#[test]
fn comptime_keeps_task_facts_in_the_front_end() {
    let source = format!(
        "{SOURCE}\n@folded :: 40 + 2\n\nfn show() {{\n    print(@folded)\n}}\n"
    );
    jet::compile(&source).expect("comptime and runtime task code must share the front end");
}

#[test]
fn repl_matches_canonical_task_output() {
    let transcript = jet::REPL::run_transcript(&[SOURCE, "run()"], None);
    assert_eq!(
        transcript,
        format!("ok\n{EXPECTED}"),
        "REPL task output must match the canonical source"
    );
}

#[test]
fn web_keeps_the_existing_task_tir_boundary() {
    let scratch = common::Scratch::new("task-state-web");
    let entry = scratch.join("app.jet");
    fs::write(&entry, SOURCE).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["check", entry.to_str().unwrap(), "--target", "web"])
        .env("NO_COLOR", "1")
        .output()
        .expect("spawn jet");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !output.status.success() && combined.contains("E-WEB-TIR-UNSUPPORTED"),
        "web must keep native task code at its existing TIR boundary:\n{combined}"
    );
}
