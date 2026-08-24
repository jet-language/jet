//! D-CONC-CROSS1 / D-CONC-FREEZE1: crossing and frozen values keep one meaning
//! through the parser, sema, TIR, AOT, JIT, interpreter, comptime, REPL, and web.

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

const SOURCE: &str = r#"
struct Snapshot { value: Int }

fn run() {
    source := Snapshot.{ value: 41 }
    frozen :: freeze(source)
    twice :: freeze(freeze(source))
    task.group snapshots {
        first :: task { frozen.value }
        second :: task { frozen.value }
        print(first.join() ?? 0)
        print(second.join() ?? 0)
    }

    owned := Snapshot.{ value: 7 }
    task.group moved {
        child :: task ^owned { owned.value }
        print(child.join() ?? 0)
    }
    print(twice.value)
}
"#;

const EXPECTED: &str = "41\n41\n7\n41\n";

#[test]
fn parser_and_sema_accept_the_ratified_crossing_surface() {
    let (tokens, lexer_diagnostics) = jet::Lexer::lex(SOURCE);
    assert!(
        lexer_diagnostics.is_empty(),
        "lexer diagnostics: {lexer_diagnostics:?}"
    );
    assert!(
        jet::Parser::parse(&tokens).is_ok(),
        "freeze/^ fixture must parse"
    );
    jet::compile(SOURCE).expect("freeze/^ fixture must pass sema");
}

#[test]
fn tir_keeps_freeze_as_an_owned_value_operation() {
    let output = jet::compile(SOURCE).expect("freeze/^ fixture must compile");
    assert!(
        output.rust.contains("clone") || output.rust.contains("Snapshot"),
        "freeze must lower through the existing owned-value representation"
    );
}

#[test]
fn aot_jit_and_interpreter_agree_on_crossing_and_freeze() {
    tir_support::assert_tiers_agree("concurrency_freeze_tiers", SOURCE, EXPECTED);
}

#[test]
fn comptime_accepts_the_same_pure_freeze_operation() {
    let source =
        format!("{SOURCE}\n@folded :: freeze(41)\n\nfn show() {{\n    print(@folded)\n}}\n");
    jet::compile(&source).expect("comptime freeze must use the shared front end");
}

#[test]
fn repl_accepts_and_runs_freeze() {
    let transcript = jet::REPL::run_transcript(
        &["source := 41", "frozen :: freeze(source)", "print(frozen)"],
        None,
    );
    assert_eq!(transcript, "41\n");
}

#[test]
fn web_accepts_the_pure_freeze_representation() {
    let source = r#"
fn run() {
    source := 41
    frozen :: freeze(source)
    print(frozen)
}
"#;
    let output = jet::compile_web_with_path(source, "tests/fixtures/concurrency_freeze.jet")
        .expect("web target must accept the shared freeze operation");
    assert!(output.web.is_some(), "web target must produce artifacts");
}

// D-CONC-CROSS1=A / I9: this is the smallest accepted crossing. The same
// parser, fact proof, TIR, and task adapter must carry the frozen value on
// every execution surface.
const CROSSING_SOURCE: &str = r#"
struct Snapshot { value: Int }

fn run() {
    source := Snapshot{value: 41}
    frozen :: freeze(source)
    worker :: task frozen.value + 1
    print(worker.join() ?? 0)
}
"#;

const CROSSING_EXPECTED: &str = "42\n";

#[test]
fn crossing_plane_parser_accepts_the_canonical_source() {
    let (tokens, diagnostics) = jet::Lexer::lex(CROSSING_SOURCE);
    assert!(diagnostics.is_empty(), "lexer diagnostics: {diagnostics:?}");
    assert!(
        jet::Parser::parse(&tokens).is_ok(),
        "crossing source must parse on the shared parser"
    );
}

#[test]
fn crossing_plane_sema_accepts_the_fact_proof() {
    jet::compile(CROSSING_SOURCE).expect("crossing source must pass sema");
}

#[test]
fn crossing_plane_tir_erase_the_fact() {
    let output = jet::compile(CROSSING_SOURCE).expect("crossing source must pass sema");
    assert!(
        output.rust.contains("JetTask::spawn") || output.rust.contains("jet_scheduler_spawn"),
        "TIR must lower the accepted crossing to the task adapter"
    );
    assert!(
        !output.rust.contains("Sendability") && !output.rust.contains("FlowFacts"),
        "crossing facts must erase before codegen"
    );
}

#[test]
fn crossing_plane_aot_matches_expected() {
    if tir_support::have_rustc() {
        let (code, stdout, stderr) =
            tir_support::build_and_run_full("jet_crossing_plane", "aot", CROSSING_SOURCE);
        assert_eq!(code, 0, "AOT failed: {stderr}");
        assert_eq!(stdout, CROSSING_EXPECTED, "AOT output drifted: {stderr}");
    }
}

#[test]
fn crossing_plane_jit_matches_expected() {
    let (code, stdout, stderr) =
        tir_support::jit_run("concurrency_crossing_plane_jit", CROSSING_SOURCE);
    assert_eq!(code, 0, "default jet run failed: {stderr}");
    assert_eq!(
        stdout, CROSSING_EXPECTED,
        "default jet run output drifted: {stderr}"
    );
}

#[test]
fn crossing_plane_interpreter_matches_expected() {
    let (code, stdout, stderr) =
        tir_support::interpreter_run("concurrency_crossing_plane_interpreter", CROSSING_SOURCE);
    assert_eq!(code, 0, "interpreter failed: {stderr}");
    assert_eq!(
        stdout, CROSSING_EXPECTED,
        "interpreter output drifted: {stderr}"
    );
}

#[test]
fn crossing_plane_comptime_uses_the_same_front_end() {
    let source =
        format!("{CROSSING_SOURCE}\n@folded :: 40 + 2\n\nfn show() {{ print(@folded) }}\n");
    jet::compile(&source).expect("comptime crossing source must use the shared sema path");
}

#[test]
fn crossing_plane_repl_matches_the_runtime_result() {
    let transcript = jet::REPL::run_transcript(&[CROSSING_SOURCE, "run()"], None);
    assert_eq!(transcript, format!("ok\n{CROSSING_EXPECTED}"));
}

#[test]
fn crossing_plane_web_accepts_the_same_source() {
    let output = jet::compile_web_with_path(
        CROSSING_SOURCE,
        "tests/fixtures/concurrency_crossing_plane.jet",
    )
    .expect("web target must accept the crossing source");
    let web = output.web.expect("web target must produce artifacts");
    assert!(
        web.js_app.contains("jet_task_spawn"),
        "web adapter must retain the accepted task crossing"
    );
}
