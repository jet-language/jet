//! Card #1555 c8: the measured-value knowledge grade has one behavior on each
//! front-end, execution, and web surface.

use std::fs;
use std::process::Command;

mod common;
#[path = "tir_support/mod.rs"]
mod tir_support;

const SOURCE: &str = r#"
fn run() {
    left :: measurement(12.0, uncertainty: 0.1)
    right :: measurement(3.0, uncertainty: 0.2)
    print(left + right)
}
"#;

const WEB_SOURCE: &str = "#Target(Web)\n";
const EXPECTED: &str = "15.0 ± 0.223606797749979\n";

fn web_source() -> String {
    format!("{WEB_SOURCE}{SOURCE}")
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
fn measurement_parser_accepts_the_canonical_constructor() {
    let (tokens, diagnostics) = jet::Lexer::lex(SOURCE);
    assert!(
        diagnostics.is_empty(),
        "lexer rejected measurement: {diagnostics:?}"
    );
    assert!(
        jet::Parser::parse(&tokens).is_ok(),
        "parser rejected the canonical measurement constructor"
    );
}

#[test]
fn measurement_sema_accepts_the_knowledge_grade() {
    let result = jet::compile(SOURCE);
    assert!(result.is_ok(), "sema rejected measured values: {result:#?}");
}

#[test]
fn measurement_tir_keeps_the_carrier_and_operation() {
    let compiled = jet::compile(SOURCE).expect("measurement source must reach TIR");
    assert!(compiled.rust.contains("JetMeasurement"));
    assert!(
        compiled.rust.contains("jet_std::JetMeasurement::new") || compiled.rust.contains(".add("),
        "TIR lost the measured carrier operation:\n{}",
        compiled.rust
    );
}

#[test]
fn measurement_aot_matches_the_known_result() {
    if !tir_support::have_rustc() {
        return;
    }
    let (code, stdout, stderr) =
        tir_support::build_and_run_full("measurement_tier_aot", "measurement_tier_aot", SOURCE);
    assert_eq!(code, 0, "AOT failed: {stderr}");
    assert_eq!(stdout, EXPECTED);
}

#[test]
fn measurement_default_jit_matches_the_known_result() {
    let (code, stdout, stderr) = tir_support::jit_run("measurement_tier_jit", SOURCE);
    assert_eq!(code, 0, "default JIT failed: {stderr}");
    assert_eq!(stdout, EXPECTED);
}

#[test]
fn measurement_interpreter_matches_the_known_result() {
    let (code, stdout, stderr) =
        tir_support::interpreter_run("measurement_tier_interpreter", SOURCE);
    assert_eq!(code, 0, "interpreter failed: {stderr}");
    assert_eq!(stdout, EXPECTED);
}

#[test]
fn measurement_comptime_matches_the_known_result() {
    if !tir_support::have_rustc() {
        return;
    }
    let source = r#"
@folded :: measurement(12.0, uncertainty: 0.1) + measurement(3.0, uncertainty: 0.2)

fn run() {
    print(@folded)
}
"#;
    let (code, stdout, stderr) = tir_support::build_and_run_full(
        "measurement_tier_comptime",
        "measurement_tier_comptime",
        source,
    );
    assert_eq!(code, 0, "comptime-backed AOT failed: {stderr}");
    assert_eq!(stdout, EXPECTED);
}

#[test]
fn measurement_repl_matches_the_known_result() {
    let transcript = jet::REPL::run_transcript(
        &[
            "measurement(12.0, uncertainty: 0.1)",
            "measurement(12.0, uncertainty: 0.1) + measurement(3.0, uncertainty: 0.2)",
        ],
        None,
    );
    assert!(
        !transcript.contains("error ["),
        "REPL rejected measurement: {transcript}"
    );
    assert!(
        transcript.contains(EXPECTED.trim_end()),
        "REPL disagreed with the execution tiers: {transcript}"
    );
}

#[test]
fn measurement_web_js_and_wasm_use_the_same_kernel() {
    let output =
        jet::compile_web_with_path(&web_source(), "tests/fixtures/measurement_tiers_web.jet")
            .unwrap_or_else(|diagnostics| panic!("web measurement rejected: {diagnostics:#?}"));
    let web = output.web.expect("web target must produce artifacts");
    assert!(web.js_app.contains("jet_measurement_kernel_add"));
    assert!(web.js_app.contains("jet_measurement_show"));
    assert!(web.wasm_rust.contains("jet_measurement_kernel_add"));
    assert!(web.wasm_rust.contains("JetMeasurement"));

    if !have_tool("rustc") || !have_tool("node") || !have_wasm_target() {
        eprintln!(
            "note: skipping measured-value web execution (need rustc, wasm32 target, and node)"
        );
        return;
    }

    let scratch = common::Scratch::new("measurement-web");
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
        "rustc rejected measured-value web output: {}",
        String::from_utf8_lossy(&wasm.stderr)
    );

    let js = Command::new("node")
        .current_dir(&scratch.path)
        .arg("app.js")
        .output()
        .expect("spawn web JS app");
    assert!(
        js.status.success(),
        "node rejected measured-value web output: stdout={} stderr={}",
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
        .expect("spawn web Wasm harness");
    assert!(
        wasm_run.status.success(),
        "node rejected measured-value Wasm output: stdout={} stderr={}",
        String::from_utf8_lossy(&wasm_run.stdout),
        String::from_utf8_lossy(&wasm_run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&wasm_run.stdout), EXPECTED);
}
