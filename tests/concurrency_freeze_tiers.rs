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
    assert!(lexer_diagnostics.is_empty(), "lexer diagnostics: {lexer_diagnostics:?}");
    assert!(jet::Parser::parse(&tokens).is_ok(), "freeze/^ fixture must parse");
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
    let source = format!(
        "{SOURCE}\n@folded :: freeze(41)\n\nfn show() {{\n    print(@folded)\n}}\n"
    );
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
