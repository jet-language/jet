//! Card #1551 c9: exact defaults and the explicit Float opt-in share one
//! meaning across every front end and execution tier.

use std::fs;
use std::process::Command;

mod common;
#[path = "tir_support/mod.rs"]
mod tir_support;

const SOURCE: &str = r#"
fn run() {
    third :: 1 / 3
    print(third)
    print(third * 3 == 1)
    print(0.1 + 0.2 == 0.3)
    fast :: Float{19.99}
    print(fast)
}
"#;

const EXPECTED: &str = "1/3\ntrue\ntrue\n19.99\n";

fn web_source() -> String {
    format!("#Target(Web)\n{SOURCE}")
}

fn have_tool(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn have_wasm_target() -> bool {
    Command::new("rustc")
        .args([
            "--print",
            "target-libdir",
            "--target",
            "wasm32-unknown-unknown",
        ])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[test]
fn exact_numbers_parser_accepts_the_canonical_surface() {
    let (tokens, diagnostics) = jet::Lexer::lex(SOURCE);
    assert!(
        diagnostics.is_empty(),
        "lexer rejected exact numbers: {diagnostics:?}"
    );
    assert!(
        jet::Parser::parse(&tokens).is_ok(),
        "parser rejected exact numbers"
    );
}

#[test]
fn exact_numbers_sema_accepts_exact_defaults_and_float_opt_in() {
    tir_support::compile("exact_number_tiers_sema", SOURCE);
}

#[test]
fn exact_numbers_tir_keeps_the_numeric_prelude_carriers() {
    let compiled = tir_support::compile("exact_number_tiers_tir", SOURCE);
    assert!(
        compiled.contains("jet_decimal") || compiled.contains("JetDecimal"),
        "TIR lost the exact Decimal carrier:\n{}",
        compiled
    );
    assert!(
        compiled.contains("19.99") && compiled.contains("f64"),
        "TIR lost the explicit machine Float path:\n{}",
        compiled
    );
}

#[test]
fn exact_numbers_aot_matches_the_known_result() {
    if !tir_support::have_rustc() {
        return;
    }
    let (code, stdout, stderr) =
        tir_support::build_and_run_full("exact_number_tier_aot", "exact_number_tier_aot", SOURCE);
    assert_eq!(code, 0, "AOT failed: {stderr}");
    assert_eq!(stdout, EXPECTED);
}

#[test]
fn exact_numbers_default_jit_matches_the_known_result() {
    let (code, stdout, stderr) = tir_support::jit_run("exact_number_tier_jit", SOURCE);
    assert_eq!(code, 0, "default JIT failed: {stderr}");
    assert_eq!(stdout, EXPECTED);
}

#[test]
fn exact_numbers_interpreter_matches_the_known_result() {
    let (code, stdout, stderr) =
        tir_support::interpreter_run("exact_number_tier_interpreter", SOURCE);
    assert_eq!(code, 0, "interpreter failed: {stderr}");
    assert_eq!(stdout, EXPECTED);
}

#[test]
fn exact_numbers_comptime_matches_the_known_result() {
    if !tir_support::have_rustc() {
        return;
    }
    let source = r#"
@third :: 1 / 3
@roundtrip :: @third * 3
@decimal :: 0.1 + 0.2

fn run() {
    print(@third)
    print(@roundtrip == 1)
    print(@decimal == 0.3)
    print(Float{19.99})
}
"#;
    let (code, stdout, stderr) = tir_support::build_and_run_full(
        "exact_number_tier_comptime",
        "exact_number_tier_comptime",
        source,
    );
    assert_eq!(code, 0, "comptime-backed AOT failed: {stderr}");
    assert_eq!(stdout, EXPECTED);
}

#[test]
fn exact_numbers_repl_matches_the_known_result() {
    let transcript = jet::REPL::run_transcript(
        &[
            "1 / 3",
            "(1 / 3) * 3 == 1",
            "0.1 + 0.2 == 0.3",
            "Float{19.99}",
        ],
        None,
    );
    assert!(
        !transcript.contains("error ["),
        "REPL rejected exact numbers: {transcript}"
    );
    assert!(
        transcript.contains("1/3"),
        "REPL lost the exact quotient: {transcript}"
    );
    assert!(
        transcript.contains("19.99"),
        "REPL lost the Float opt-in: {transcript}"
    );
    assert!(
        transcript.matches("true").count() >= 2,
        "REPL disagreed with the exact equality results: {transcript}"
    );
}

#[test]
fn exact_numbers_web_matches_the_known_result() {
    let output =
        jet::compile_web_with_path(&web_source(), "tests/fixtures/exact_number_tiers_web.jet")
            .unwrap_or_else(|diagnostics| panic!("web exact numbers rejected: {diagnostics:#?}"));
    let web = output.web.expect("web target must produce artifacts");

    if !have_tool("rustc") || !have_tool("node") || !have_wasm_target() {
        eprintln!(
            "note: skipping exact-number web execution (need rustc, wasm32 target, and node)"
        );
        return;
    }

    let scratch = common::Scratch::new("exact-number-web");
    fs::write(scratch.join("app.js"), &web.js_app).unwrap();
    fs::write(scratch.join("jet_dom_runtime.js"), &web.dom_runtime).unwrap();
    fs::write(scratch.join("app_wasm.rs"), &web.wasm_rust).unwrap();

    let wasm = Command::new("rustc")
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
        .expect("spawn web rustc");
    assert!(
        wasm.status.success(),
        "rustc rejected exact-number web output: {}",
        String::from_utf8_lossy(&wasm.stderr)
    );

    let js = Command::new("node")
        .current_dir(&scratch.path)
        .arg("app.js")
        .output()
        .expect("spawn exact-number web JS app");
    assert!(
        js.status.success(),
        "node rejected exact-number web output: stdout={} stderr={}",
        String::from_utf8_lossy(&js.stdout),
        String::from_utf8_lossy(&js.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&js.stdout), EXPECTED);

    fs::write(
        scratch.join("wasm_harness.mjs"),
        r#"
const { instantiateWasm, takeWasmError } = await import("./jet_dom_runtime.js");
const instance = await instantiateWasm("./app.wasm");
instance.exports.jet_export_run();
if (takeWasmError(instance.exports)?.tag !== "Ok") throw new Error("unexpected Wasm outcome");
"#,
    )
    .unwrap();
    let wasm_run = Command::new("node")
        .current_dir(&scratch.path)
        .arg("wasm_harness.mjs")
        .output()
        .expect("spawn exact-number Wasm harness");
    assert!(
        wasm_run.status.success(),
        "node rejected exact-number Wasm output: stdout={} stderr={}",
        String::from_utf8_lossy(&wasm_run.stdout),
        String::from_utf8_lossy(&wasm_run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&wasm_run.stdout), EXPECTED);
}
