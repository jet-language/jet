//! Card #1547 criterion 7: one fact declaration stays a declaration-only item
//! through every applicable frontend and execution tier.

#[path = "tir_support/mod.rs"]
mod tir_support;
mod common;

use std::fs;
use std::process::Command;

use jet::AST::Item;
use jet_foundation::Registry;

const SOURCE: &str = r#"
fact Exactness(@holds: .Value, @safe: .Gain, @gates: [approx, raw], @decision: "D-TEST")

fn run() {
    print("fact declaration")
}
"#;

const COMPTIME_SOURCE: &str = r#"
fact Flow(@holds: .Value, @safe: .Gain, @gates: [], @decision: "D-TEST")
@answer :: "fact declaration"

fn run() {
    print(@answer)
}
"#;

const EXPECTED: &str = "fact declaration\n";

/// Parser: the real parser preserves the source declaration as an AST item.
#[test]
fn parser_reads_the_fact_source_and_fixture() {
    let (tokens, lexer_diagnostics) = jet::Lexer::lex(Registry::FACT_SOURCE);
    assert!(lexer_diagnostics.is_empty(), "lex: {lexer_diagnostics:?}");
    let program = jet::Parser::parse(&tokens).expect("Facts.jet must parse");
    let parsed_names: Vec<_> = program
        .items
        .iter()
        .filter_map(|item| match item {
            Item::FactDecl(declaration) => Some(declaration.name.as_str()),
            _ => None,
        })
        .collect();
    let registry_names: Vec<_> = Registry::fact_declarations()
        .iter()
        // `$name` is the registered dotted identity for compiler planes;
        // `source_name` is the ordinary fact identifier the parser sees.
        .map(|declaration| declaration.source_name)
        .collect();
    assert_eq!(parsed_names, registry_names);

    let (tokens, lexer_diagnostics) = jet::Lexer::lex(SOURCE);
    assert!(
        lexer_diagnostics.is_empty(),
        "lex fixture: {lexer_diagnostics:?}"
    );
    assert!(
        jet::Parser::parse(&tokens)
            .expect("fact fixture must parse")
            .items
            .iter()
            .any(|item| matches!(
                item,
                Item::FactDecl(declaration) if declaration.name == "Exactness"
            ))
    );
}

/// Registry: the one table preserves the declaration's typed law metadata.
#[test]
fn registry_reads_the_same_fact_columns() {
    let declaration = Registry::fact_declarations()
        .iter()
        .find(|declaration| declaration.name == "Exactness")
        .expect("Exactness is written in Facts.jet");
    let row = Registry::row("Exactness").expect("Exactness is in the one registry");
    assert_eq!(row.target, declaration.target);
    assert_eq!(row.safe_direction, declaration.safe_direction);
    assert_eq!(row.gates, declaration.gates);
    assert_eq!(row.decision, declaration.decision);
}

/// Sema: a fact declaration is an erased item beside executable code.
#[test]
fn sema_accepts_the_fixture() {
    let out = jet::compile(SOURCE);
    assert!(
        out.is_ok(),
        "sema rejected the fact fixture: {:#?}",
        out.err()
    );
}

/// TIR/codegen receives only the executable function, not the fact columns.
#[test]
fn tir_erases_the_declaration() {
    let out = jet::compile(SOURCE).expect("fact fixture compiles");
    assert!(out.rust.contains("fact declaration"));
    assert!(!out.rust.contains("@holds"));
    assert!(!out.rust.contains("D-TEST"));
}

/// AOT and default `jet run` agree on the executable part.
#[test]
fn aot_and_jet_run_agree() {
    tir_support::assert_tiers_agree("fact_declaration_tiers", SOURCE, EXPECTED);
}

/// The default resident JIT and the forced interpreter both execute the same
/// erased program. The default path must not fall back or deopt for a
/// declaration-only fact item.
#[test]
fn resident_jit_and_forced_interpreter_agree_without_fallback() {
    let scratch = common::Scratch::new("fact-declaration-tiers");
    let entry = scratch.join("app.jet");
    fs::write(&entry, SOURCE).unwrap();
    let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap()).unwrap();
    let errors: Vec<_> = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
        .into_iter()
        .filter(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "fact fixture rejected before tier proof: {errors:?}");
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle),
        "fact fixture is not resident-JIT safe: {}",
        jet_jit::resident_jit_safe_bundle_detail(&bundle)
    );
    jet_jit::try_compile_bundle(&bundle).expect("fact fixture must compile in resident JIT");

    jet_jit::reset_jit_trace_for_test();
    match jet::Interpreter::dev_iteration(entry.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            assert_eq!(exit_code, 0, "resident JIT failed: {stderr}");
            assert_eq!(stderr, "");
            assert_eq!(stdout, EXPECTED);
        }
        jet::Interpreter::RunOutcome::Problems(diagnostics) => {
            panic!("resident JIT rejected fact fixture: {diagnostics:?}")
        }
    }
    assert!(jet_jit::jit_executed_for_test(), "resident JIT did not execute");
    assert!(
        !jet_jit::fallback_invoked_for_test() && !jet_jit::deopt_invoked_for_test(),
        "fact fixture used a fallback or interpreter deopt"
    );

    jet_jit::reset_jit_trace_for_test();
    match jet::Interpreter::dev_iteration(entry.to_str().unwrap(), false, true) {
        jet::Interpreter::RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            assert_eq!(exit_code, 0, "forced interpreter failed: {stderr}");
            assert_eq!(stderr, "");
            assert_eq!(stdout, EXPECTED);
        }
        jet::Interpreter::RunOutcome::Problems(diagnostics) => {
            panic!("forced interpreter rejected fact fixture: {diagnostics:?}")
        }
    }
}

/// Comptime still folds beside a declaration-only fact item.
#[test]
fn comptime_accepts_the_fixture() {
    let out = jet::compile(COMPTIME_SOURCE).expect("comptime fact fixture compiles");
    assert!(out.rust.contains("fact declaration"));
}

/// REPL accepts the same declaration-only item before executing code.
#[test]
fn repl_accepts_the_fixture() {
    let transcript = jet::REPL::run_transcript(
        &[
            "fact Flow(@holds: .Value, @safe: .Gain, @gates: [], @decision: \"D-TEST\")",
            "print(\"fact declaration\")",
        ],
        None,
    );
    assert!(
        transcript.contains("fact declaration"),
        "REPL rejected or lost the declaration-only item:\n{transcript}"
    );
}

/// Web checking accepts the same source without a native-only reflection path.
#[test]
fn web_target_accepts_the_fixture() {
    let scratch = common::Scratch::new("fact-declaration-web");
    let entry = scratch.join("app.jet");
    fs::write(&entry, SOURCE).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["check", entry.to_str().unwrap(), "--target", "web"])
        .env("NO_COLOR", "1")
        .output()
        .expect("spawn jet");
    assert!(
        out.status.success(),
        "web check failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
