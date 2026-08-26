//! D-TAIL-RETURN1=A / I9: block values, value arm tables, and early returns
//! keep one meaning through hosted tiers, comptime, and the web backend.

mod common;

use std::fs;
use std::process::Command;

const PACKAGE_SOURCE: &str = r#"
name: "tail_return_parity"
version: "0.1.0"
authority: .{ holds: { allow: [IO] } }
"#;

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

@expected_one :: label(1)
@expected_other :: label(2)
@expected_early :: early(true)
@expected_late :: early(false)

fn run() {
    print(@expected_one)
    print(@expected_other)
    print(@expected_early)
    print(@expected_late)
    print(label(1))
    print(label(2))
    print(early(true))
    print(early(false))
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
    assert_packaged_cli_tiers_agree(
        COMPTIME_SOURCE,
        "one\nother\nearly\nlate\none\nother\nearly\nlate\n",
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
            .env("JET_CACHE_DIR", &cache)
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
