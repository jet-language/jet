//! D-FAILURE-FOUNDATION1=A / I9: a declared error conversion keeps one carrier
//! and one report meaning across the resident, interpreter, and AOT edges.

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use std::fs;
use std::process::Command;

const MATRIX: &str = r#"
#Error
enum TypedFailure {
    Bad
}

#Error
enum StoreFailure {
    Missing
}

fn implicit(value: Int) -> Int {
    if value == 0 {
        return Err("implicit")
    }
    return value
}

fn explicit(value: Int) -> Int TypedFailure! {
    if value == 0 {
        return Err(TypedFailure.Bad)
    }
    return value
}

impl StoreFailure -> Err {
    return Err("converted")
}

fn converted(value: Int) -> Int StoreFailure! {
    if value == 0 {
        return Err(StoreFailure.Missing)
    }
    return value
}

fn contextual_source() -> Int { Err("context", code: "E_CONTEXT", cause: Err("root")) }

fn contextual() -> Int { contextual_source()?("loading") }

fn optional_success(value: Int) -> Int? {
    if value == 0 {
        return None
    }
    return Val(value)
}

fn unit_success(fail: Bool) {
    if fail {
        return Err("unit")
    }
}

fn unit_caller(fail: Bool) -> Int {
    unit_success(fail)
    return 7
}

fn impossible() -> Int Never! { 7 }

fn matrix() -> String {
    return "{implicit(2) ?? -1}|{implicit(0) ?? -1}|{explicit(2) ?? -2}|{explicit(0) ?? -2}|{converted(2) ?? -3}|{converted(0) ?? -3}|{contextual() ?? -4}|{optional_success(2) ?? -5}|{optional_success(0) ?? -5}|{unit_caller(false) ?? -6}|{unit_caller(true) ?? -6}|{impossible() ?? -7}"
}

COMPTIME_MATRIX :: comptime { matrix() }

fn run() {
    print(COMPTIME_MATRIX)
    print(matrix())
}
"#;

const MATRIX_STDOUT: &str = "2|-1|2|-2|2|-3|-4|2|-5|7|-6|7\n2|-1|2|-2|2|-3|-4|2|-5|7|-6|7\n";

const CONVERSION: &str = r#"
#Error
enum StoreFailure {
    Missing
}

impl StoreFailure -> Err {
    return Err("converted")
}

fn read() -> Int StoreFailure! { Err(StoreFailure.Missing) }

fn run() {
    read()
}
"#;

const RNG_INT_RANGE: &str = include_str!("fixtures/runtime_e1003.jet");

#[test]
fn runtime_e1003_is_a_stop_with_exit_70_on_every_hosted_tier() {
    let files = [("main.jet", RNG_INT_RANGE)];
    let runs = [
        ("default", tir_support::default_run("runtime_e1003", RNG_INT_RANGE)),
        ("interpreter", tir_support::interpreter_run("runtime_e1003", RNG_INT_RANGE)),
        ("debug AOT", tir_support::build_and_run_full("runtime_e1003", "main", RNG_INT_RANGE)),
        ("release AOT", tir_support::build_release_and_run_multi("runtime_e1003", "main.jet", &files)),
    ];
    for (tier, (code, stdout, stderr)) in runs {
        assert_eq!(code, 70, "{tier}: {stderr}");
        assert!(stdout.is_empty(), "{tier}: {stdout}");
        // Engine locations are adapter facts. The report and exit law are not.
        let report = stderr.lines()
            .filter(|line| !line.starts_with("  --> "))
            .collect::<Vec<_>>().join("\n") + "\n";
        assert_eq!(report, include_str!("fixtures/runtime_e1003.stderr"), "{tier}");
        assert_eq!(stderr.matches("Stop [E1003]").count(), 1, "{tier}: {stderr}");
    }
}

