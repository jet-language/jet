mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use std::fs;
use std::process::Command;

use jet::compile;

const SOURCE: &str = r#"
#Error
enum TextError { Bad }

Pattern :: distinct String

impl Pattern.CheckedText {
    type Error = TextError

    fn check(text: String) !TextError -[]> {
        if text == "" { return Err(TextError.Bad) }
        return Ok()
    }

    fn encode_hole<T: Printable>(value: T) String -[]> {
        return "{hole}"
    }
}

fn run() {
    value :: Pattern{"hello"}
    print("{value.raw()}")
}
"#;

const TIERS_SOURCE: &str = r#"
#Error
enum TextError { Bad }

Pattern :: distinct String

impl Pattern.CheckedText {
    type Error = TextError

    fn check(text: String) !TextError -[]> {
        if text == "hello [1]" || text == "ok" { return Ok() }
        return Err(TextError.Bad)
    }

    fn encode_hole<T: Printable>(value: T) String -[]> {
        return "[1]"
    }
}

fn accepts(text: String) Bool -[]> {
    result :: Pattern.from(text)
    result ? value -> return true ! error -> return false
}

@comptime_good :: accepts("ok")
@comptime_bad :: accepts("")

fn run() {
    literal :: Pattern{"hello {1}"}
    print(literal.raw())

    good :: Pattern.from("ok")
    good ? value -> print(value.raw()) ! error -> print("good rejected")

    bad :: Pattern.from("")
    bad ? value -> print("bad accepted") ! error -> print("bad rejected")

    runtime_good :: accepts("ok")
    runtime_bad :: accepts("")
    print("{@comptime_good}")
    print("{@comptime_bad}")
    print("{runtime_good}")
    print("{runtime_bad}")
}
"#;

const TIERS_STDOUT: &str = "hello [1]\nok\nbad rejected\ntrue\nfalse\ntrue\nfalse\n";

const DYNAMIC_ERROR_SOURCE: &str = r#"
#Error
enum PatternError { Rejected }

Pattern :: distinct String

impl Pattern.CheckedText {
    type Error = PatternError

    fn check(text: String) !PatternError -[]> {
        if text == "bad" { return Err(PatternError.Rejected) }
        return Ok()
    }

    fn encode_hole<T: Printable>(value: T) String -[]> {
        return ""
    }
}

impl PatternError -> Err {
    return Err("pattern rejected", code: "E_PATTERN", cause: Err("invalid shape"))
}

fn parse(text: String) Pattern -[]> {
    pattern :: Pattern.from(text)
    return Ok(pattern)
}

fn load(text: String) Pattern -[]> {
    pattern :: parse(text)?("loading pattern")
    return Ok(pattern)
}

fn run() {
    _ :: load("bad")
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
        .collect::<Vec<_>>()
        .join("\n");
    if stderr.ends_with('\n') {
        normalized.push('\n');
    }
    normalized
}

#[test]
fn ordinary_checked_text_source_compiles() {
    let output = compile(SOURCE).expect("ordinary CheckedText source should compile");
    assert!(output.rust.contains("pub trait CheckedText"));
    assert!(!output.rust.contains("__jet_CheckedText"));
    assert!(!output.rust.contains("jet_checked_text_from"));
}

#[test]
fn malformed_checked_text_impl_reports_the_trait_contract() {
    let source = r#"
Pattern :: distinct String

impl Pattern.CheckedText {
    fn check(text: String) !Error -[]> { return }
    fn encode_hole<T: Printable>(value: T) String -[]> { return "" }
}

fn run() {}
"#;
    let diagnostics = compile(source).expect_err("missing associated Error must be rejected");
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == "E0913"));
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == "E0907"));
}

#[test]
fn checked_text_raw_requires_unsafe() {
    let source = r#"
Pattern :: distinct String

impl Pattern.CheckedText {
    type Error = Error
    fn check(text: String) !Error -[]> { return }
    fn encode_hole<T: Printable>(value: T) String -[]> { return "" }
}

fn run() {
    value :: Pattern.raw("already checked")
    print(value.raw())
}
"#;
    let diagnostics = compile(source).expect_err("raw construction must require unsafe");
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == "E0387"));
}

