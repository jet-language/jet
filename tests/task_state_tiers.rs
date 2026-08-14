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
fn web_matches_expected_task_output() {
    let scratch = common::Scratch::new("task-state-web");
    let entry = scratch.join("app.jet");
    fs::write(&entry, SOURCE).unwrap();
    let shown = entry.to_string_lossy();
    let output = jet::compile_web_with_path(SOURCE, &shown).unwrap_or_else(|diagnostics| {
        panic!(
            "front end rejected web fixture:\n{}",
            jet::render_diagnostics(&shown, SOURCE, &diagnostics)
        )
    });
    let web = output.web.expect("web target must produce web artifacts");
    fs::write(scratch.join("app.js"), &web.js_app).unwrap();
    fs::write(scratch.join("app_wasm.rs"), &web.wasm_rust).unwrap();
    fs::write(scratch.join("jet_dom_runtime.js"), &web.dom_runtime).unwrap();
    fs::write(scratch.join("package.json"), r#"{"type":"module"}"#).unwrap();
    let rustc = Command::new("rustc")
        .current_dir(&scratch.path)
        .args([
            "--edition",
            "2021",
            "--target",
            "wasm32-unknown-unknown",
            "--crate-type",
            "cdylib",
            "-O",
            "app_wasm.rs",
            "-o",
            "app.wasm",
        ])
        .output()
        .expect("spawn rustc");
    assert!(
        rustc.status.success(),
        "rustc rejected web task fixture:\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let node = Command::new("node")
        .current_dir(&scratch.path)
        .arg("app.js")
        .output()
        .expect("spawn node");
    assert!(
        node.status.success(),
        "web task fixture failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&node.stdout),
        String::from_utf8_lossy(&node.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&node.stdout), EXPECTED);
}
