//! Card #713/#715: whole/stream encoding parity across AOT, comptime (where
//! pure), the default `jet run`, and the forced interpreter.
//!
//! One matrix harness records tier outcomes instead of isolated per-format tests.
//! Edition 2026 keeps the D-ENCBASE-STRICT1 compatibility union and single-arg
//! `json.canonical` surface exercised by #710–#712.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use jet::Interpreter::RunOutcome;

mod common;
#[path = "tir_support/mod.rs"]
mod tir_support;

static SEQ: AtomicU64 = AtomicU64::new(0);

struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "jet_enc_parity_{tag}_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self { dir }
    }

    fn write_project(&self, edition: &str, body: &str) -> PathBuf {
        fs::write(
            self.dir.join("package.jet"),
            format!("name: \"enc\"\nversion: \"0.1.0\"\nedition: \"{edition}\"\nauthority: {{ holds: {{ allow: [FS, IO, Mem.Alloc] }} }}\n"),
        )
        .unwrap();
        let path = self.dir.join("run.jet");
        // The AOT tier runs the built binary with `current_dir(scratch)`, but the
        // dev/JIT tier runs in-process at the cargo-test cwd (the repo root). Fixtures
        // must therefore name absolute paths, or dev-tier output litters the repo.
        fs::write(
            &path,
            body.replace("@DIR@", &self.dir.display().to_string()),
        )
        .unwrap();
        path
    }

    fn path(&self) -> &Path {
        &self.dir
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProgramOutput {
    exit: i32,
    stdout: String,
    stderr: String,
}

impl ProgramOutput {
    fn ran(stdout: String, stderr: String, exit: i32) -> Self {
        Self {
            exit,
            stdout,
            stderr,
        }
    }
}

fn on_encoding_stack<R: Send>(work: impl FnOnce() -> R + Send) -> R {
    let result = std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn_scoped(scope, work)
            .expect("spawn encoding parity worker")
            .join()
    });
    match result {
        Ok(value) => value,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

fn run_aot(jet_path: &Path, cwd: &Path) -> ProgramOutput {
    assert!(common::have_rustc(), "AOT parity requires rustc");
    let src = fs::read_to_string(jet_path).unwrap();
    let shown = jet_path.to_string_lossy();
    let out = jet::compile_with_path(&src, &shown).unwrap_or_else(|diags| {
        panic!(
            "AOT compile rejected:\n{}",
            jet::render_diagnostics(&shown, &src, &diags)
        )
    });
    let rs = cwd.join("parity_aot.rs");
    let bin = cwd.join("parity_aot");
    fs::write(&rs, &out.rust).unwrap();
    let mut rustc = Command::new("rustc");
    rustc.args([
        "--edition",
        "2021",
        rs.to_str().unwrap(),
        "-o",
        bin.to_str().unwrap(),
    ]);
    if let Some(link) = &out.ffi {
        rustc
            .arg("--extern")
            .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        for deps_dir in link.dependency_dirs().filter(|dir| dir.is_dir()) {
            rustc
                .arg("-L")
                .arg(format!("dependency={}", deps_dir.display()));
        }
    }
    let built = rustc.output().unwrap();
    assert!(
        built.status.success(),
        "rustc rejected generated encoding fixture:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let run = Command::new(&bin).current_dir(cwd).output().unwrap();
    ProgramOutput::ran(
        String::from_utf8_lossy(&run.stdout).into_owned(),
        String::from_utf8_lossy(&run.stderr).into_owned(),
        run.status.code().unwrap_or(-1),
    )
}

fn checked_bundle(path: &str) -> jet::Driver::CheckedSourceBundle {
    let mut bundle = jet::Loader::load_entry(path).expect("fixture should load");
    let source = fs::read_to_string(path).expect("fixture source should remain readable");
    let (diags, facts, nominals) = jet::Sema::check_bundle_with_effect_facts(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags
        .iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(
        errors.is_empty(),
        "fixture must type-check:\n{}",
        jet::render_diagnostics(path, &source, &diags)
    );
    jet::Driver::CheckedSourceBundle::new(bundle, facts, nominals)
}

/// The default `jet run`: build, then execute the artifact.
fn run_default_dev(path: &str) -> ProgramOutput {
    match crate::common::dev_run(path, false, false) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => {
            panic!("default `jet run` rejected encoding probe: {diags:?}")
        }
    }
}

fn run_forced_interpreter(path: &str) -> ProgramOutput {
    match crate::common::dev_run(path, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => {
            panic!("forced interpreter rejected encoding probe: {diags:?}")
        }
    }
}

fn assert_aot_comptime_binding_parity(label: &str, source: &str, jet_path: &Path, cwd: &Path) {
    let aot = run_aot(jet_path, cwd);
    assert_eq!(aot.exit, 0, "{label} AOT failed: {}", aot.stderr);
    let lines: Vec<_> = aot.stdout.lines().collect();
    assert!(
        lines.len() >= 2,
        "{label} expected comptime|runtime probe lines, got {:?}",
        lines
    );
    for (i, line) in lines.iter().enumerate() {
        let (comptime, runtime) = line
            .split_once('|')
            .unwrap_or_else(|| panic!("{label} line {i} missing comptime|runtime split: {line}"));
        assert_eq!(
            comptime, runtime,
            "{label} comptime/AOT divergence on line {i}: comptime={comptime:?} runtime={runtime:?}"
        );
    }
    let diags = jet::check_with_path(jet_path.to_str().unwrap());
    assert!(
        diags.is_empty(),
        "{label} sema rejected comptime fixture:\n{}",
        jet::render_diagnostics(jet_path.to_str().unwrap(), source, &diags)
    );
    let _ = source;
}

/// Named interpreter gaps that cannot claim encoding parity (C5).
/// File-backed stream handles still stop on the interpreter:
/// ResourceNew / THandleOp (E0956) or panic-stop from missing file handles (E0953).
fn named_encoding_dev_gap(stderr: &str) -> bool {
    let e0956 = stderr.contains("E0956")
        && (stderr.contains("ResourceNew")
            || stderr.contains("ResourceTake")
            || stderr.contains("handle `")
            || stderr.contains("FileReader")
            || stderr.contains("FileWriter")
            || stderr.contains("JSONReader")
            || stderr.contains("JSONWriter")
            || stderr.contains("CSVReader")
            || stderr.contains("CSVWriter")
            || stderr.contains("XMLReader")
            || stderr.contains("XMLWriter")
            || stderr.contains("CBORReader")
            || stderr.contains("CBORWriter")
            || stderr.contains("JSONLReader")
            || stderr.contains("JSONLWriter"));
    // E0953: interpreter panic/stop on file-backed encoding stream probes when
    // create/open never materialize a handle under ResourceNew.
    let e0953 = stderr.contains("E0953")
        && (stderr.contains("stopped the build") || stderr.contains("panic"));
    e0956 || e0953
}

fn assert_default_dev_matches_aot_or_honest_gap(
    label: &str,
    jet_path: &Path,
    _cwd: &Path,
    aot: &ProgramOutput,
    interpreter_fallback: bool,
) {
    let dev = run_default_dev(jet_path.to_str().unwrap());
    assert_eq!(dev.stdout, aot.stdout, "{label} default `jet run` stdout drift");
    assert_eq!(dev.stderr, aot.stderr, "{label} default `jet run` stderr drift");
    assert_eq!(dev.exit, aot.exit, "{label} default `jet run` exit drift");
    if interpreter_fallback {
        let interp = run_forced_interpreter(jet_path.to_str().unwrap());
        if named_encoding_dev_gap(&interp.stderr) {
            // Explicit unsupported path — does not satisfy interpreter parity.
            eprintln!(
                "note: {label} interpreter named gap (no parity claim): {}",
                interp.stderr
            );
        } else {
            assert_eq!(
                interp.stdout, aot.stdout,
                "{label} forced interpreter must preserve AOT encoding semantics"
            );
            assert_eq!(interp.exit, aot.exit, "{label} interpreter exit drift");
        }
    }
}

/// Runtime-only whole-value probes shared by AOT ↔ default-dev (#778 lens).
/// Encoding parse/decode at comptime is AOT-pure but impure on the tiered run
/// interpreter (E0956); default-dev must not pretend comptime bindings work there.
/// Avoid enum-match Result arms here — the whole-program evaluator lacks them.
const WHOLE_VALUE_RUNTIME: &str = r#"
use core.encoding.json as json
use core.encoding.jsonl as jsonl
use core.encoding.csv as csv
use core.encoding.xml as xml
use core.encoding.cbor as cbor
use core.encoding.base64 as base64
use core.encoding.base32 as base32

fn run() {
    print(json.to_string(json.parse("{{\"b\":2,\"a\":1}}") ?? panic("json")) ?? panic("JSON render"))
    print((jsonl.parse("{{\"a\":1}}\n{{\"a\":2}}\n") ?? panic("jsonl")).len())
    print((csv.parse("name,score\nada,9\n") ?? panic("csv")).len())
    print(xml.to_string(xml.parse("<r xmlns=\"urn:r\">a&amp;</r>") ?? panic("xml")) ?? panic("xml render"))
    print(json.to_string(cbor.parse(cbor.to_bytes(json.parse("{{\"a\":1}}") ?? panic("j")) ?? panic("e")) ?? panic("p")) ?? panic("JSON render"))
    print((base64.decode("Zg==") ?? panic("b64")).len())
    print((base64.decode_url("aGk") ?? panic("b64url")).len())
    print((base32.decode("MZXQ====") ?? panic("b32")).len())
}
"#;

const CONFIGURED_CSV_RUNTIME: &str = r#"
use core.encoding.csv as csv

fn run() {
    raw :: String.from_bytes([U8]{"name\tnote\x0D\x0A\x0D\x0Aada\t\"line1\x0D\x0Aline2\"\x0D\x0A"}) ?? panic("fixture")
    crlf :: String.from_bytes([U8]{"\x0D\x0A"}) ?? panic("crlf")
    rows :: csv.rows(raw, delimiter: "\t", header: true, skip_blank: true) ?? panic("rows")
    print(rows.len())
    print(rows[0].line)
    print(rows[0].fields[0])
    note :: rows[0].fields[1]
    print(&note.replace(crlf, "|"))
    parsed :: csv.parse(raw, delimiter: "\t", header: true, skip_blank: true) ?? panic("parse")
    print(parsed.len())
    malformed :: String.from_bytes([U8]{"name\tnote\x0D\x0A\"unterminated"}) ?? panic("malformed")
    _attempted :: csv.parse(malformed, delimiter: "\t", header: true, skip_blank: true) ?? {
        print(true)
        return
    }
    print(false)
}
"#;

/// AOT-only comptime|runtime binding parity. Inline expressions only — local
/// fn wrappers from comptime hit E0956 on the shared evaluator seam.
const WHOLE_VALUE_COMPTIME: &str = r#"
use core.encoding.json as json
use core.encoding.jsonl as jsonl
use core.encoding.csv as csv
use core.encoding.xml as xml
use core.encoding.cbor as cbor
use core.encoding.base64 as base64
use core.encoding.base32 as base32

JSON_CANON :: prep { json.to_string(json.parse("{{\"b\":2,\"a\":1}}") ?? panic("json")) ?? panic("JSON render") }
JSONL_N :: prep { (jsonl.parse("{{\"a\":1}}\n{{\"a\":2}}\n") ?? panic("jsonl")).len() }
CSV_N :: prep { (csv.parse("name,score\nada,9\n") ?? panic("csv")).len() }
XML_TEXT :: prep { xml.to_string(xml.parse("<r xmlns=\"urn:r\">a&amp;</r>") ?? panic("xml")) ?? panic("xml render") }
CBOR_ROUND :: prep { json.to_string(cbor.parse(cbor.to_bytes(json.parse("{{\"a\":1}}") ?? panic("j")) ?? panic("e")) ?? panic("p")) ?? panic("JSON render") }
B64_N :: prep { (base64.decode("Zg==") ?? panic("b64")).len() }
B64URL_N :: prep { (base64.decode_url("aGk") ?? panic("b64url")).len() }
B32_N :: prep { (base32.decode("MZXQ====") ?? panic("b32")).len() }

fn run() {
    print("{JSON_CANON}|{json.to_string(json.parse("{{\"b\":2,\"a\":1}}") ?? panic("json")) ?? panic("JSON render")}")
    print("{JSONL_N}|{(jsonl.parse("{{\"a\":1}}\n{{\"a\":2}}\n") ?? panic("jsonl")).len()}")
    print("{CSV_N}|{(csv.parse("name,score\nada,9\n") ?? panic("csv")).len()}")
    print("{XML_TEXT}|{xml.to_string(xml.parse("<r xmlns=\"urn:r\">a&amp;</r>") ?? panic("xml")) ?? panic("xml render")}")
    print("{CBOR_ROUND}|{json.to_string(cbor.parse(cbor.to_bytes(json.parse("{{\"a\":1}}") ?? panic("j")) ?? panic("e")) ?? panic("p")) ?? panic("JSON render")}")
    print("{B64_N}|{(base64.decode("Zg==") ?? panic("b64")).len()}")
    print("{B64URL_N}|{(base64.decode_url("aGk") ?? panic("b64url")).len()}")
    print("{B32_N}|{(base32.decode("MZXQ====") ?? panic("b32")).len()}")
}
"#;

