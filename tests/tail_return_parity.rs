//! D-TAIL-RETURN1=A / I9: block values, value arm tables, and early returns
//! keep one meaning through hosted tiers, comptime, and the web backend.

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use std::fs;
use std::process::Command;

const COMPTIME_SOURCE: &str = r#"
fn label(value: Int) String -> {
    if value == {
        1 -> { "one" }
        else -> { "other" }
    }
}

fn early(flag: Bool) String -> {
    if flag { return "early" }
    "late"
}

@expected :: "{label(1)}|{label(2)}|{early(true)}|{early(false)}"

fn run() {
    actual :: "{label(1)}|{label(2)}|{early(true)}|{early(false)}"
    print("{@expected}")
    print("{actual}")
}
"#;

const WEB_SOURCE: &str = r#"#Target(Web)

#Target(JS)
fn js_block(flag: Bool) Int -[]> {
    if flag -> {
        value :: 6
        value + 1
    } else -> { 3 }
}

#Target(JS)
fn js_arm(value: Int) Int -[]> {
    if value == {
        1 -> { 10 }
        else -> { 20 }
    }
}

#Target(JS)
fn js_early(flag: Bool) Int -[]> {
    if flag { return 30 }
    40
}

#WasmExport
fn wasm_block(flag: Bool) Int -[]> {
    if flag -> { 7 } else -> { 3 }
}

#WasmExport
fn wasm_arm(value: Int) Int -[]> {
    if value == {
        1 -> { 10 }
        else -> { 20 }
    }
}

#WasmExport
fn wasm_early(flag: Bool) Int -[]> {
    if flag { return 30 }
    40
}

#Target(JS)
fn run() {
    print(js_block(true))
    print(js_arm(2))
    print(js_early(true))
    print(js_early(false))
}
"#;

#[test]
fn block_values_arm_tables_and_early_returns_match_comptime_and_hosted_tiers() {
    tir_support::assert_tiers_agree(
        "tail_return_comptime_parity",
        COMPTIME_SOURCE,
        "one|other|early|late\none|other|early|late\n",
    );
}

#[test]
fn block_values_arm_tables_and_early_returns_match_web_runtime() {
    if !have_tool("node") {
        eprintln!("note: skipping tail-return web test (need node)");
        return;
    }

    let out = jet::compile_web_with_path(WEB_SOURCE, "tests/fixtures/tail_return_web.jet")
        .expect("tail-return web source must compile");
    let web = out.web.expect("web compile must return web artifacts");
    assert!(web.wasm_rust.contains("wasm_block"));
    assert!(web.wasm_rust.contains("wasm_arm"));
    assert!(web.wasm_rust.contains("wasm_early"));

    let dir = common::unique_tmp("jet_tail_return_web");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("app.js"), &web.js_app).unwrap();
    fs::write(dir.join("jet_dom_runtime.js"), &web.dom_runtime).unwrap();
    let output = Command::new("node")
        .current_dir(&dir)
        .arg("app.js")
        .output()
        .expect("node must run the generated web app");
    let _ = fs::remove_dir_all(&dir);
    assert!(
        output.status.success(),
        "generated web app failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "7\n20\n30\n40\n");
}

fn have_tool(name: &str) -> bool {
    Command::new(name).arg("--version").output().is_ok()
}
