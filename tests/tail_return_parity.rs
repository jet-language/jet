//! D-TAIL-RETURN1=A / I9: block values, value arm tables, and early returns
//! keep one meaning through hosted tiers, comptime, and the web backend.

mod common;

use std::fs;
use std::process::Command;

const PACKAGE_SOURCE: &str = r#"
name: "tail_return_parity"
version: "0.1.0"
authority: { holds: { allow: [IO] } }
"#;

const COMPTIME_SOURCE: &str = r#"
fn label(value: Int) -> String Never! {
    if value == {
        1 -> { "one" }
        else -> { "other" }
    }
}

fn early(flag: Bool) -> String Never! {
    if flag { return "early" }
    "late"
}

enum Packet {
    Data(Int)
    Retry(Int)
    Empty
    Ignore(Int)
}

fn packet_value(packet: Packet) -> Int Never! {
    if packet == {
        .Data(10..19) -> { 100 }
        .Data(value) | .Retry(value) -> {
            if value == 0 -> return 9
            if value > 0 -> { value + 1 } else -> { -1 }
        }
        .Ignore(_) -> { 0 }
        .Empty -> { 0 }
        else -> { -2 }
    }
}

EXPECTED_ONE :: prep { label(1) }
EXPECTED_OTHER :: prep { label(2) }
EXPECTED_EARLY :: prep { early(true) }
EXPECTED_LATE :: prep { early(false) }
EXPECTED_RANGE :: prep { packet_value(Packet.Data(12)) }
EXPECTED_OR :: prep { packet_value(Packet.Retry(2)) }
EXPECTED_NESTED_RETURN :: prep { packet_value(Packet.Data(0)) }
EXPECTED_EMPTY :: prep { packet_value(Packet.Empty) }
EXPECTED_WILDCARD :: prep { packet_value(Packet.Ignore(17)) }

fn run() {
    print(EXPECTED_ONE)
    print(EXPECTED_OTHER)
    print(EXPECTED_EARLY)
    print(EXPECTED_LATE)
    print(label(1))
    print(label(2))
    print(early(true))
    print(early(false))
    print(EXPECTED_RANGE)
    print(EXPECTED_OR)
    print(EXPECTED_NESTED_RETURN)
    print(EXPECTED_EMPTY)
    print(EXPECTED_WILDCARD)
    print(packet_value(Packet.Data(12)))
    print(packet_value(Packet.Retry(2)))
    print(packet_value(Packet.Data(0)))
    print(packet_value(Packet.Empty))
    print(packet_value(Packet.Ignore(17)))
}
"#;

const WEB_SOURCE: &str = r#"#Target(Web)

#Target(JS)
fn js_block(flag: Bool) -[]> Int {
    if flag -> {
        value :: 6
        value + 1
    } else -> { 3 }
}

#Target(JS)
fn js_arm(value: Int) -[]> Int {
    if value == {
        1 -> { 10 }
        else -> { 20 }
    }
}

#Target(JS)
fn js_early(flag: Bool) -[]> Int {
    if flag { return 30 }
    40
}

enum Packet {
    Data(Int)
    Retry(Int)
    Empty
    Ignore(Int)
}

#Target(JS)
fn packet_value(packet: Packet) -[]> Int {
    if packet == {
        .Data(10..19) -> { 100 }
        .Data(value) | .Retry(value) -> {
            if value == 0 -> return 9
            if value > 0 -> { value + 1 } else -> { -1 }
        }
        .Ignore(_) -> { 0 }
        .Empty -> { 0 }
        else -> { -2 }
    }
}

#WasmExport
fn wasm_block(flag: Bool) -[]> Int {
    if flag -> { 7 } else -> { 3 }
}

#WasmExport
fn wasm_arm(value: Int) -[]> Int {
    if value == {
        1 -> { 10 }
        else -> { 20 }
    }
}

#WasmExport
fn wasm_early(flag: Bool) -[]> Int {
    if flag { return 30 }
    40
}

#Target(JS)
fn run() {
    print(js_block(true))
    print(js_arm(2))
    print(js_early(true))
    print(js_early(false))
    print(packet_value(Packet.Data(12)))
    print(packet_value(Packet.Retry(2)))
    print(packet_value(Packet.Data(0)))
    print(packet_value(Packet.Ignore(17)))
    print(packet_value(Packet.Empty))
    print(wasm_block(true))
    print(wasm_arm(2))
    print(wasm_early(true))
    print(wasm_early(false))
}
"#;

#[test]
fn block_values_arm_tables_and_early_returns_match_comptime_and_hosted_tiers() {
    assert_packaged_cli_tiers_agree(
        COMPTIME_SOURCE,
        "one\nother\nearly\nlate\none\nother\nearly\nlate\n100\n3\n9\n0\n0\n100\n3\n9\n0\n0\n",
    );
}

/// L0507's automatic fix respells a classic chain as one arm table. When the
/// chain is a function's last statement, the table becomes the function's
/// final expression (D-BODY-LAST1=B), for both a declared result and a unit
/// function. The fix is graded safe because the meaning is the same on every
/// hosted tier. The fixed text comes from the compiler's own edits.
#[test]
fn l0507_arm_table_fix_keeps_a_tail_chain_meaning_on_every_tier() {
    let source = r#"fn grade(score: Int) -> String {
    if score >= 90 {
        return "a"
    } else if score >= 80 {
        return "b"
    } else {
        return "c"
    }
}

