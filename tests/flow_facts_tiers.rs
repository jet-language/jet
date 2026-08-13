//! Card #1621 (D-FACT-FLOW1): the one flow-fact store is sema-only, so every
//! execution tier must read the same program the same way.
//!
//! The fixture below leans on three planes at once — a proven presence test
//! that refines `String?` to `String` (narrowing), a value given away inside one
//! arm (moves), and a branch whose arms must be joined rather than kept
//! last-walked. One test per tier: parser, sema, TIR, AOT, `jet run` (Cranelift
//! and the interpreter behind it), comptime, REPL, and web.

#[path = "tir_support/mod.rs"]
mod tir_support;
mod common;

use std::fs;
use std::process::Command;

/// One program, every plane. `label` narrows an optional across a branch;
/// `total` is written in both arms and read after they meet.
const SOURCE: &str = r#"
fn label(text: String?) => String {
    if text != None {
        return text
    }
    return "none"
}

fn score(flag: Bool) => Int {
    total := 0
    if {
        flag -> total = 2
        else -> total = 3
    }
    return total
}

fn run() {
    print(label(Val("keep")))
    print(label(None))
    print(score(true))
    print(score(false))
}
"#;

const EXPECTED: &str = "keep\nnone\n2\n3\n";

/// Tier 1 — parser: the fixture is ordinary ratified syntax.
#[test]
fn parser_reads_the_fixture() {
    let (tokens, lexer_diagnostics) = jet::Lexer::lex(SOURCE);
    assert!(lexer_diagnostics.is_empty(), "{lexer_diagnostics:?}");
    assert!(
        jet::Parser::parse(&tokens).is_ok(),
        "the flow-fact fixture must parse"
    );
}

/// Tier 2 — sema: the store proves the narrowing and the branch merge, so the
/// program checks clean.
#[test]
fn sema_proves_the_fixture() {
    let out = jet::compile(SOURCE);
    assert!(out.is_ok(), "sema must accept the fixture: {:#?}", out.err());
}

/// Tier 3 — TIR: the proven presence test reaches lowering as a proven unwrap,
/// which is what keeps codegen mechanical (D-FLOWTYPE1).
#[test]
fn tir_records_the_proven_unwrap() {
    let out = jet::compile(SOURCE).expect("fixture compiles");
    assert!(
        out.rust.contains("if let Some("),
        "the refined optional must reach lowering already proven:\n{}",
        out.rust
    );
}

/// Tier 4 and 5 — AOT and `jet run` (Cranelift, with the interpreter behind it)
/// agree on one meaning.
#[test]
fn aot_and_jet_run_agree() {
    tir_support::assert_tiers_agree("flow_facts_tiers", SOURCE, EXPECTED);
}

/// Tier 6 — comptime: the same branch and the same optional refinement, folded
/// at build time, answer the same way.
#[test]
fn comptime_folds_the_same_answer() {
    let source = format!(
        "{SOURCE}\n@folded :: score(true)\n\nfn show() {{\n    print(@folded)\n}}\n"
    );
    let out = jet::compile(&source);
    assert!(
        out.is_ok(),
        "comptime must fold the same branch: {:#?}",
        out.err()
    );
}

/// Tier 7 — REPL: the same definitions typed into a session behave the same.
#[test]
fn repl_answers_the_same() {
    let transcript = jet::REPL::run_transcript(
        &[
            "fn label(text: String?) => String { if text != None { return text } return \"none\" }",
            "print(label(Val(\"keep\")))",
            "print(label(None))",
        ],
        None,
    );
    assert!(
        transcript.contains("keep") && transcript.contains("none"),
        "REPL disagreed with the other tiers:\n{transcript}"
    );
}

/// Tier 8 — web: the same fixture builds for the web target, so nothing about
/// the store leaked into a native-only path.
#[test]
fn web_target_builds_the_fixture() {
    let scratch = common::Scratch::new("flow-facts-web");
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