#[test]
fn rng_draw_preserves_owned_exact_int_results_on_every_hosted_tier() {
    // Both one-value intervals fit i64 but lie outside the inline Int range.
    // The provider returns an already-owned tagged Int, never a raw i64 result.
    let source = include_str!("fixtures/rng_owned_int_result.jet");
    let files = [("main.jet", source)];
    let runs = [
        ("default", tir_support::default_run("rng_owned_int_result", source)),
        ("interpreter", tir_support::interpreter_run("rng_owned_int_result", source)),
        ("debug AOT", tir_support::build_and_run_full("rng_owned_int_result", "main", source)),
        ("release AOT", tir_support::build_release_and_run_multi("rng_owned_int_result", "main.jet", &files)),
    ];
    for (tier, (code, stdout, stderr)) in runs {
        assert_eq!(code, 0, "{tier}: {stderr}");
        assert_eq!(stdout, "4611686018427387904\n-4611686018427387905\n", "{tier}");
        assert!(stderr.is_empty(), "{tier}: {stderr}");
    }
}

#[test]
fn runtime_e1003_report_matches_the_registered_snapshot() {
    let report = jet_foundation::Outcome::jet_render_runtime_stop(
        "E1003", "", 0, "", "", 1, 1,
        "A default Int value does not fit in the C i64 range", "",
    );
    assert_eq!(report.exit_code, 70);
    assert_eq!(report.source, "runtime");
    assert_eq!(report.rendered, include_str!("fixtures/runtime_e1003.stderr"));
}