fn show(score: Int) {
    if score >= 90 {
        print("A")
    } else if score >= 80 {
        print("B")
    } else {
        print("C")
    }
}

fn run() {
    print(grade(95))
    print(grade(85))
    print(grade(10))
    show(95)
    show(85)
    show(10)
}
"#;
    // Compile from a real file: the driver resolves the entry path on disk.
    let scratch = common::unique_tmp("jet_l0507_fix");
    fs::create_dir_all(&scratch).unwrap();
    let compile = |text: &str| {
        let path = scratch.join("run.jet");
        fs::write(&path, text).unwrap();
        jet::compile_with_path(text, path.to_str().unwrap())
    };
    let compiled = compile(source)
        .unwrap_or_else(|diagnostics| panic!("classic chains must compile: {diagnostics:?}"));
    let edits: Vec<_> = compiled
        .lints
        .iter()
        .filter(|diagnostic| diagnostic.code == "L0507")
        .filter_map(|diagnostic| diagnostic.edit.clone())
        .collect();
    assert_eq!(edits.len(), 2, "both chains must carry the arm-table fix");
    let fixed = jet::FixEngine::apply_edits(source, &edits).expect("the L0507 edits must apply");
    assert!(fixed.contains("    if {\n        score >= 90 -> {"), "{fixed}");
    let recompiled = compile(&fixed).unwrap_or_else(|diagnostics| {
        panic!("fixed source must compile:\n{fixed}\n{diagnostics:?}")
    });
    assert!(
        !recompiled.lints.iter().any(|d| d.code == "L0507"),
        "fixed source still warns:\n{fixed}"
    );
    let _ = fs::remove_dir_all(&scratch);
    let expected = "a\nb\nc\nA\nB\nC\n";
    assert_packaged_cli_tiers_agree(source, expected);
    assert_packaged_cli_tiers_agree(&fixed, expected);
}

#[test]
fn block_values_arm_tables_and_early_returns_match_web_runtime() {
    if !have_tool("rustc") || !have_tool("node") || !have_wasm_target() {
        eprintln!("note: skipping tail-return web test (need rustc, wasm32 target, and node)");
        return;
    }

    let out = jet::compile_web_with_path(WEB_SOURCE, "tests/fixtures/tail_return_web.jet")
        .expect("tail-return web source must compile");
    let web = out.web.expect("web compile must return web artifacts");

    let scratch = common::Scratch::new("jet_tail_return_web");
    fs::write(scratch.join("app.js"), &web.js_app).unwrap();
    fs::write(scratch.join("jet_dom_runtime.js"), &web.dom_runtime).unwrap();
    fs::write(scratch.join("app_wasm.rs"), &web.wasm_rust).unwrap();
    fs::write(scratch.join("package.json"), r#"{"type":"module"}"#).unwrap();
    fs::write(
        scratch.join("harness.mjs"),
        r#"const { jet_main } = await import("./app.js");
await jet_main();
"#,
    )
    .unwrap();
    let wasm = Command::new("rustc")
        .current_dir(&scratch.path)
        .args([
            "--edition", "2021", "--target", "wasm32-unknown-unknown",
            "--crate-type", "cdylib", "-O", "app_wasm.rs", "-o", "app.wasm",
        ])
        .output()
        .expect("rustc must build the generated Wasm partition");
    assert!(
        wasm.status.success(),
        "generated Wasm failed to compile:\n{}",
        String::from_utf8_lossy(&wasm.stderr)
    );
    let output = Command::new("node")
        .current_dir(&scratch.path)
        .arg("harness.mjs")
        .output()
        .expect("node must run the generated web harness");
    assert!(
        output.status.success(),
        "generated web app failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "7\n20\n30\n40\n100\n3\n9\n0\n0\n7\n20\n30\n40\n"
    );
}

fn have_tool(name: &str) -> bool {
    Command::new(name).arg("--version").output().is_ok()
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

fn assert_packaged_cli_tiers_agree(src: &str, expected_stdout: &str) {
    let root = common::unique_tmp("jet_tail_return_parity");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("package.jet"), PACKAGE_SOURCE).unwrap();
    fs::write(root.join("run.jet"), src).unwrap();

    let modes = [
        ("release", true, false),
        ("default", false, false),
        ("interpret", false, true),
    ];
    let mut baseline = None;
    for (mode, release, interpret) in modes {
        let cache = root.join(format!("cache-{mode}"));
        let mut command = Command::new(env!("CARGO_BIN_EXE_jet"));
        command.arg("run");
        if release {
            command.arg("--release");
        }
        if interpret {
            command.arg("--interpret");
        }
        let output = command
            .arg("run.jet")
            .current_dir(&root)
            .env("JET_STORE_DIR", &cache)
            .env("JETPACK_ENV", "1")
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        let result = (
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        );
        assert_eq!(result.0, 0, "{mode} run failed:\n{}", result.2);
        assert_eq!(result.1, expected_stdout, "{mode} output");
        if let Some((baseline_mode, baseline_code, baseline_stdout)) = &baseline {
            assert_eq!(result.0, *baseline_code, "{mode} exit code disagreed with {baseline_mode}");
            assert_eq!(result.1, *baseline_stdout, "{mode} output disagreed with {baseline_mode}");
        } else {
            baseline = Some((mode, result.0, result.1));
        }
    }
    let _ = fs::remove_dir_all(root);
}
