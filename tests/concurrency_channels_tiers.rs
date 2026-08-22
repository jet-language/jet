//! Card #1560 / D-CONC-CHAN1 / D-CONC-CHAN2: one channel/readiness meaning
//! through parser, sema, TIR, AOT, JIT, interpreter, comptime, REPL, and web.

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use std::fs;
use std::process::Command;

const SOURCE: &str = include_str!("../examples/features/concurrency/select_channel.jet");
const EXPECTED: &str = include_str!("../examples/features/expected/concurrency/select_channel.out");

const COMPTIME_SOURCE: &str = r#"
fn choose() Int {
    (sender, receiver) :: channel<Int>()
    sender.send(7)
    result := 0
    if {
        value, receiver -> result = value
        after 0ms -> result = -1
    }
    return result
}

@folded :: choose()

fn run() {
    print(@folded)
}
"#;

const REPL_SOURCE: &str = r#"
fn wait_for_value(receiver: Receiver<Int>) {
    if {
        value, receiver -> print(value)
        after 0ms -> print(-1)
    }
}

fn run() {
    (sender, receiver) :: channel<Int>()
    sender.send(7)
    wait_for_value(receiver)
}
"#;

#[test]
fn parser_reads_the_ratified_channel_surface() {
    let (tokens, diagnostics) = jet::Lexer::lex(SOURCE);
    assert!(diagnostics.is_empty(), "lexer diagnostics: {diagnostics:?}");
    assert!(
        jet::Parser::parse(&tokens).is_ok(),
        "channel/readiness example must parse"
    );
}

#[test]
fn sema_accepts_plain_endpoint_readiness_and_drain_surface() {
    jet::compile(SOURCE).expect("channel/readiness example must pass sema");
}

#[test]
fn tir_keeps_one_private_readiness_wait_door() {
    let output = jet::compile_with_path(SOURCE, "examples/features/concurrency/select_channel.jet")
        .expect("channel/readiness example must lower");
    assert!(
        output.rust.contains("jet_select_wait_tagged"),
        "TIR/AOT must marshal readiness through the shared Prelude door:\n{}",
        output.rust
    );
    assert!(
        output.rust.contains("jet_std::channel"),
        "the builtin channel must lower through the existing channel Prelude:\n{}",
        output.rust
    );
}

#[test]
fn aot_runs_the_channel_readiness_example() {
    if tir_support::have_rustc() {
        let (code, stdout, stderr) =
            tir_support::build_and_run_full("jet_channel_tiers", "aot", SOURCE);
        assert_eq!(code, 0, "AOT failed: {stderr}");
        assert_eq!(stdout, EXPECTED, "AOT output drifted: {stderr}");
    }
}

#[test]
fn jit_runs_the_channel_readiness_example() {
    let (code, stdout, stderr) = tir_support::jit_run("channel_tiers_jit", SOURCE);
    assert_eq!(code, 0, "default jet run failed: {stderr}");
    assert_eq!(stdout, EXPECTED, "default jet run output drifted: {stderr}");
}

#[test]
fn interpreter_runs_the_channel_readiness_example() {
    let (code, stdout, stderr) =
        tir_support::interpreter_run("channel_tiers_interpreter", SOURCE);
    assert_eq!(code, 0, "interpreter failed: {stderr}");
    assert_eq!(
        stdout, EXPECTED,
        "interpreter output drifted from the channel Prelude: {stderr}"
    );
}

#[test]
fn comptime_folds_a_channel_send_and_readiness_wait() {
    let output = jet::compile(COMPTIME_SOURCE);
    assert!(
        output.is_ok(),
        "comptime channel/select fold must use the shared evaluator: {:#?}",
        output.err()
    );
}

#[test]
fn repl_runs_plain_endpoint_readiness() {
    let transcript = jet::REPL::run_transcript(&[REPL_SOURCE, "run()"], None);
    assert_eq!(
        transcript,
        format!("ok\n{EXPECTED_REPL}"),
        "REPL channel/readiness output drifted"
    );
}

const EXPECTED_REPL: &str = "7\n";

#[test]
fn web_runs_the_same_channel_readiness_example() {
    let scratch = common::Scratch::new("channel-readiness-web");
    let output = jet::compile_web_with_path(
        SOURCE,
        "examples/features/concurrency/select_channel.jet",
    )
    .unwrap_or_else(|diagnostics| {
        panic!(
            "web target rejected channel/readiness:\n{}",
            jet::render_diagnostics(
                "examples/features/concurrency/select_channel.jet",
                SOURCE,
                &diagnostics
            )
        )
    });
    let web = output.web.expect("web target must produce artifacts");
    assert!(
        web.js_app.contains("jet_scheduler_select"),
        "web must marshal readiness through the scheduler Prelude"
    );
    assert!(
        web.js_app.contains("jet_channel_new"),
        "web must marshal the builtin channel through the channel Prelude"
    );
    fs::write(scratch.join("app.js"), &web.js_app).unwrap();
    fs::write(scratch.join("jet_dom_runtime.js"), &web.dom_runtime).unwrap();
    fs::write(scratch.join("package.json"), r#"{"type":"module"}"#).unwrap();
    if Command::new("node").arg("--version").output().is_ok() {
        let node = Command::new("node")
            .current_dir(&scratch.path)
            .arg("app.js")
            .output()
            .expect("spawn node");
        assert!(
            node.status.success(),
            "web channel example failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&node.stdout),
            String::from_utf8_lossy(&node.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&node.stdout), EXPECTED);
    }
}