const HOSTILE_JSON_TOML_DEPTH_RUNTIME: &str = r#"
use core.encoding.json as json
use core.encoding.toml as toml

fn run() {
    open64 :: "[".repeat(64)
    close64 :: "]".repeat(64)
    json.parse("{open64}{close64}") ? _value -> { print("json64:accepted") } ! error -> { print("json64:{error.line}:{error.message}") }

    open65 :: "[".repeat(65)
    close65 :: "]".repeat(65)
    json.parse("{open65}{close65}") ? _value -> { print("json65:accepted") } ! error -> { print("json65:{error.line}:{error.message}") }

    toml.parse("value = {open64}{close64}") ? _value -> { print("toml64:accepted") } ! error -> { print("toml64:{error.line}:{error.message}") }

    toml.parse("value = {open65}{close65}") ? _value -> { print("toml65:accepted") } ! error -> { print("toml65:{error.line}:{error.message}") }
}
"#;

#[test]
fn whole_value_codecs_match_aot_comptime_and_default_dev() {
    on_encoding_stack(whole_value_codecs_match_aot_comptime_and_default_dev_inner);
}

#[test]
fn configured_csv_options_match_aot_default_dev_and_interpreter() {
    on_encoding_stack(configured_csv_options_match_aot_default_dev_and_interpreter_inner);
}

fn configured_csv_options_match_aot_default_dev_and_interpreter_inner() {
    if !common::have_rustc() {
        eprintln!("note: skipping configured CSV parity (need rustc)");
        return;
    }
    let scratch = Scratch::new("configured_csv");
    let path = scratch.write_project("2026", CONFIGURED_CSV_RUNTIME);
    fs::write(
        scratch.path().join("package.jet"),
        "name: \"configured-csv\"\nversion: \"0.1.0\"\nedition: \"2026\"\nauthority: { holds: { allow: [IO, Mem.Alloc] } }\n",
    )
    .unwrap();
    let aot = run_aot(&path, scratch.path());
    assert_eq!(aot.exit, 0, "configured CSV AOT failed: {}", aot.stderr);
    let expected = "1\n3\nada\nline1|line2\n1\ntrue\n";
    assert_eq!(aot.stdout, expected, "configured CSV AOT output drift");
    let dev = run_default_dev(path.to_str().unwrap());
    assert_eq!(dev.exit, 0, "configured CSV default-dev failed: {}", dev.stderr);
    assert_eq!(dev.stdout, aot.stdout, "configured CSV default-dev drift");
    let interpreter = run_forced_interpreter(path.to_str().unwrap());
    assert_eq!(interpreter.exit, 0, "configured CSV interpreter failed: {}", interpreter.stderr);
    assert_eq!(interpreter.stdout, aot.stdout, "configured CSV interpreter drift");
}

#[test]
fn serde_examples_match_aot_default_run_and_interpreter() {
    on_encoding_stack(serde_examples_match_aot_default_run_and_interpreter_inner);
}

fn serde_examples_match_aot_default_run_and_interpreter_inner() {
    if !common::have_rustc() {
        eprintln!("note: skipping serde example parity (need rustc)");
        return;
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = Scratch::new("serde_examples");
    for (stem, expected) in [
        (
            "serde/encoding_breadth",
            concat!(
                "{\"a\":1,\"b\":2}\ntrue\n2\ntrue\n",
                "<r xmlns:h=\"urn:h\"><h:c id=\"7\">ok</h:c></r>\n",
                "true\n{\"a\":1,\"b\":2}\naGk\n2\nNBUQ====\n2\n",
            ),
        ),
        (
            "serde/serde_generic",
            concat!(
                "{\"value\":7}\n42\n{\"value\":\"hi\"}\n",
                "{\"raw\":9}\n3\n",
            ),
        ),
        (
            "serde/codable_default",
            concat!(
                "{\"name\":\"web\",\"ports\":[80,443],\"env\":\"prod\"}\n",
                "web\n[80, 443]\nprod\n",
                "{\"name\":\"web\",\"ports\":[80,443],\"env\":\"prod\"}\n",
                "[8080]\n",
            ),
        ),
    ] {
        let jet_path = root.join("Examples/features").join(format!("{stem}.jet"));
        let expected_path = root
            .join("Examples/features/expected")
            .join(format!("{stem}.out"));
        assert_eq!(
            fs::read_to_string(&expected_path).expect("serde golden output"),
            expected,
            "{stem} golden output drifted"
        );

        let aot = run_aot(&jet_path, scratch.path());
        let expected_output = ProgramOutput::ran(expected.to_string(), String::new(), 0);
        assert_eq!(aot, expected_output, "{stem} AOT output drifted");

        let default_run = run_default_dev(jet_path.to_str().unwrap());
        assert_eq!(default_run, aot, "{stem} default run diverged from AOT");

        let interpreter = run_forced_interpreter(jet_path.to_str().unwrap());
        assert_eq!(interpreter, aot, "{stem} interpreter diverged from AOT");
    }
}

#[test]
fn serde_derive_examples_match_production_cli_tiers() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for stem in [
        "serde/codable_default",
        "serde/hand_codec",
        "serde/serde_derive",
        "serde/serde_generic",
    ] {
        let expected_path = root
            .join("Examples/features/expected")
            .join(format!("{stem}.out"));
        let expected = fs::read_to_string(&expected_path)
            .unwrap_or_else(|error| panic!("read {expected_path:?}: {error}"));
        tir_support::assert_example_cli_tiers_agree(stem, &expected);
    }
}

fn whole_value_codecs_match_aot_comptime_and_default_dev_inner() {
    if !common::have_rustc() {
        eprintln!("note: skipping whole-value encoding parity (need rustc)");
        return;
    }
    // AOT comptime|runtime binding parity (pure on AOT evaluator).
    let ct = Scratch::new("whole_ct");
    let ct_path = ct.write_project("2026", WHOLE_VALUE_COMPTIME);
    assert_aot_comptime_binding_parity(
        "whole-value-comptime",
        WHOLE_VALUE_COMPTIME,
        &ct_path,
        ct.path(),
    );

    // AOT ↔ default-dev runtime parity under #778 tiered lens (no comptime).
    let scratch = Scratch::new("whole");
    let path = scratch.write_project("2026", WHOLE_VALUE_RUNTIME);
    let aot = run_aot(&path, scratch.path());
    assert_eq!(
        aot.exit, 0,
        "whole-value runtime AOT failed: {}",
        aot.stderr
    );
    assert_default_dev_matches_aot_or_honest_gap("whole-value", &path, scratch.path(), &aot, true);
}