#[test]
fn checked_text_acceptance_matches_comptime_aot_jit_interpreter_and_web() {
    tir_support::assert_tiers_agree(
        "checked_text_tiers",
        TIERS_SOURCE,
        TIERS_STDOUT,
    );
    assert_checked_text_web_tier();
}

#[test]
fn dynamic_checked_text_from_keeps_the_shared_error_report() {
    let generated = compile(DYNAMIC_ERROR_SOURCE).expect("dynamic CheckedText source should compile");
    assert!(generated.rust.contains("pub trait CheckedText"));
    assert!(!generated.rust.contains("__jet_CheckedText"));
    assert!(!generated.rust.contains("jet_checked_text_from"));
    assert!(generated.rust.contains("__jet_errconv_PatternError_to_Err"));

    let (jit_code, jit_out, jit_err) =
        tir_support::jit_run("checked_text_dynamic_error", DYNAMIC_ERROR_SOURCE);
    assert_eq!(jit_code, 1, "dynamic checked text failure must escape: {jit_err}");
    assert!(jit_out.is_empty(), "dynamic checked text failure printed stdout: {jit_out}");
    assert!(jit_err.starts_with("Error [E_PATTERN]: pattern rejected (type: PatternError)\n"));
    assert!(jit_err.contains("  cause: invalid shape\n"));
    assert!(jit_err.contains("  conversion: PatternError -> Err\n"));
    assert!(jit_err.contains(") — loading pattern\n"));
    assert!(jit_err.contains("Trail [E3002]"));

    let (interpreter_code, interpreter_out, interpreter_err) =
        tir_support::interpreter_run("checked_text_dynamic_error", DYNAMIC_ERROR_SOURCE);
    assert_eq!(interpreter_code, jit_code);
    assert_eq!(interpreter_out, jit_out);
    assert_eq!(
        normalize_journey_paths(&interpreter_err),
        normalize_journey_paths(&jit_err)
    );

    if tir_support::have_rustc() {
        let (aot_code, aot_out, aot_err) = tir_support::build_and_run_full(
            "checked_text_dynamic_error",
            "main",
            DYNAMIC_ERROR_SOURCE,
        );
        assert_eq!(aot_code, jit_code);
        assert_eq!(aot_out, jit_out);
        assert_eq!(
            normalize_journey_paths(&aot_err),
            normalize_journey_paths(&jit_err)
        );
    }
}

fn assert_checked_text_web_tier() {
    let output = jet::compile_web_with_path(TIERS_SOURCE, "checked_text_web.jet")
        .expect("ordinary CheckedText source should compile for web");
    let web = output.web.expect("ordinary CheckedText source should produce web artifacts");
    assert!(web.wasm_rust.contains("__jet_checked_text__Pattern__check"));
    assert!(web
        .wasm_rust
        .contains("__jet_checked_text__Pattern__encode_hole"));

    let have_tool = |name: &str| {
        Command::new(name)
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    };
    let have_wasm_target = Command::new("rustc")
        .args([
            "--print",
            "target-libdir",
            "--target",
            "wasm32-unknown-unknown",
        ])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    if !have_tool("rustc") || !have_tool("node") || !have_wasm_target {
        eprintln!("note: skipping checked-text web execution (need rustc, wasm32 target, and node)");
        return;
    }

    let scratch = common::Scratch::new("checked-text-web");
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
        .expect("spawn checked-text web rustc");
    assert!(
        wasm.status.success(),
        "rustc rejected checked-text web output: {}",
        String::from_utf8_lossy(&wasm.stderr)
    );

    let node = Command::new("node")
        .current_dir(&scratch.path)
        .arg("app.js")
        .output()
        .expect("spawn checked-text web app");
    assert!(
        node.status.success(),
        "node rejected checked-text web output: stdout={} stderr={}",
        String::from_utf8_lossy(&node.stdout),
        String::from_utf8_lossy(&node.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&node.stdout),
        TIERS_STDOUT
    );
}
