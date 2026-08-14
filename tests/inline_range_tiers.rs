//! D-TYPE2-SPELL1 / card #1549: inline value ranges keep one meaning across
//! the comptime, REPL, and web entry points that do not use the gate ledger.

use std::fs;
use std::process::Command;

#[path = "tir_support/mod.rs"]
mod tir_support;
mod common;

const EXAMPLE: &str = include_str!("../examples/features/types/range_types.jet");
const EXPECTED: &str = include_str!("../examples/features/expected/types/range_types.out");

fn have_tool(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn have_wasm_target() -> bool {
    Command::new("rustc")
        .args(["--print", "target-libdir", "--target", "wasm32-unknown-unknown"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[test]
fn inline_range_comptime_and_repl_accept_legal_inputs() {
    let compiled = jet::compile_with_path(EXAMPLE, "examples/features/types/range_types.jet")
        .expect("inline range example must compile through the shared front end");
    assert!(
        compiled.rust.contains("42"),
        "comptime inline-range binding was not carried into generated output"
    );

    // The REPL accepts one item or statement per input. Keep `#Numeric` in the
    // file-wide comptime check above; it is not legal at a statement site.
    let transcript = jet::REPL::run_transcript(
        &[
            "fn set_brightness(level: Int(0..100)) => Int(0..100) :: level",
            "fn checked_inline(raw: Int) => Int(0..100) ? String :: Int(0..100).from_int(raw)",
            "print(set_brightness(42))",
            "print(checked_inline(3) ?? Int(0..100).from_int(0))",
        ],
        None,
    );
    assert_eq!(transcript, "ok\nok\n42\n3\n");
}

#[test]
fn inline_range_example_matches_aot_default_run_and_interpreter() {
    tir_support::assert_example_cli_tiers_agree("types/range_types", EXPECTED);
}

#[test]
fn inline_range_web_runtime_matches_the_shared_value() {
    let source = r#"#Target(Web)
fn set_brightness(level: Int(0..100)) => Int(0..100) :: level

fn run() {
    print(set_brightness(42))
    print(Int(0..100).from_int(3))
}
"#;
    let shown = "tests/fixtures/inline_range_web.jet";
    let output = jet::compile_web_with_path(source, shown)
        .unwrap_or_else(|diags| panic!("web inline-range source was rejected: {diags:#?}"));
    let web = output.web.expect("web target must produce artifacts");
    assert!(
        web.js_app.contains("function jet_inline_range_from_int"),
        "web JS must embed the shared inline-range Prelude kernel"
    );
    assert!(
        web.wasm_rust.contains("jet_inline_range_from_int"),
        "web Wasm must call the shared inline-range Prelude kernel"
    );

    if !have_tool("rustc") || !have_tool("node") || !have_wasm_target() {
        eprintln!("note: skipping inline-range web execution (need rustc, wasm32 target, and node)");
        return;
    }

    let scratch = common::Scratch::new("inline-range-web");
    fs::write(scratch.join("app.js"), &web.js_app).unwrap();
    fs::write(scratch.join("jet_dom_runtime.js"), &web.dom_runtime).unwrap();
    fs::write(scratch.join("app_wasm.rs"), &web.wasm_rust).unwrap();
    fs::write(scratch.join("package.json"), r#"{"type":"module"}"#).unwrap();

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
        "rustc rejected inline-range web output: {}",
        String::from_utf8_lossy(&wasm.stderr)
    );

    let node = Command::new("node")
        .current_dir(&scratch.path)
        .arg("app.js")
        .output()
        .expect("spawn inline-range web app");
    assert!(
        node.status.success(),
        "node rejected inline-range web output: stdout={} stderr={}",
        String::from_utf8_lossy(&node.stdout),
        String::from_utf8_lossy(&node.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&node.stdout), "42\n3\n");
}