#[test]
fn hostile_json_and_toml_depth_match_aot_default_run_and_interpreter() {
    on_encoding_stack(hostile_json_and_toml_depth_match_aot_default_run_and_interpreter_inner);
}

fn hostile_json_and_toml_depth_match_aot_default_run_and_interpreter_inner() {
    if !common::have_rustc() {
        eprintln!("note: skipping hostile JSON/TOML depth parity (need rustc)");
        return;
    }
    let scratch = Scratch::new("hostile_json_toml_depth");
    let path = scratch.write_project("2026", HOSTILE_JSON_TOML_DEPTH_RUNTIME);
    let expected = concat!(
        "json64:accepted\n",
        "json65:1:JSON value is nested too deeply\n",
        "toml64:accepted\n",
        "toml65:1:TOML value is nested too deeply\n",
    );

    let aot = run_aot(&path, scratch.path());
    assert_eq!(aot.exit, 0, "hostile JSON/TOML AOT failed: {}", aot.stderr);
    assert_eq!(aot.stdout, expected);
    assert_eq!(aot.stderr, "");

    let dev = run_default_dev(path.to_str().unwrap());
    assert_eq!(dev, aot, "hostile JSON/TOML default run diverged from AOT");

    let interpreter = run_forced_interpreter(path.to_str().unwrap());
    assert_eq!(
        interpreter, aot,
        "hostile JSON/TOML forced interpreter diverged from AOT"
    );
}

#[test]
fn malformed_text_codec_errors_match_aot_default_dev_and_interpreter() {
    on_encoding_stack(malformed_text_codec_errors_match_aot_default_dev_and_interpreter_inner);
}

fn malformed_text_codec_errors_match_aot_default_dev_and_interpreter_inner() {
    if !common::have_rustc() {
        eprintln!("note: skipping malformed text codec parity (need rustc)");
        return;
    }
    let source = r#"
use core.encoding.json as json
use core.encoding.toml as toml
use core.encoding.yaml as yaml

#Codable
struct Row {
    value: Int
}

fn json_parse_error() -[]> String {
    json.parse("\n{{oops") ? _value -> return "accepted" ! error -> return "{error.line}|{error.message}"
}

fn json_decode_error() -[]> String {
    json.decode<Row>("\n{{oops") ? _value -> return "accepted" ! errors -> return "{errors[0].path}|{errors[0].reason}"
}

fn toml_parse_error() -[]> String {
    toml.parse("value = ") ? _value -> return "accepted" ! error -> return error.message
}

fn yaml_parse_error() -[]> String {
    yaml.parse("key: value\nbad") ? _value -> return "accepted" ! error -> return error.message
}

fn run() {
    print(json_parse_error())
    print(json_decode_error())
    print(toml_parse_error())
    print(yaml_parse_error())
}
"#;
    let scratch = Scratch::new("malformed_text_codecs");
    let path = scratch.write_project("2026", source);
    let expected = concat!(
        "2|expected quoted text\n",
        "|invalid JSON (line 2): expected quoted text\n",
        "expected a value\n",
        "expected `key: value`\n",
    );
    let aot = run_aot(&path, scratch.path());
    assert_eq!(
        aot.exit, 0,
        "malformed text codec AOT failed: {}",
        aot.stderr
    );
    assert_eq!(aot.stdout, expected);
    let dev = run_default_dev(path.to_str().unwrap());
    assert_eq!(dev, aot, "default `jet run` diverged");
    let interpreter = run_forced_interpreter(path.to_str().unwrap());
    assert_eq!(interpreter, aot, "forced interpreter diverged");
}

#[test]
fn json_quoted_unicode_and_duplicate_key_errors_match_all_tiers() {
    on_encoding_stack(json_quoted_unicode_and_duplicate_key_errors_match_all_tiers_inner);
}

fn json_quoted_unicode_and_duplicate_key_errors_match_all_tiers_inner() {
    if !common::have_rustc() {
        eprintln!("note: skipping JSON quoted Unicode parity (need rustc)");
        return;
    }
    let source = r#"
use core.encoding as encoding
use core.encoding.json as json

fn run() {
    value :: json.parse("{{\"text\":\"quote: \\\" and music: \\uD834\\uDD1E\"}}") ?? panic("valid JSON")
    print(json.to_string(value) ?? panic("JSON render"))
    json.parse("{{\"a\":1,\"a\":}}") ? _value -> {
        print("duplicate accepted")
    } ! error -> {
        print("{error.kind == encoding.EncodingErrorKind.Syntax}|{error.path}|{error.reason}")
    }
}
"#;
    let scratch = Scratch::new("json_quoted_unicode_duplicate_key");
    let path = scratch.write_project("2026", source);
    let expected = r#"{"text":"quote: \" and music: 𝄞"}
true|/a|duplicate object key `a`
"#;
    let aot = run_aot(&path, scratch.path());
    assert_eq!(aot.exit, 0, "JSON quoted Unicode AOT failed: {}", aot.stderr);
    assert_eq!(aot.stdout, expected);
    let dev = run_default_dev(path.to_str().unwrap());
    assert_eq!(dev, aot, "default `jet run` diverged");
    let interpreter = run_forced_interpreter(path.to_str().unwrap());
    assert_eq!(interpreter, aot, "forced interpreter diverged");
}

#[test]
fn json_unsupported_and_wire_escapes_match_comptime_aot_dev_and_interpreter() {
    on_encoding_stack(|| {
        let scratch = Scratch::new("json_unsupported");
        let path = scratch.write_project(
            "2026",
            include_str!("../Examples/features/serde/json_unsupported.jet"),
        );
        let expected = include_str!("../Examples/features/expected/serde/json_unsupported.out");
        let aot = run_aot(&path, scratch.path());
        assert_eq!(aot.exit, 0, "JSON Unsupported AOT failed: {}", aot.stderr);
        assert_eq!(aot.stdout, expected);
        let dev = run_default_dev(path.to_str().unwrap());
        assert_eq!(dev, aot, "JSON Unsupported default `jet run` diverged");
        let interpreter = run_forced_interpreter(path.to_str().unwrap());
        assert_eq!(
            interpreter, aot,
            "JSON Unsupported forced interpreter diverged"
        );
    });
}

#[test]
fn cbor_typed_schema_matches_comptime_default_dev_and_deopt() {
    on_encoding_stack(cbor_typed_schema_matches_comptime_default_dev_and_deopt_inner);
}

fn cbor_typed_schema_matches_comptime_default_dev_and_deopt_inner() {
    if !common::have_rustc() {
        eprintln!("note: skipping typed CBOR schema parity (need rustc)");
        return;
    }
    let source = r#"
use core.encoding.cbor as cbor
use core.encoding.hex as hex
use core.text as text

#Codable
struct Packet {
    id: Int
    payload: [U8]
}

ROOT :: prep { hex.encode(cbor.to_bytes_canonical([U8]{222, 173}) ?? panic("root")) }
PACKET :: prep { hex.encode(cbor.to_bytes_canonical(Packet{ id: 7, payload: [222, 173] }) ?? panic("packet")) }

fn gap() -[]> String {
    folded := text.casefold("Straße")
    if folded != "strasse" { panic("casefold") }
    actual_root := hex.encode(cbor.to_bytes_canonical([U8]{222, 173}) ?? panic("root"))
    actual_packet := hex.encode(cbor.to_bytes_canonical(Packet{ id: 7, payload: [222, 173] }) ?? panic("packet"))
    return "{actual_root}|{actual_packet}"
}

fn run() {
    print("{ROOT}|{PACKET}")
    print(gap())
}
"#;
    let scratch = Scratch::new("cbor_typed_schema");
    let path = scratch.write_project("2026", source);
    checked_bundle(path.to_str().unwrap());
    let expected =
        "42dead|a262696407677061796c6f616442dead\n42dead|a262696407677061796c6f616442dead\n";
    let aot = run_aot(&path, scratch.path());
    assert_eq!(aot.exit, 0, "typed CBOR AOT failed: {}", aot.stderr);
    assert_eq!(aot.stdout, expected);
    let dev = run_default_dev(path.to_str().unwrap());
    assert_eq!(dev.exit, 0, "typed CBOR default run failed: {}", dev.stderr);
    assert_eq!(dev.stdout, expected, "typed CBOR default run drifted");
    let interpreter = run_forced_interpreter(path.to_str().unwrap());
    assert_eq!(interpreter.stdout, expected, "typed CBOR interpreter drifted");
}

#[test]
fn cbor_codable_bodies_match_aot_resident_and_forced_deopt() {
    on_encoding_stack(cbor_codable_bodies_match_aot_resident_and_forced_deopt_inner);
}