#[test]
fn runtime_e1003_web_uses_the_same_stop_metadata_and_exit_70() {
    let scratch = common::Scratch::new("runtime_e1003_web");
    tir_support::write_test_package(&scratch.path, tir_support::TIR_TEST_PACKAGE);
    let source = "#Target(JS)\nfn run() {}\n";
    let path = scratch.path.join("main.jet");
    fs::write(&path, source).unwrap();
    let output = jet::compile_web_with_path(source, path.to_str().unwrap())
        .expect("web E1003 projection fixture must compile");
    let web = output.web.expect("web artifacts");
    fs::write(scratch.path.join("package.json"), "{\"type\":\"module\"}\n").unwrap();
    fs::write(scratch.path.join("app.js"), format!("{}\nexport {{ jet_runtime_stop, JET_RUNTIME_STOP_METADATA }};\n", web.js_app)).unwrap();
    fs::write(scratch.path.join("jet_dom_runtime.js"), web.dom_runtime).unwrap();
    fs::write(scratch.path.join("run.mjs"), r#"
const { jet_runtime_stop, JET_RUNTIME_STOP_METADATA } = await import("./app.js");
if (JET_RUNTIME_STOP_METADATA.E1003.source !== "runtime" || JET_RUNTIME_STOP_METADATA.E1003.exit_code !== 70) throw new Error("E1003 runtime exit law lost");
try {
    jet_runtime_stop("E1003", "", 0, "A default Int value does not fit in the C i64 range", "", "", 1, 1, "");
    throw new Error("runtime stop returned");
} catch (error) {
    if (error.name !== "JetError" || error.code !== "E1003") throw error;
    process.stdout.write(error.frame);
}
"#).unwrap();
    let run = Command::new("node").current_dir(&scratch.path).arg("run.mjs")
        .output().expect("web Stop proof requires Node");
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    assert!(run.stderr.is_empty());
    let report = String::from_utf8_lossy(&run.stdout).lines()
        .filter(|line| !line.is_empty()).collect::<Vec<_>>().join("\n") + "\n";
    assert_eq!(report, include_str!("fixtures/runtime_e1003.stderr"));
}

const NET_ERROR_CONVERSION: &str = r#"
use core.net as net

fn run() {
    result :: net.dns_srv("_jet._tcp.example.test", -1)
    print(result.len())
}
"#;

const RECOVERED_CONTEXT: &str = r#"
fn contextual_source() -> Int { Err("context", cause: Err("root")) }

fn contextual() -> Int { contextual_source()?("loading") }

fn later() -> Int { Err("later") }

fn run() {
    contextual() ?? 0
    later()
}
"#;

fn normalize_journey_paths(stderr: &str) -> String {
    let mut normalized = stderr
        .lines()
        .map(|line| {
            let Some(open) = line.find(" (") else {
                return line.to_string();
            };
            let Some(colon) = line.rfind(':') else {
                return line.to_string();
            };
            if !line[open + 2..colon].contains(".jet") {
                return line.to_string();
            }
            format!("{}<source>:{}", &line[..open + 2], &line[colon + 1..])
        })
        .map(|line| {
            let Some(start) = line.match_indices("jet_").find_map(|(start, _)| {
                (start == 0 || line.as_bytes().get(start - 1) == Some(&b'/')).then_some(start)
            }) else {
                return line;
            };
            let Some(end) = line[start..].find(".jet").map(|offset| start + offset + 4) else {
                return line;
            };
            format!("{}<source>{}", &line[..start], &line[end..])
        })
        .collect::<Vec<_>>()
        .join("\n");
    if stderr.ends_with('\n') {
        normalized.push('\n');
    }
    normalized
}

fn runtime_error_suffix(stderr: &str) -> &str {
    stderr
        .find("Error:")
        .map(|start| &stderr[start..])
        .unwrap_or_else(|| panic!("runtime error missing from stderr: {stderr}"))
}

#[test]
fn declared_conversion_keeps_one_report_across_runtime_tiers() {
    let (jit_code, jit_out, jit_err) = tir_support::default_run("failure_conversion_tiers", CONVERSION);
    assert_eq!(jit_code, 1, "default JIT must report the converted failure");
    assert!(
        jit_out.is_empty(),
        "converted failure must not print stdout"
    );
    assert!(jit_err.contains("converted"), "JIT report: {jit_err}");

    let (interpreter_code, interpreter_out, interpreter_err) =
        tir_support::default_run("failure_conversion_tiers", CONVERSION);
    assert_eq!(interpreter_code, jit_code);
    assert_eq!(interpreter_out, jit_out);
    assert_eq!(
        normalize_journey_paths(&interpreter_err),
        normalize_journey_paths(&jit_err)
    );

    if tir_support::have_rustc() {
        let (aot_code, aot_out, aot_err) =
            tir_support::build_and_run_full("failure_conversion_tiers", "main", CONVERSION);
        assert_eq!(aot_code, jit_code);
        assert_eq!(aot_out, jit_out);
        assert_eq!(
            normalize_journey_paths(&aot_err),
            normalize_journey_paths(&jit_err)
        );
    }
}

#[test]
fn core_net_error_conversion_renders_in_resident_jit() {
    let (jit_code, jit_out, jit_err) =
        tir_support::default_run("net_error_conversion_tiers", NET_ERROR_CONVERSION);
    assert_eq!(
        jit_code, 1,
        "default JIT must report the NetError conversion"
    );
    assert!(
        jit_out.is_empty(),
        "failed DNS lookup must not print stdout"
    );
    let jit_runtime = runtime_error_suffix(&jit_err);
    assert!(
        jit_runtime.contains("network timeout must be non-negative"),
        "JIT report: {jit_err}"
    );
    assert!(
        jit_runtime.contains("(type: NetError)"),
        "JIT report lost the NetError type: {jit_err}"
    );
    assert!(
        jit_runtime.contains("conversion"),
        "JIT report lost the conversion trail: {jit_err}"
    );

    let (interpreter_code, interpreter_out, interpreter_err) =
        tir_support::default_run("net_error_conversion_tiers", NET_ERROR_CONVERSION);
    assert_eq!(interpreter_code, jit_code);
    assert_eq!(interpreter_out, jit_out);
    assert_eq!(
        normalize_journey_paths(&interpreter_err),
        normalize_journey_paths(&jit_err)
    );

    if tir_support::have_rustc() {
        let (aot_code, aot_out, aot_err) = tir_support::build_and_run_full(
            "net_error_conversion_tiers",
            "main",
            NET_ERROR_CONVERSION,
        );
        assert_eq!(aot_code, jit_code);
        assert_eq!(aot_out, jit_out);
        assert_eq!(
            normalize_journey_paths(runtime_error_suffix(&aot_err)),
            normalize_journey_paths(jit_runtime)
        );
    }
}

#[test]
fn recovered_context_does_not_leak_into_a_later_failure() {
    let (jit_code, jit_out, jit_err) =
        tir_support::default_run("failure_recovery_tiers", RECOVERED_CONTEXT);
    assert_eq!(jit_code, 1, "default JIT must report the later failure");
    assert!(jit_out.is_empty(), "later failure must not print stdout");
    assert!(jit_err.contains("later"), "JIT report: {jit_err}");
    assert!(
        !jit_err.contains("loading"),
        "recovered context leaked into JIT report: {jit_err}"
    );

    let (interpreter_code, interpreter_out, interpreter_err) =
        tir_support::default_run("failure_recovery_tiers", RECOVERED_CONTEXT);
    assert_eq!(interpreter_code, jit_code);
    assert_eq!(interpreter_out, jit_out);
    assert_eq!(
        normalize_journey_paths(&interpreter_err),
        normalize_journey_paths(&jit_err)
    );

    if tir_support::have_rustc() {
        let (aot_code, aot_out, aot_err) =
            tir_support::build_and_run_full("failure_recovery_tiers", "main", RECOVERED_CONTEXT);
        assert_eq!(aot_code, jit_code);
        assert_eq!(aot_out, jit_out);
        assert_eq!(
            normalize_journey_paths(&aot_err),
            normalize_journey_paths(&jit_err)
        );
    }
}

#[test]
fn failure_contract_matrix_matches_comptime_and_hosted_tiers() {
    tir_support::assert_tiers_agree_with_application_policy(
        "failure_contract_matrix_tiers",
        MATRIX,
        MATRIX_STDOUT,
        "name: \"failure_contract_matrix_tiers\"\nversion: \"0.1.0\"\nauthority: { holds: { allow: [IO, Mem.Alloc] } }\n",
    );
}

#[test]
fn failure_contract_matrix_matches_web_js_and_wasm() {
    if !have_tool("rustc") || !have_tool("node") || !have_wasm_target() {
        eprintln!("note: skipping failure matrix web execution (need rustc, wasm32, and node)");
        return;
    }

    let output = jet::compile_web_with_path(MATRIX, "tests/fixtures/failure_tier_parity.jet")
        .expect("failure matrix must compile for web");
    let web = output.web.expect("web compile must produce artifacts");
    let scratch = common::Scratch::new("failure-tier-parity-web");
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
        .expect("spawn web wasm rustc");
    assert!(
        wasm.status.success(),
        "rustc rejected failure matrix web output: {}",
        String::from_utf8_lossy(&wasm.stderr)
    );

    let js = Command::new("node")
        .current_dir(&scratch.path)
        .arg("app.js")
        .output()
        .expect("run web JS failure matrix");
    assert!(
        js.status.success(),
        "web JS failure matrix failed: {}",
        String::from_utf8_lossy(&js.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&js.stdout), MATRIX_STDOUT);

    fs::write(
        scratch.join("wasm.mjs"),
        r#"
const { instantiateWasm, takeWasmError } = await import("./jet_dom_runtime.js");
const instance = await instantiateWasm("./app.wasm");
const status = instance.exports.jet_export_run();
if (status !== 0) throw new Error(`failure matrix Wasm status: ${status}`);
if (takeWasmError(instance.exports)?.tag !== "Ok") throw new Error("failure matrix Wasm stopped");
"#,
    )
    .unwrap();
    let wasm_run = Command::new("node")
        .current_dir(&scratch.path)
        .arg("wasm.mjs")
        .output()
        .expect("run web Wasm failure matrix");
    assert!(
        wasm_run.status.success(),
        "web Wasm failure matrix failed: {}",
        String::from_utf8_lossy(&wasm_run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&wasm_run.stdout), MATRIX_STDOUT);
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
