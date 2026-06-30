//! D-WEBBACKEND1 M2 (c123): `--target=web` WASM + JS artifact golden runs.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn have_tool(name: &str) -> bool {
    Command::new(name).arg("--version").output().is_ok()
}

fn build_web_fixture(stem: &str, src: &str, shown: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("jet_web_{stem}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::create_dir_all(dir.join("build")).unwrap();

    let out = jet::compile_web_with_path(src, shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected web fixture:\n{}",
            jet::render_diagnostics(shown, src, &diags)
        )
    });
    let web = out.web.expect("web target compile must produce web artifacts");
    assert!(
        web.manifest_json.contains("\"status\": \"m2\""),
        "manifest should be M2: {}",
        web.manifest_json
    );
    assert!(
        web.manifest_json.contains("\"partitions\""),
        "manifest missing partitions"
    );
    fs::write(dir.join("build/web.manifest.json"), &web.manifest_json).unwrap();
    fs::write(dir.join("build/jet_dom_runtime.js"), &web.dom_runtime).unwrap();
    fs::write(dir.join("build/app.js"), &web.js_app).unwrap();
    fs::write(dir.join("build/app_wasm.rs"), &web.wasm_rust).unwrap();

    let wasm_path = dir.join("build/app.wasm");
    let rustc = Command::new("rustc")
        .current_dir(&dir)
        .args([
            "--edition",
            "2021",
            "--target",
            "wasm32-unknown-unknown",
            "--crate-type",
            "cdylib",
            "-O",
            "build/app_wasm.rs",
            "-o",
            "build/app.wasm",
        ])
        .output()
        .unwrap();
    assert!(
        rustc.status.success(),
        "rustc rejected wasm for {stem}:\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    assert!(wasm_path.is_file(), "missing app.wasm for {stem}");
    dir
}

fn run_web_app(dir: &PathBuf) -> String {
    let node = Command::new("node")
        .current_dir(dir.join("build"))
        .arg("app.js")
        .output()
        .unwrap();
    assert!(
        node.status.success(),
        "node run failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&node.stdout),
        String::from_utf8_lossy(&node.stderr)
    );
    String::from_utf8_lossy(&node.stdout).into_owned()
}

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

#[test]
fn jet_cli_explain_partition_requires_web_target() {
    let jet = jet_bin();
    let out = Command::new(&jet)
        .args([
            "build",
            "--explain-partition",
            "examples/features/01_hello.jet",
        ])
        .output()
        .unwrap();
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !combined.contains("isn't a flag"),
        "CLI should accept --explain-partition, got:\n{combined}"
    );
    assert!(
        combined.contains("`--explain-partition` requires `--target=web`"),
        "expected web-target guard, got:\n{combined}"
    );
}

#[test]
fn jet_cli_web_build_succeeds() {
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping jet CLI web build test");
        return;
    }
    let jet = jet_bin();
    let out = Command::new(&jet)
        .args([
            "build",
            "--target=web",
            "examples/features/164_web_compute.jet",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "jet CLI web build failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn compile_web_file_loads() {
    let out = jet::compile_web("examples/features/164_web_compute.jet").expect("compile_web");
    assert!(out.web.is_some());
}

#[test]
fn web_hello_dom_shim_roundtrip() {
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping web_build hello (need rustc + node)");
        return;
    }
    let src = include_str!("../examples/features/163_web_hello.jet");
    let dir = build_web_fixture("hello", src, "examples/features/163_web_hello.jet");
    let stdout = run_web_app(&dir);
    let expected = include_str!("../examples/features/expected/163_web_hello.web.out");
    assert_eq!(stdout, expected);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn web_compute_wasm_bridge_roundtrip() {
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping web_build compute (need rustc + node)");
        return;
    }
    let src = include_str!("../examples/features/164_web_compute.jet");
    let dir = build_web_fixture("compute", src, "examples/features/164_web_compute.jet");
    let stdout = run_web_app(&dir);
    let expected = include_str!("../examples/features/expected/164_web_compute.out");
    assert_eq!(stdout, expected);
    let _ = fs::remove_dir_all(&dir);
}