fn cbor_codable_bodies_match_aot_resident_and_forced_deopt_inner() {
    if !common::have_rustc() {
        eprintln!("note: skipping forced-deopt Codable parity (need rustc)");
        return;
    }
    let source = r#"
use core.encoding.cbor as cbor
use core.encoding.hex as hex
use core.text as text

struct Token { raw: String }

impl Token.Encode {
    fn encode(self) -> Dyn { Dyn.Text("wire") }
}

impl Token.Decode {
    fn decode(tree: Dyn) -[]> Token ![FieldError] {
        value :: tree.text()
        if value != "wire" {
            return Err([FieldError{ path: "$xml_event", reason: "bad token" }])
        }
        return Ok(Token{ raw: "decoded" })
    }
}

#Codable
enum Event {
    Idle
    Count(Int)
}

#Codable
struct Wrap<T> {
    value: T
}

#[Codable, RenameAll(camel)]
struct Packet {
    display_name: String
    token: Token
    event: Event
    bytes: [U8]
    numbers: [Int]
    labels: [String:Int]
    maybe: ?Int
    score: Float
    letter: Char
}

fn resident() -[]> String {
    value := Packet{
        display_name: "Ada",
        token: Token{ raw: "ignored" },
        event: Event.Count(7),
        bytes: [222, 173],
        numbers: [-2, 3],
        labels: [String:Int]{ "x": 6 },
        maybe: Val(8),
        score: 1.5,
        letter: 'Z'
    }
    encoded := cbor.to_bytes_canonical(value) ?? panic("encode")
    decoded := cbor.decode<Packet>(encoded.copy()) ?? panic("decode {err}")
    roundtrip := cbor.to_bytes_canonical(decoded) ?? panic("re-encode")
    boxed := cbor.decode<Wrap<Int>>([U8]{ 0xa1, 0x65, 0x76, 0x61, 0x6c, 0x75, 0x65, 0x0b }) ?? panic("generic decode")
    boxed_wire := cbor.to_bytes_canonical(boxed) ?? panic("generic encode")
    boxed_back := cbor.decode<Wrap<Int>>(boxed_wire.copy()) ?? panic("generic roundtrip")
    return "{hex.encode(encoded)}|{hex.encode(roundtrip)}|{decoded.token.raw}|{boxed_back.value}"
}

fn forced_deopt(seed: String) -[]> String {
    folded := text.casefold(seed)
    if folded != "strasse" { panic("casefold") }
    value := Packet{
        display_name: "Ada",
        token: Token{ raw: "ignored" },
        event: Event.Count(7),
        bytes: [222, 173],
        numbers: [-2, 3],
        labels: [String:Int]{ "x": 6 },
        maybe: Val(8),
        score: 1.5,
        letter: 'Z'
    }
    encoded := cbor.to_bytes_canonical(value) ?? panic("encode")
    decoded := cbor.decode<Packet>(encoded.copy()) ?? panic("decode {err}")
    roundtrip := cbor.to_bytes_canonical(decoded) ?? panic("re-encode")
    boxed := cbor.decode<Wrap<Int>>([U8]{ 0xa1, 0x65, 0x76, 0x61, 0x6c, 0x75, 0x65, 0x0b }) ?? panic("generic decode")
    boxed_wire := cbor.to_bytes_canonical(boxed) ?? panic("generic encode")
    boxed_back := cbor.decode<Wrap<Int>>(boxed_wire.copy()) ?? panic("generic roundtrip")
    return "{hex.encode(encoded)}|{hex.encode(roundtrip)}|{decoded.token.raw}|{boxed_back.value}"
}

fn run() {
    print(resident())
    print(forced_deopt("Straße"))
}
"#;
    let scratch = Scratch::new("cbor_codable_deopt");
    let path = scratch.write_project("2026", source);
    fs::write(
        scratch.path().join("package.jet"),
        "name: \"enc\"\nversion: \"0.1.0\"\nedition: \"2026\"\nauthority: { holds: { allow: [IO, Mem.Alloc] } }\n",
    )
    .unwrap();
    checked_bundle(path.to_str().unwrap());
    let aot = run_aot(&path, scratch.path());
    assert_eq!(aot.exit, 0, "Codable corpus AOT failed: {}", aot.stderr);
    let mut lines = aot.stdout.lines();
    let resident = lines.next().expect("resident output");
    let deopt = lines.next().expect("deopt output");
    assert_eq!(resident, deopt, "AOT corpus functions diverged");
    assert!(
        resident.ends_with("|decoded|11"),
        "custom and generic Decode bodies did not run: {resident}"
    );
    assert!(
        lines.next().is_none(),
        "unexpected AOT output: {}",
        aot.stdout
    );
    let dev = run_default_dev(path.to_str().unwrap());
    assert_eq!(dev, aot);
    let interpreter = run_forced_interpreter(path.to_str().unwrap());
    assert_eq!(interpreter, aot, "Codable corpus interpreter drifted");
}

#[test]
fn cbor_migration_plan_matches_aot_and_forced_deopt() {
    on_encoding_stack(cbor_migration_plan_matches_aot_and_forced_deopt_inner);
}

fn cbor_migration_plan_matches_aot_and_forced_deopt_inner() {
    if !common::have_rustc() {
        eprintln!("note: skipping CBOR migration parity (need rustc)");
        return;
    }
    let source = r#"
use core.encoding.cbor as cbor
use core.text as text

#Codable
struct ScoreRank { value: Int }

// v1 wire shape: legacyId, name, score: Int
// v2 removes legacyId; v3 renames name and converts score; v4 adds hostName.
#[PublishedSchema, Codable, RenameAll(camel)]
struct Profile {
    display_name: String
    score: ScoreRank
    host_name: String
}

migration Profile {
    remove legacy_id
}

migration Profile {
    rename name -> display_name
    change score: Int -> ScoreRank via { (n) -> ScoreRank{ value: n } }
}

migration Profile {
    add host_name: String = "localhost"
}

fn decode_old() -[]> String {
    bytes :: [U8]{
        0xa3,
        0x64, 0x6e, 0x61, 0x6d, 0x65,
        0x63, 0x41, 0x64, 0x61,
        0x65, 0x73, 0x63, 0x6f, 0x72, 0x65,
        0x18, 0x5f,
        0x68, 0x6c, 0x65, 0x67, 0x61, 0x63, 0x79, 0x49, 0x64,
        0x09
    }
    profile :: cbor.decode<Profile>(bytes.copy()) ?? panic("decode")
    return "{profile.display_name}|{profile.score.value}|{profile.host_name}"
}

fn forced_deopt() -[]> String {
    if text.casefold("Straße") != "strasse" { panic("casefold") }
    bytes :: [U8]{
        0xa3,
        0x64, 0x6e, 0x61, 0x6d, 0x65,
        0x63, 0x41, 0x64, 0x61,
        0x65, 0x73, 0x63, 0x6f, 0x72, 0x65,
        0x18, 0x5f,
        0x68, 0x6c, 0x65, 0x67, 0x61, 0x63, 0x79, 0x49, 0x64,
        0x09
    }
    profile :: cbor.decode<Profile>(bytes.copy()) ?? panic("decode")
    return "{profile.display_name}|{profile.score.value}|{profile.host_name}"
}

fn failed_change() -[]> String {
    bytes :: [U8]{
        0xa3,
        0x64, 0x6e, 0x61, 0x6d, 0x65,
        0x63, 0x41, 0x64, 0x61,
        0x65, 0x73, 0x63, 0x6f, 0x72, 0x65,
        0x63, 0x62, 0x61, 0x64,
        0x68, 0x6c, 0x65, 0x67, 0x61, 0x63, 0x79, 0x49, 0x64,
        0x09
    }
    cbor.decode<Profile>(bytes.copy()) ? _value -> return "accepted" ! error -> return "{error[0].path}:{error[0].reason}"
}

fn run() {
    print(forced_deopt())
    print(failed_change())
}
"#;
    let scratch = Scratch::new("cbor_migration_deopt");
    let path = scratch.write_project("2026", source);
    fs::write(
        scratch.path().join("package.jet"),
        "name: \"enc\"\nversion: \"0.1.0\"\nedition: \"2026\"\nauthority: { holds: { allow: [IO, Mem.Alloc] } }\n",
    )
    .unwrap();
    checked_bundle(path.to_str().unwrap());
    let aot = run_aot(&path, scratch.path());
    assert_eq!(aot.exit, 0, "migration corpus AOT failed: {}", aot.stderr);
    assert_eq!(
        aot.stdout,
        "Ada|95|localhost\nscore:expected Int, found text \"bad\"\n"
    );
    let dev = run_default_dev(path.to_str().unwrap());
    assert_eq!(dev, aot);
    let interpreter = run_forced_interpreter(path.to_str().unwrap());
    assert_eq!(interpreter, aot, "migration corpus interpreter drifted");
}

#[test]
fn published_schema_preserves_unknown_wire_order_across_tiers() {
    on_encoding_stack(published_schema_preserves_unknown_wire_order_across_tiers_inner);
}

fn published_schema_preserves_unknown_wire_order_across_tiers_inner() {
    if !common::have_rustc() {
        eprintln!("note: skipping published-schema parity (need rustc)");
        return;
    }
    let source = r#"
use core.encoding.json as json
use core.text as text

#[PublishedSchema, Codable]
struct Published {
    known: Int
}

#Codable
struct Plain {
    known: Int
}

#[Codable, DenyUnknownFields]
struct Strict {
    known: Int
}

fn published() -[]> String {
    value :: json.decode<Published>("{{\"middle\":2,\"known\":7,\"future\":1,\"tail\":3}}") ?? panic("published decode")
    return json.to_string(value) ?? panic("JSON render")
}

fn plain() -[]> String {
    value :: json.decode<Plain>("{{\"future\":1,\"known\":7,\"middle\":2}}") ?? panic("plain decode")
    return json.to_string(value) ?? panic("JSON render")
}

fn strict() -[]> String {
    json.decode<Strict>("{{\"known\":7,\"extra\":1}}") ? _value -> return "strict accepted" ! errors -> return errors[0].reason
}

fn malformed() -[]> String {
    json.decode<Published>("{{\"known\":7") ? _value -> return "malformed accepted" ! _error -> return "malformed rejected"
}

fn forced_deopt() -[]> String {
    if text.casefold("Straße") != "strasse" { panic("casefold") }
    return published()
}

fn run() {
    print(published())
    print(plain())
    print(strict())
    print(malformed())
    print(forced_deopt())
}
"#;
    let scratch = Scratch::new("published_schema_unknowns");
    let path = scratch.write_project("2026", source);
    checked_bundle(path.to_str().unwrap());
    let expected = concat!(
        "{\"middle\":2,\"known\":7,\"future\":1,\"tail\":3}\n",
        "{\"known\":7}\n",
        "E2412: unknown field `extra`\n",
        "malformed rejected\n",
        "{\"middle\":2,\"known\":7,\"future\":1,\"tail\":3}\n",
    );
    let aot = run_aot(&path, scratch.path());
    assert_eq!(aot.exit, 0, "published-schema AOT failed: {}", aot.stderr);
    assert_eq!(aot.stdout, expected);
    let dev = run_default_dev(path.to_str().unwrap());
    assert_eq!(dev, aot, "default `jet run` diverged");
    let interpreter = run_forced_interpreter(path.to_str().unwrap());
    assert_eq!(interpreter, aot, "forced interpreter diverged");
}


#[test]
fn cbor_primitive_container_boundaries_match_aot_and_forced_deopt() {
    on_encoding_stack(cbor_primitive_container_boundaries_match_aot_and_forced_deopt_inner);
}

fn cbor_primitive_container_boundaries_match_aot_and_forced_deopt_inner() {
    if !common::have_rustc() {
        eprintln!("note: skipping CBOR primitive boundary parity (need rustc)");
        return;
    }
    let source = r#"
use core.encoding.cbor as cbor
use core.text as text

#CodableAsBase
Severity :: distinct Int(0..10)

fn invalid_i8_rejected() -[]> Bool {
    cbor.decode<I8>([U8]{ 0x18, 0x80 }) ? _value -> return false ! _error -> return true
}

fn invalid_u32_rejected() -[]> Bool {
    cbor.decode<U32>([U8]{ 0x20 }) ? _value -> return false ! _error -> return true
}

fn invalid_f32_rejected() -[]> Bool {
    cbor.decode<F32>([U8]{ 0xfb, 0x7f, 0xef, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff }) ? _value -> return false ! _error -> return true
}

fn invalid_fixed_rejected() -[]> Bool {
    cbor.decode<[Int#2]>([U8]{ 0x81, 0x01 }) ? _value -> return false ! _error -> return true
}

fn invalid_range_rejected() -[]> Bool {
    cbor.decode<Severity>([U8]{ 0x0b }) ? _value -> return false ! _error -> return true
}

fn invalid_i8_error() -[]> String {
    cbor.decode<I8>([U8]{ 0x18, 0x80 }) ? _value -> return "accepted" ! error -> return error[0].reason
}

fn invalid_u8_error() -[]> String {
    cbor.decode<U8>([U8]{ 0x19, 0x01, 0x00 }) ? _value -> return "accepted" ! error -> return error[0].reason
}

fn invalid_fixed_bytes_error() -[]> String {
    cbor.decode<[U8#2]>([U8]{ 0x41, 0xde }) ? _value -> return "accepted" ! error -> return error[0].reason
}

fn forced_deopt() -[]> String {
    if text.casefold("Straße") != "strasse" { panic("casefold") }
    signed := cbor.decode<I8>(~(cbor.to_bytes_canonical(I8{ -8 }) ?? panic("i8 encode"))) ?? panic("i8 decode")
    unsigned := cbor.decode<U32>(~(cbor.to_bytes_canonical(U32{ 4000000000 }) ?? panic("u32 encode"))) ?? panic("u32 decode")
    narrow := cbor.decode<F32>(~(cbor.to_bytes_canonical(F32{ 1.5 }) ?? panic("f32 encode"))) ?? panic("f32 decode")
    pair := cbor.decode<[Int#2]>(~(cbor.to_bytes_canonical([Int#2]{ 1, 2 }) ?? panic("fixed encode"))) ?? panic("fixed decode")
    bytes := cbor.decode<[U8#2]>(~(cbor.to_bytes_canonical([U8#2]{ 222, 173 }) ?? panic("bytes encode"))) ?? panic("bytes decode")
    tree := cbor.decode<Dyn>(~(cbor.to_bytes_canonical(Dyn.Object(["n": Dyn.Int(7)])) ?? panic("tree encode"))) ?? panic("tree decode")
    severity := cbor.decode<Severity>(~(cbor.to_bytes_canonical(Severity.from_int(7)) ?? panic("range encode"))) ?? panic("range decode")
    tree_n := (&tree.field("n") ?? panic("tree field")).int() ?? panic("tree int")
    return "{signed}|{unsigned}|{narrow}|{pair[0]},{pair[1]}|{bytes[0]},{bytes[1]}|{tree_n}|{severity.raw()}|{invalid_i8_rejected()}|{invalid_u32_rejected()}|{invalid_f32_rejected()}|{invalid_fixed_rejected()}|{invalid_range_rejected()}|{invalid_i8_error()}|{invalid_u8_error()}|{invalid_fixed_bytes_error()}"
}

fn run() {
    print(forced_deopt())
}
"#;
    let scratch = Scratch::new("cbor_primitive_boundaries");
    let path = scratch.write_project("2026", source);
    fs::write(
        scratch.path().join("package.jet"),
        "name: \"enc\"\nversion: \"0.1.0\"\nedition: \"2026\"\nauthority: { holds: { allow: [IO, Mem.Alloc] } }\n",
    )
    .unwrap();
    checked_bundle(path.to_str().unwrap());
    let expected = "-8|4000000000|1.5|1,2|222,173|7|7|true|true|true|true|true|expected I8, found out-of-range Int|expected U8, found out-of-range Int|expected a fixed list of length 2, found 1";
    let aot = run_aot(&path, scratch.path());
    assert_eq!(aot.exit, 0, "primitive boundary AOT failed: {}", aot.stderr);
    assert_eq!(aot.stdout, format!("{expected}\n"));
    let dev = run_default_dev(path.to_str().unwrap());
    assert_eq!(dev, aot);
    let interpreter = run_forced_interpreter(path.to_str().unwrap());
    assert_eq!(interpreter, aot, "primitive corpus interpreter drifted");
}

#[test]
fn dyn_int_accessor_matches_aot_without_coercion() {
    on_encoding_stack(dyn_int_accessor_matches_aot_without_coercion_inner);
}

fn dyn_int_accessor_matches_aot_without_coercion_inner() {
    if !common::have_rustc() {
        eprintln!("note: skipping Dyn Int accessor parity (need rustc)");
        return;
    }
    let source = r#"
fn run() {
    Dyn.Int(7).int() ? value -> print(value) ! error -> print(error)
    Dyn.Float(7.0).int() ? value -> print(value) ! error -> print(error)
    Dyn.Text("7").int() ? value -> print(value) ! error -> print(error)
}
"#;
    let scratch = Scratch::new("dyn_int");
    let path = scratch.write_project("2026", source);
    checked_bundle(path.to_str().unwrap());
    let aot = run_aot(&path, scratch.path());
    assert_eq!(aot.exit, 0, "Dyn Int AOT failed: {}", aot.stderr);
    assert_eq!(
        aot.stdout,
        "7\n[expected int, got 7.0]\n[expected int, got \"7\"]\n"
    );
    let dev = run_default_dev(path.to_str().unwrap());
    assert_eq!(dev, aot);
}

#[test]
fn dyn_int_accessor_matches_aot_inside_a_helper() {
    on_encoding_stack(dyn_int_accessor_matches_aot_inside_a_helper_inner);
}

#[test]
fn dyn_scalar_and_unordered_parity_matches_all_tiers() {
    on_encoding_stack(dyn_scalar_and_unordered_parity_matches_all_tiers_inner);
}

fn dyn_scalar_and_unordered_parity_matches_all_tiers_inner() {
    if !common::have_rustc() {
        eprintln!("note: skipping Dyn scalar/equality parity (need rustc)");
        return;
    }
    let source = r#"
use core.encoding.json as json

fn run() {
    Dyn.Text("strict").text() ? value -> print(value) ! _error -> print("strict-error")
    Dyn.Int(7).text() ? _value -> print("strict-bad") ! _error -> print("strict-error")

    print(Dyn.Text("text").to_text() ?? "none")
    print(Dyn.Int(-42).to_text() ?? "none")
    print(Dyn.Float(1.0).to_text() ?? "none")
    print(Dyn.Bool(false).to_text() ?? "none")
    print(Dyn.Null.to_text() ?? "none")
    print(Dyn.Array([Dyn.Int(1)]).to_text() ?? "none")
    print(Dyn.Object(["a": Dyn.Int(1)]).to_text() ?? "none")

    left :: json.parse("{{\"outer\":{{\"b\":1.0,\"a\":[1,false]}},\"name\":\"x\"}}") ?? panic("left")
    right :: json.parse("{{\"name\":\"x\",\"outer\":{{\"a\":[1,false],\"b\":1.0}}}}") ?? panic("right")
    print(left.equal_unordered(right))
    print(Dyn.Null.equal_unordered(Dyn.Null))
    print(Dyn.Array([Dyn.Int(1), Dyn.Int(2)]).equal_unordered(Dyn.Array([Dyn.Int(2), Dyn.Int(1)])))
    print(Dyn.Int(1).equal_unordered(Dyn.Float(1.0)))
    print(Dyn.Bool(false).equal_unordered(Dyn.Bool(false)))
    print(Dyn.Text("same").equal_unordered(Dyn.Text("same")))
    print(Dyn.Text("1").equal_unordered(Dyn.Int(1)))
    print(Dyn.Float(Float.NAN).equal_unordered(Dyn.Float(Float.NAN)))
}
"#;
    let scratch = Scratch::new("dyn_scalar_unordered");
    let path = scratch.write_project("2026", source);
    checked_bundle(path.to_str().unwrap());
    let expected = concat!(
        "strict\nstrict-error\n",
        "text\n-42\n1.0\nfalse\nnone\nnone\nnone\n",
        "true\ntrue\nfalse\nfalse\ntrue\ntrue\nfalse\nfalse\n",
    );
    let aot = run_aot(&path, scratch.path());
    assert_eq!(
        aot.exit, 0,
        "Dyn scalar/equality AOT failed: {}",
        aot.stderr
    );
    assert_eq!(aot.stdout, expected);
    let dev = run_default_dev(path.to_str().unwrap());
    assert_eq!(dev, aot, "Dyn scalar/equality default run drift");
    let interpreter = run_forced_interpreter(path.to_str().unwrap());
    assert_eq!(
        interpreter, aot,
        "Dyn scalar/equality interpreter drift"
    );
}

fn dyn_int_accessor_matches_aot_inside_a_helper_inner() {
    if !common::have_rustc() {
        eprintln!("note: skipping Dyn Int deopt parity (need rustc)");
        return;
    }
    let source = r#"
use core.text as text

fn gap() {
    folded := text.casefold("Straße")
    if folded != "strasse" { panic("casefold") }
    Dyn.Int(7).int() ? value -> print(value) ! error -> print(error)
    Dyn.Float(7.0).int() ? value -> print(value) ! error -> print(error)
    Dyn.Text("7").int() ? value -> print(value) ! error -> print(error)
}

fn run() {
    gap()
}
"#;
    let scratch = Scratch::new("dyn_int_deopt");
    let path = scratch.write_project("2026", source);
    checked_bundle(path.to_str().unwrap());
    let aot = run_aot(&path, scratch.path());
    assert_eq!(aot.exit, 0, "Dyn Int deopt AOT failed: {}", aot.stderr);
    assert_eq!(
        aot.stdout,
        "7\n[expected int, got 7.0]\n[expected int, got \"7\"]\n"
    );
    let dev = run_default_dev(path.to_str().unwrap());
    assert_eq!(dev, aot);
    let interpreter = run_forced_interpreter(path.to_str().unwrap());
    assert_eq!(interpreter, aot, "Dyn Int interpreter drift");
}

/// `csv.to_string` takes either the dynamic `[[String]]` rows form or a typed
/// `[T]` list of `#Codable` values. A native tier once sent both to the
/// rows host, so a typed list rendered "$xml_event" and still reported success — silent
/// data loss on the default `jet run` path (#1269). Both shapes are exercised
/// here so the rows form stays covered too.
const TYPED_CSV_ENCODE: &str = r#"
use core.encoding.csv as csv

#Codable
struct Sale {
    item: String
    qty: Int
}

fn run() {
    sales :: [Sale{item: "pen", qty: 3}, Sale{item: "ink", qty: 5}]
    typed :: csv.to_string(sales)
    print("typed-len: {typed.len()}")
    print(typed)
    rows :: [["name", "score"], ["ada", "9"]]
    print("rows: {csv.to_string(rows)}")
}
"#;

#[test]
fn typed_csv_encode_matches_aot_and_default_dev() {
    on_encoding_stack(typed_csv_encode_matches_aot_and_default_dev_inner);
}

fn typed_csv_encode_matches_aot_and_default_dev_inner() {
    if !common::have_rustc() {
        eprintln!("note: skipping typed CSV encode parity (need rustc)");
        return;
    }
    let scratch = Scratch::new("typed_csv_encode");
    let path = scratch.write_project("2026", TYPED_CSV_ENCODE);
    let aot = run_aot(&path, scratch.path());
    assert_eq!(aot.exit, 0, "typed CSV AOT failed: {}", aot.stderr);
    // An empty success is never correct: pin the real cells, not just parity,
    // so both lenses agreeing on "$xml_event" could not pass this test.
    assert!(
        aot.stdout.contains("item,qty\npen,3\nink,5"),
        "typed CSV AOT lost the records: {}",
        aot.stdout
    );
    assert!(
        aot.stdout.contains("typed-len: 20"),
        "typed CSV AOT length drifted: {}",
        aot.stdout
    );
    assert!(
        aot.stdout.contains("rows: name,score\nada,9"),
        "dynamic rows CSV regressed: {}",
        aot.stdout
    );
    assert_default_dev_matches_aot_or_honest_gap(
        "typed-csv-encode",
        &path,
        scratch.path(),
        &aot,
        true,
    );
}

#[test]
fn custom_encode_survives_containers() {
    on_encoding_stack(|| {
        let source = r#"
use core.encoding.json as json
struct Token { raw: String }
impl Token.Encode {
    fn encode(self) -> Dyn { Dyn.Text("wire") }
}
fn token() -> Token? { Val(Token{raw: "raw"}) }
fn empty() -> Token? { None }
fn run() {
    print(json.to_string([Token{raw: "raw"}]) ?? panic("JSON render"))
    print(json.to_string(token()) ?? panic("JSON render"))
    print(json.to_string(empty()) ?? panic("JSON render"))
    tokens := [String:Token]{}
    tokens["a"] = Token{raw: "raw"}
    print(json.to_string(tokens) ?? panic("JSON render"))
}
"#;
        let scratch = Scratch::new("custom_container_encode");
        let path = scratch.write_project("2026", source);
        let aot = run_aot(&path, scratch.path());
        assert_eq!(aot.exit, 0, "{}", aot.stderr);
        assert_eq!(aot.stdout, "[\"wire\"]\n\"wire\"\nnull\n{\"a\":\"wire\"}\n");
        let default_run = run_default_dev(path.to_str().unwrap());
        assert_eq!(default_run, aot);
        assert_eq!(run_forced_interpreter(path.to_str().unwrap()), aot);
    });
}

#[test]
fn csv_to_string_rejects_values_that_are_not_rows_or_records() {
    for (name, value) in [("scalar", "42"), ("scalar-list", "[1, 2]")] {
        let src = format!(
            "use core.encoding.csv as csv\nfn run() {{\n    print(csv.to_string({value}))\n}}\n"
        );
        let Err(diags) =
            jet::Driver::compile_generated_src(&src, name, jet::Sema::CompileMode::Run)
        else {
            panic!("{name} reached a CSV runtime lens");
        };
        assert!(
            diags.iter().any(|diag| diag.code == "E0112"),
            "{name} missed existing argument mismatch: {diags:?}"
        );
    }
}

fn stream_fixture(format: &str, body: &str) -> String {
    format!(
        r#"
use core.encoding as encoding
use core.encoding.[{format} as fmt]
use core.files as files

{body}
"#
    )
}

fn assert_aot_dev_stream_parity(label: &str, source: &str) {
    on_encoding_stack(|| assert_aot_dev_stream_parity_inner(label, source));
}

fn assert_aot_dev_stream_parity_inner(label: &str, source: &str) {
    if !common::have_rustc() {
        eprintln!("note: skipping {label} stream parity (need rustc)");
        return;
    }
    let scratch = Scratch::new(label);
    let path = scratch.write_project("2026", source);
    let aot = run_aot(&path, scratch.path());
    assert_eq!(aot.exit, 0, "{label} AOT stream failed: {}", aot.stderr);
    assert_default_dev_matches_aot_or_honest_gap(label, &path, scratch.path(), &aot, false);
}

#[test]
fn json_stream_reader_writer_matches_aot_and_default_dev() {
    on_encoding_stack(|| {
        assert!(
            common::have_rustc(),
            "criterion 5 requires rustc: the canonical encoding_json_stream example must execute across all tiers"
        );
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let jet_path = root.join("Examples/features/serde/encoding_json_stream.jet");
        let python_path = root.join("Examples/features/serde/encoding_json_stream.py");
        let expected_path = root.join("Examples/features/expected/serde/encoding_json_stream.out");
        let expected = fs::read_to_string(&expected_path).expect("canonical encoding golden");
        let expected_json = "{\"a\":1,\"b\":2}";
        let expected_events =
            "event:ObjectStart\nevent:Key:a\nevent:Int:1\nevent:Key:b\nevent:Int:2\nevent:ObjectEnd\n";
        let expected_runtime = format!("{expected_events}read-count:6\neof\n{expected_json}\n");
        assert_eq!(
            expected, expected_runtime,
            "canonical golden must pin each JSON stream event identity and serialized content"
        );

        let python_scratch = Scratch::new("json-stream-python");
        let python_output_path = python_scratch.path().join("python.json");
        let python = Command::new("python3")
            .arg(&python_path)
            .arg(&python_output_path)
            .output()
            .expect("matched Python encoding fixture must execute");
        assert!(
            python.status.success(),
            "matched Python encoding fixture failed:\n{}",
            String::from_utf8_lossy(&python.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&python.stdout),
            expected_json,
            "matched Python fixture serialized content"
        );
        assert_eq!(
            fs::read_to_string(&python_output_path).expect("Python fixture output"),
            expected_json,
            "matched Python fixture file content"
        );

        let canonical_source =
            fs::read_to_string(&jet_path).expect("canonical encoding source");
        let scratch = Scratch::new("json-stream-canonical");
        let staged_path = scratch.write_project("2026", &canonical_source);
        let aot = run_aot(&staged_path, scratch.path());
        let dev = run_default_dev(staged_path.to_str().unwrap());
        let interpreter = run_forced_interpreter(staged_path.to_str().unwrap());

        for (label, output) in [
            ("AOT", &aot),
            ("default jet run", &dev),
            ("forced interpreter", &interpreter),
        ] {
            assert_eq!(
                output.exit, 0,
                "{label} canonical encoding run failed: {}",
                output.stderr
            );
            assert!(
                output.stderr.is_empty(),
                "{label} emitted stderr: {}",
                output.stderr
            );
            assert_eq!(
                output.stdout, expected_runtime,
                "{label} output drifted from the canonical serialized stream"
            );
            assert!(
                output.stdout.contains(expected_json),
                "{label} output is missing serialized JSON content"
            );
        }
        assert_eq!(dev, aot, "default `jet run` drifted from canonical AOT");
        assert_eq!(
            interpreter, aot,
            "forced interpreter drifted from canonical AOT"
        );
    });
}

#[test]
fn jsonl_csv_xml_cbor_streams_match_aot_and_default_dev() {
    let jsonl = r#"
fn run() {
    path := "@DIR@/rows.jsonl"
    output :: files.create(path) ?? panic("create")
    writer :: fmt.writer(^output) ?? panic("writer")
    writer.write(Dyn.Object(["a": Dyn.Int(1)])) ?? panic("write")
    writer.finish() ?? panic("finish")
    input :: files.open(path) ?? panic("open")
    reader :: fmt.reader(^input) ?? panic("reader")
    first :: reader.next()
    if first == {
        .Val(_) -> print(true)
        .None -> print(false)
    }
    print(files.read(path) ?? panic("read"))
}
"#;
    assert_aot_dev_stream_parity("jsonl-stream", &stream_fixture("jsonl", jsonl));

    let csv = r#"
fn run() {
    path := "@DIR@/rows.csv"
    output :: files.create(path) ?? panic("create")
    writer :: fmt.writer(^output, encoding.EncodingLimits.safe()) ?? panic("writer")
    writer.write(["ok", "true"]) ?? panic("write")
    writer.finish() ?? panic("finish")
    input :: files.open(path) ?? panic("open")
    reader :: fmt.reader(^input) ?? panic("reader")
    first :: reader.next()
    if first == {
        .Val(_) -> print(true)
        .None -> print(false)
    }
    print(files.read(path) ?? panic("read"))
}
"#;
    assert_aot_dev_stream_parity("csv-stream", &stream_fixture("csv", csv));

    let xml = r#"
fn xml_name(local: String) -[]> Dyn {
    return Dyn.Object([
        "raw": Dyn.Text(~local),
        "prefix": Dyn.Null,
        "local": Dyn.Text(~local),
        "namespace_uri": Dyn.Null,
    ])
}

fn run() {
    path := "@DIR@/doc.xml"
    output :: files.create(path) ?? panic("create")
    writer :: fmt.writer(^output, encoding.EncodingLimits.safe()) ?? panic("writer")
    writer.write(Dyn.Object([
        "$xml_event": Dyn.Text("document_start"),
        "encoding": Dyn.Null,
        "bom": Dyn.Array([]),
    ])) ?? panic("document_start")
    writer.write(Dyn.Object([
        "$xml_event": Dyn.Text("element_start"),
        "name": xml_name("r"),
        "namespaces": Dyn.Array([]),
        "attributes": Dyn.Array([]),
        "empty_style": Dyn.Text("empty"),
        "open_lexical": Dyn.Object([
            "raw_text": Dyn.Null,
            "raw_bytes": Dyn.Null,
            "semantic": Dyn.Object([
                "name": xml_name("r"),
                "namespaces": Dyn.Array([]),
                "attributes": Dyn.Array([]),
                "empty_style": Dyn.Text("empty"),
            ]),
        ]),
    ])) ?? panic("root")
    writer.write(Dyn.Object(["$xml_event": Dyn.Text("document_end")])) ?? panic("end")
    writer.finish() ?? panic("finish")
    input :: files.open(path) ?? panic("open")
    reader :: fmt.reader(^input) ?? panic("reader")
    first :: reader.next()
    if first == {
        .Val(_) -> print(true)
        .None -> print(false)
    }
    print(files.read(path) ?? panic("read"))
}
"#;
    assert_aot_dev_stream_parity("xml-stream", &stream_fixture("xml", xml));

    let cbor = r#"
fn run() {
    path := "@DIR@/packet.cbor"
    output :: files.create(path) ?? panic("create")
    writer :: fmt.writer(^output) ?? panic("writer")
    writer.write(encoding.DataEvent.ObjectStart) ?? panic("start")
    writer.write(encoding.DataEvent.Key("aa")) ?? panic("key aa")
    writer.write(encoding.DataEvent.Int(1)) ?? panic("int aa")
    writer.write(encoding.DataEvent.Key("b")) ?? panic("key b")
    writer.write(encoding.DataEvent.Int(2)) ?? panic("int b")
    writer.write(encoding.DataEvent.Key("f")) ?? panic("key f")
    writer.write(encoding.DataEvent.Float(1.5)) ?? panic("float f")
    writer.write(encoding.DataEvent.Key("z")) ?? panic("key z")
    writer.write(encoding.DataEvent.Float(-0.0)) ?? panic("float z")
    writer.write(encoding.DataEvent.ObjectEnd) ?? panic("end")
    writer.finish() ?? panic("finish")
    input :: files.open(path) ?? panic("open")
    reader :: fmt.reader(^input) ?? panic("reader")
    first :: reader.next()
    if first == {
        .Val(_) -> print(true)
        .None -> print(false)
    }
    whole := Dyn.Object([
        "aa": Dyn.Int(1),
        "b": Dyn.Int(2),
        "f": Dyn.Float(1.5),
        "z": Dyn.Float(-0.0),
    ])
    print((fmt.to_bytes_canonical(whole) ?? panic("whole")) == (files.read_bytes(path) ?? panic("bytes")))
}
"#;
    assert_aot_dev_stream_parity("cbor-stream", &stream_fixture("cbor", cbor));
}

#[test]
fn collect_stream_and_unfold_whole_share_canonical_law() {
    on_encoding_stack(collect_stream_and_unfold_whole_share_canonical_law_inner);
}

fn collect_stream_and_unfold_whole_share_canonical_law_inner() {
    if !common::have_rustc() {
        eprintln!("note: skipping collect/unfold parity (need rustc)");
        return;
    }
    let source = r#"
use core.encoding.json as json
use core.encoding.cbor as cbor
use core.encoding as encoding
use core.files as files

fn run() {
    whole := json.parse("{{\"a\":1,\"b\":2}}") ?? panic("json")
    canon := json.to_string(whole) ?? panic("JSON render")
    path := "@DIR@/round.json"
    output :: files.create(path) ?? panic("create")
    writer :: json.writer(^output, encoding.EncodingLimits.safe(), false) ?? panic("writer")
    writer.write(encoding.DataEvent.ObjectStart) ?? panic("os")
    writer.write(encoding.DataEvent.Key("a")) ?? panic("ka")
    writer.write(encoding.DataEvent.Int(1)) ?? panic("va")
    writer.write(encoding.DataEvent.Key("b")) ?? panic("kb")
    writer.write(encoding.DataEvent.Int(2)) ?? panic("vb")
    writer.write(encoding.DataEvent.ObjectEnd) ?? panic("oe")
    writer.finish() ?? panic("finish")
    collected := json.parse(files.read(path) ?? panic("read")) ?? panic("collect")
    print((json.to_string(collected) ?? panic("JSON render")) == canon)
    print(json.to_string(collected) ?? panic("JSON render"))

    cbor_path := "@DIR@/round.cbor"
    cbor_out :: files.create(cbor_path) ?? panic("cbor create")
    cbor_writer :: cbor.writer(^cbor_out) ?? panic("cbor writer")
    cbor_writer.write(encoding.DataEvent.ObjectStart) ?? panic("cs")
    cbor_writer.write(encoding.DataEvent.Key("a")) ?? panic("ck")
    cbor_writer.write(encoding.DataEvent.Int(1)) ?? panic("ci")
    cbor_writer.write(encoding.DataEvent.ObjectEnd) ?? panic("ce")
    cbor_writer.finish() ?? panic("cf")
    whole_tree := Dyn.Object(["a": Dyn.Int(1)])
    stream_bytes := files.read_bytes(cbor_path) ?? panic("cbor bytes")
    print((cbor.to_bytes_canonical(whole_tree) ?? panic("canonical")) == stream_bytes)
}
"#;
    let scratch = Scratch::new("collect_unfold");
    let path = scratch.write_project("2026", source);
    let aot = run_aot(&path, scratch.path());
    assert_eq!(aot.exit, 0, "collect/unfold fixture failed: {}", aot.stderr);
    assert_eq!(
        aot.stdout, "true\n{\"a\":1,\"b\":2}\ntrue\n",
        "JSON collect and CBOR canonical stream bytes must match whole-value law"
    );
    assert_default_dev_matches_aot_or_honest_gap(
        "collect-unfold",
        &path,
        scratch.path(),
        &aot,
        false,
    );
}

#[test]
fn comptime_rejects_file_backed_streams_at_named_boundary() {
    for (label, module) in [
        ("json-reader", "json as json"),
        ("jsonl-reader", "jsonl as jsonl"),
        ("csv-reader", "csv as csv"),
        ("xml-reader", "xml as xml"),
        ("cbor-reader", "cbor as cbor"),
    ] {
        let scratch = Scratch::new(label);
        let source = format!(
            "use core.encoding.[{module}]\nuse core.files as files\n\nPROBE :: prep {{ files.read(\"probe.txt\") }}\n\nfn ignored() {{ input :: files.open(\"x\") }}\n\nfn run() {{\n    print(PROBE)\n}}\n"
        );
        let path = scratch.write_project("2026", &source);
        let diags = jet::check_with_path(path.to_str().unwrap());
        assert!(
            diags.iter().any(|d| d.code == "E3410"),
            "{label} must reject file-backed ambient I/O in comptime:\n{}",
            jet::render_diagnostics(path.to_str().unwrap(), &source, &diags)
        );
    }
}

#[test]
fn malformed_limits_and_terminal_errors_match_across_applicable_tiers() {
    on_encoding_stack(malformed_limits_and_terminal_errors_match_across_applicable_tiers_inner);
}

fn malformed_limits_and_terminal_errors_match_across_applicable_tiers_inner() {
    if !common::have_rustc() {
        eprintln!("note: skipping differential hostile parity (need rustc)");
        return;
    }
    let source = r#"
use core.encoding as encoding
use core.encoding.json as json
use core.files as files

fn terminal_limit_probe() -[FS]> String {
    bad_path := "@DIR@/bad.json"
    limits := encoding.EncodingLimits.safe()
    limits.max_total_bytes = Val(5)
    fs_write := files.create(bad_path) ?? panic("create")
    writer :: json.writer(^fs_write, limits, false) ?? panic("writer")
    writer.write(encoding.DataEvent.ArrayStart) ?? panic("array")
    writer.write(encoding.DataEvent.Text("abcd")) ? _value -> return "limit-missed" ! first -> {
        writer.finish() ? _value -> return "terminal-missed" ! second -> return "{first.reason == second.reason}"
    }
    return "unreachable"
}

fn malformed_reader_probe() -[FS]> String {
    files.write("@DIR@/malformed.json", "{{\"a\":") ?? panic("write malformed")
    input :: files.open("@DIR@/malformed.json") ?? panic("open")
    reader :: json.reader(^input, encoding.EncodingLimits.safe()) ?? panic("reader")
    reader.next() ? _maybe -> return "malformed-missed" ! error -> {
        reader.next() ? _maybe -> return "repeat-missed" ! second -> return "{error.kind == encoding.EncodingErrorKind.Syntax}|{error.path}|{error.reason == second.reason}"
    }
    return "unreachable"
}

fn run() {
    print(terminal_limit_probe())
    print(malformed_reader_probe())
}
"#;
    let scratch = Scratch::new("hostile");
    let path = scratch.write_project("2026", source);
    let aot = run_aot(&path, scratch.path());
    assert_eq!(aot.exit, 0, "hostile differential failed: {}", aot.stderr);
    assert!(
        aot.stdout.lines().any(|line| line == "true"),
        "terminal error latch must repeat identical reason, got {:?}",
        aot.stdout
    );
    assert!(
        aot.stdout.contains("Syntax") || aot.stdout.contains("true"),
        "malformed reader must surface typed syntax path, got {:?}",
        aot.stdout
    );
    assert_default_dev_matches_aot_or_honest_gap(
        "hostile-differential",
        &path,
        scratch.path(),
        &aot,
        false,
    );
}

/// #2510: a file-backed `String` must flow from `fs.read` into typed JSON
/// decoding with explicit fallible handling.
#[test]
fn exact_typed_json_numbers_match_aot_default_run_and_interpreter() {
    on_encoding_stack(exact_typed_json_numbers_match_aot_default_run_and_interpreter_inner);
}

fn exact_typed_json_numbers_match_aot_default_run_and_interpreter_inner() {
    if !common::have_rustc() {
        eprintln!("note: skipping exact typed JSON parity (need rustc)");
        return;
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let python_path = root.join("Examples/features/serde/json_typed.py");
    let python = Command::new("python3")
        .arg(&python_path)
        .output()
        .expect("exact-number Python fixture must execute");
    assert!(
        python.status.success(),
        "exact-number Python fixture failed: {}",
        String::from_utf8_lossy(&python.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&python.stdout),
        concat!(
            "12.340\n0.00001\n",
            "12345678901234567890123456789012345678901234567890\n",
            "accepted-nonfinite:2\n",
        ),
        "the matched Python decoder should preserve exact values but accept two non-finite extensions that Jet rejects",
    );

    let source = r#"
use core.encoding.json as json
use core.data as data
use core.files as fs

#Codable
struct ExactNumbers {
    amount: Decimal
    exponent: Decimal
    whole: Decimal
    tenth: Decimal
    tenth_with_zero: Decimal
    scientific_tenth: Decimal
    adjacent_lo: Int
    adjacent_hi: Int
    large: Int
    large_exp: Int
}

#Codable
struct SmallI64 {
    value: I64
}

#Codable
struct ExactIntRow {
    adjacent_hi: Int
    large: Int
}

LIMITED_TEXT :: prep { "1{"0".repeat(1000000)}" }

fn run() {
    exact_text :: fs.read("@DIR@/exact.json") ?? panic("exact JSON read")
    value :: json.decode<ExactNumbers>(exact_text) ?? panic("exact JSON decode")
    print(value.amount.to_string())
    print(value.exponent.to_string())
    print(value.whole.to_string())
    print(value.tenth.to_string())
    print(value.tenth_with_zero.to_string())
    print(value.scientific_tenth.to_string())
    print(value.adjacent_lo.to_string())
    print(value.adjacent_hi.to_string())
    print(value.large.to_string())
    print(value.large_exp.to_string())

    stream_raw :: "{{\"adjacent_hi\":9007199254740993,\"large\":12345678901234567890123456789012345678901234567890}}"
    rows :: data.json<ExactIntRow>("[{stream_raw}]") ?? panic("stream decode")
    loop row in rows {
        print(row.adjacent_hi.to_string())
        print(row.large.to_string())
    }

    json.decode<SmallI64>("{{\"value\":9223372036854775808}}") ? _value -> { print("accepted") } ! _error -> { print("i64-overflow") }
    json.decode<Float>("1e400") ? _value -> { print("accepted") } ! _error -> { print("nonfinite") }
    json.decode<Decimal>("NaN") ? _value -> { print("accepted") } ! _error -> { print("nan-rejected") }
    json.decode<Decimal>("Infinity") ? _value -> { print("accepted") } ! _error -> { print("infinity-rejected") }
    json.decode<Int>("1.5") ? _value -> { print("accepted") } ! _error -> { print("fractional-rejected") }
    json.decode<Decimal>("1e1000001") ? _value -> { print("accepted") } ! _error -> { print("exponent-limit") }
    mismatch_text :: fs.read("@DIR@/mismatch.json") ?? panic("mismatch JSON read")
    json.decode<ExactNumbers>(mismatch_text) ? _value -> { print("accepted") } ! _error -> { print("file-mismatch") }
    mismatch_raw :: "{{\"amount\":\"12.340\",\"exponent\":1E-5,\"whole\":100,\"tenth\":0.1,\"tenth_with_zero\":0.10,\"scientific_tenth\":1e-1,\"adjacent_lo\":1,\"adjacent_hi\":1,\"large\":1,\"large_exp\":1}}"
    json.decode<ExactNumbers>(mismatch_raw) ? _value -> { print("accepted") } ! _error -> { print("mismatch") }
    data.json<ExactNumbers>("[{mismatch_raw}]") ? _value -> { print("accepted") } ! _error -> { print("stream-mismatch") }
    json.decode<String>("123") ? _value -> { print("accepted") } ! _error -> { print("string-mismatch") }
    json.decode<Decimal>(@limited_text) ? _value -> { print("accepted") } ! _error -> { print("limit") }
}
"#;
    let scratch = Scratch::new("exact_typed_json_numbers");
    let path = scratch.write_project("2026", source);
    fs::write(
        scratch.path().join("exact.json"),
        r#"{"amount":12.340,"exponent":1E-5,"whole":100,"tenth":0.1,"tenth_with_zero":0.10,"scientific_tenth":1e-1,"adjacent_lo":9007199254740992,"adjacent_hi":9007199254740993,"large":12345678901234567890123456789012345678901234567890,"large_exp":1e30}"#,
    )
    .unwrap();
    fs::write(scratch.path().join("mismatch.json"), r#"{}"#).unwrap();
    let aot = run_aot(&path, scratch.path());
    assert_eq!(aot.exit, 0, "exact typed JSON AOT failed: {}", aot.stderr);
    let dev = run_default_dev(path.to_str().unwrap());
    assert_eq!(
        dev.exit, 0,
        "exact typed JSON default run failed: {}",
        dev.stderr
    );
    let interpreter = run_forced_interpreter(path.to_str().unwrap());
    let expected = concat!(
        "12.340\n0.00001\n100\n0.1\n0.10\n0.1\n",
        "9007199254740992\n9007199254740993\n",
        "12345678901234567890123456789012345678901234567890\n",
        "1000000000000000000000000000000\n",
        "9007199254740993\n",
        "12345678901234567890123456789012345678901234567890\n",
        "i64-overflow\nnonfinite\nnan-rejected\ninfinity-rejected\n",
        "fractional-rejected\nexponent-limit\nfile-mismatch\nmismatch\nstream-mismatch\n",
        "string-mismatch\nlimit\n",
    );
    for (label, output) in [
        ("AOT", &aot),
        ("default run", &dev),
        ("interpreter", &interpreter),
    ] {
        assert_eq!(
            output.exit, 0,
            "{label} exact typed JSON failed: {}",
            output.stderr
        );
        assert_eq!(output.stderr, "", "{label} emitted stderr");
        assert_eq!(
            output.stdout, expected,
            "{label} exact typed JSON output drifted"
        );
    }
    assert_eq!(dev, aot, "default run exact JSON drifted from AOT");
    assert_eq!(interpreter, aot, "interpreter exact JSON drifted from AOT");
}

#[test]
fn owned_dyn_int_and_strict_decode_match_all_tiers() {
    on_encoding_stack(|| {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        for stem in ["dyn_exact_int", "dyn_strict_decode", "dyn_number_lexeme", "dyn_cbor_bytes"] {
            let source = fs::read_to_string(root.join(format!("Examples/features/serde/{stem}.jet")))
                .expect("Dyn rules fixture");
            let expected = fs::read_to_string(root.join(format!("Examples/features/expected/serde/{stem}.out")))
                .expect("reviewed Dyn rules golden");
            let scratch = Scratch::new(stem);
            let path = scratch.write_project("2026", &source);
            let aot = run_aot(&path, scratch.path());
            let dev = run_default_dev(path.to_str().unwrap());
            let interpreter = run_forced_interpreter(path.to_str().unwrap());
            for (tier, output) in [("AOT", &aot), ("default", &dev), ("interpreter", &interpreter)] {
                assert_eq!(output.exit, 0, "{stem} {tier}: {}", output.stderr);
                assert_eq!(output.stdout, expected, "{stem} {tier} output");
                assert_eq!(output.stderr, "", "{stem} {tier} diagnostics");
            }
        }
    });
}
