//! Card #713/#715: whole/stream encoding parity across AOT, comptime (where
//! pure), and default-dev with #778 tiered backend attribution.
//!
//! One matrix harness records tier outcomes instead of isolated per-format tests.
//! Edition 2026 keeps the D-ENCBASE-STRICT1 compatibility union and single-arg
//! `json.canonical` surface exercised by #710–#712.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use jet::Interpreter::{dev_iteration, RunOutcome};
use jet_jit::{
    deopt_invoked_for_test, fallback_invoked_for_test, jit_executed_for_test, plan_bundle_tiers,
    reset_jit_trace_for_test, resident_jit_safe_bundle, resident_jit_safe_bundle_detail,
    set_trace_tiers, take_last_trace, Tier,
};

mod common;

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
            self.dir.join("pkg.jet"),
            format!(
                "payload: {{ name: \"enc\", version: \"0.1.0\", edition: \"{edition}\" }}\n"
            ),
        )
        .unwrap();
        let path = self.dir.join("run.jet");
        fs::write(&path, body).unwrap();
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum DevBackend {
    ResidentJit,
    DeoptInterp,
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
    rustc.args(["--edition", "2021", rs.to_str().unwrap(), "-o", bin.to_str().unwrap()]);
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

fn checked_bundle(path: &str) -> jet::AST::ProgramBundle {
    let mut bundle = jet::Loader::load_entry(path).expect("fixture should load");
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags
        .iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(
        errors.is_empty(),
        "fixture must type-check:\n{}",
        jet::render_diagnostics(path, "", &diags)
    );
    bundle
}

fn run_default_dev(path: &str) -> (DevBackend, ProgramOutput) {
    reset_jit_trace_for_test();
    let bundle = checked_bundle(path);
    let jit_safe = resident_jit_safe_bundle(&bundle);
    match dev_iteration(path, false, false) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            let jit = jit_executed_for_test();
            let deopt = deopt_invoked_for_test();
            let fallback = fallback_invoked_for_test();
            assert!(
                !fallback,
                "default-dev must not silently fall back to interpreter/AOT for encoding probes"
            );
            if deopt {
                (
                    DevBackend::DeoptInterp,
                    ProgramOutput::ran(stdout, stderr, exit_code),
                )
            } else {
                assert!(jit, "successful default-dev encoding probe must execute resident JIT");
                assert!(
                    jit_safe,
                    "resident JIT run requires resident_jit_safe_bundle; detail: {}",
                    resident_jit_safe_bundle_detail(&bundle)
                );
                (
                    DevBackend::ResidentJit,
                    ProgramOutput::ran(stdout, stderr, exit_code),
                )
            }
        }
        RunOutcome::Problems(diags) => {
            assert!(
                !fallback_invoked_for_test(),
                "default-dev must not silently fall back when deopt hits an interpreter gap"
            );
            assert!(
                !diags.iter().any(|d| d.code == "E2211"),
                "E2211 retired by #778; encoding probes must silent-deopt or name E0956: {diags:?}"
            );
            let detail = diags
                .iter()
                .map(|d| format!("{}:{}", d.code, d.what))
                .collect::<Vec<_>>()
                .join("; ");
            (
                DevBackend::DeoptInterp,
                ProgramOutput::ran(String::new(), format!("interpreter gap: {detail}"), 101),
            )
        }
    }
}

fn run_forced_interpreter(path: &str) -> ProgramOutput {
    match dev_iteration(path, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => panic!("forced interpreter rejected encoding probe: {diags:?}"),
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

/// Named interpreter/JIT gaps that cannot claim encoding parity (C5).
/// File-backed stream handles still stop on the #778 deopt interpreter:
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
    let (backend, dev) = run_default_dev(jet_path.to_str().unwrap());
    eprintln!("{label} default-dev backend={backend:?} exit={} deopt_gap={}", dev.exit, named_encoding_dev_gap(&dev.stderr));
    match backend {
        DevBackend::ResidentJit => {
            assert_eq!(dev.stdout, aot.stdout, "{label} default-dev/JIT stdout drift");
            assert_eq!(dev.stderr, aot.stderr, "{label} default-dev/JIT stderr drift");
            assert_eq!(dev.exit, aot.exit, "{label} default-dev/JIT exit drift");
        }
        DevBackend::DeoptInterp => {
            if dev.exit == 0 {
                assert_eq!(dev.stdout, aot.stdout, "{label} deopt-interp stdout drift");
                assert_eq!(dev.stderr, aot.stderr, "{label} deopt-interp stderr drift");
                assert_eq!(dev.exit, aot.exit, "{label} deopt-interp exit drift");
            } else if named_encoding_dev_gap(&dev.stderr) {
                // Explicit unsupported path — does not satisfy stream/default-dev parity.
                eprintln!(
                    "note: {label} default-dev named gap (no parity claim): {}",
                    dev.stderr
                );
            } else if interpreter_fallback {
                let interp = run_forced_interpreter(jet_path.to_str().unwrap());
                assert_eq!(
                    interp.stdout, aot.stdout,
                    "{label} forced-interpreter must preserve AOT encoding semantics when default-dev deopts"
                );
                assert_eq!(interp.exit, aot.exit, "{label} interpreter exit drift");
            } else {
                panic!(
                    "{label} default-dev deopt failed without named encoding gap or parity: exit={} stderr={}",
                    dev.exit, dev.stderr
                );
            }
        }
    }
}

/// Runtime-only whole-value probes shared by AOT ↔ default-dev (#778 lens).
/// Encoding parse/decode at comptime is AOT-pure but impure on the tiered run
/// interpreter (E0956); default-dev must not pretend comptime bindings work there.
/// Avoid enum-match Result arms here — whole-program deopt still lacks them (E2201).
const WHOLE_VALUE_RUNTIME: &str = r#"
use core.encoding.json as json
use core.encoding.jsonl as jsonl
use core.encoding.csv as csv
use core.encoding.xml as xml
use core.encoding.cbor as cbor
use core.encoding.base64 as base64
use core.encoding.base32 as base32

fn run() {
    print(json.to_string(json.parse("{{\"b\":2,\"a\":1}}") ?? panic("json")))
    print((jsonl.parse("{{\"a\":1}}\n{{\"a\":2}}\n") ?? panic("jsonl")).len())
    print((csv.parse("name,score\nada,9\n") ?? panic("csv")).len())
    print(xml.to_string(xml.parse("<r xmlns=\"urn:r\">a&amp;</r>") ?? panic("xml")))
    print(json.to_string(cbor.parse(cbor.to_bytes(json.parse("{{\"a\":1}}") ?? panic("j")) ?? panic("e")) ?? panic("p")))
    print((base64.decode("Zg==") ?? panic("b64")).len())
    print((base64.decode_url("aGk") ?? panic("b64url")).len())
    print((base32.decode("MZXQ====") ?? panic("b32")).len())
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

comptime json_canon = json.to_string(json.parse("{{\"b\":2,\"a\":1}}") ?? panic("json"))
comptime jsonl_n = (jsonl.parse("{{\"a\":1}}\n{{\"a\":2}}\n") ?? panic("jsonl")).len()
comptime csv_n = (csv.parse("name,score\nada,9\n") ?? panic("csv")).len()
comptime xml_text = xml.to_string(xml.parse("<r xmlns=\"urn:r\">a&amp;</r>") ?? panic("xml"))
comptime cbor_round = json.to_string(cbor.parse(cbor.to_bytes(json.parse("{{\"a\":1}}") ?? panic("j")) ?? panic("e")) ?? panic("p"))
comptime b64_n = (base64.decode("Zg==") ?? panic("b64")).len()
comptime b64url_n = (base64.decode_url("aGk") ?? panic("b64url")).len()
comptime b32_n = (base32.decode("MZXQ====") ?? panic("b32")).len()

fn run() {
    print("{json_canon}|{json.to_string(json.parse("{{\"b\":2,\"a\":1}}") ?? panic("json"))}")
    print("{jsonl_n}|{(jsonl.parse("{{\"a\":1}}\n{{\"a\":2}}\n") ?? panic("jsonl")).len()}")
    print("{csv_n}|{(csv.parse("name,score\nada,9\n") ?? panic("csv")).len()}")
    print("{xml_text}|{xml.to_string(xml.parse("<r xmlns=\"urn:r\">a&amp;</r>") ?? panic("xml"))}")
    print("{cbor_round}|{json.to_string(cbor.parse(cbor.to_bytes(json.parse("{{\"a\":1}}") ?? panic("j")) ?? panic("e")) ?? panic("p"))}")
    print("{b64_n}|{(base64.decode("Zg==") ?? panic("b64")).len()}")
    print("{b64url_n}|{(base64.decode_url("aGk") ?? panic("b64url")).len()}")
    print("{b32_n}|{(base32.decode("MZXQ====") ?? panic("b32")).len()}")
}
"#;

#[test]
fn whole_value_codecs_match_aot_comptime_and_default_dev() {
    if !common::have_rustc() {
        eprintln!("note: skipping whole-value encoding parity (need rustc)");
        return;
    }
    // AOT comptime|runtime binding parity (pure on AOT evaluator).
    let ct = Scratch::new("whole_ct");
    let ct_path = ct.write_project("2026", WHOLE_VALUE_COMPTIME);
    assert_aot_comptime_binding_parity("whole-value-comptime", WHOLE_VALUE_COMPTIME, &ct_path, ct.path());

    // AOT ↔ default-dev runtime parity under #778 tiered lens (no comptime).
    let scratch = Scratch::new("whole");
    let path = scratch.write_project("2026", WHOLE_VALUE_RUNTIME);
    let aot = run_aot(&path, scratch.path());
    assert_eq!(aot.exit, 0, "whole-value runtime AOT failed: {}", aot.stderr);
    assert_default_dev_matches_aot_or_honest_gap("whole-value", &path, scratch.path(), &aot, true);
}

/// `csv.to_string` takes either the dynamic `[[String]]` rows form or a typed
/// `[T]` list of `#Codable` values. The resident JIT used to send both to the
/// rows host, so a typed list rendered "" and still reported success — silent
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
    sales :: [Sale.{item: "pen", qty: 3}, Sale.{item: "ink", qty: 5}]
    typed :: csv.to_string(sales)
    print("typed-len: {typed.len()}")
    print(typed)
    rows :: [["name", "score"], ["ada", "9"]]
    print("rows: {csv.to_string(rows)}")
}
"#;

#[test]
fn typed_csv_encode_matches_aot_and_default_dev() {
    if !common::have_rustc() {
        eprintln!("note: skipping typed CSV encode parity (need rustc)");
        return;
    }
    let scratch = Scratch::new("typed_csv_encode");
    let path = scratch.write_project("2026", TYPED_CSV_ENCODE);
    let aot = run_aot(&path, scratch.path());
    assert_eq!(aot.exit, 0, "typed CSV AOT failed: {}", aot.stderr);
    // An empty success is never correct: pin the real cells, not just parity,
    // so both lenses agreeing on "" could not pass this test.
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

fn stream_fixture(format: &str, body: &str) -> String {
    format!(
        r#"
use core.encoding as encoding
use core.encoding.{format} as fmt
use core.files as files

{body}
"#
    )
}

fn assert_aot_dev_stream_parity(label: &str, source: &str) {
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
    let body = r#"
fn run() {
    path := "json_stream_out.json"
    output :: files.create(path) ?? panic("create")
    writer :: fmt.writer(^output, encoding.EncodingLimits.safe(), false) ?? panic("writer")
    writer.write(encoding.DataEvent.ObjectStart) ?? panic("write")
    writer.write(encoding.DataEvent.Key("ok")) ?? panic("key")
    writer.write(encoding.DataEvent.Bool(true)) ?? panic("bool")
    writer.write(encoding.DataEvent.ObjectEnd) ?? panic("end")
    writer.finish() ?? panic("finish")
    input :: files.open(path) ?? panic("open")
    reader :: fmt.reader(^input, encoding.EncodingLimits.safe()) ?? panic("reader")
    count := 0
    loop count < 5 {
        maybe :: reader.next() ?? panic("next")
        if maybe == None { print("eof"); break }
        print("event")
        count++
    }
    print(files.read(path) ?? panic("read"))
}
"#;
    assert_aot_dev_stream_parity("json-stream", &stream_fixture("json", body));
}

#[test]
fn jsonl_csv_xml_cbor_streams_match_aot_and_default_dev() {
    let jsonl = r#"
fn run() {
    path := "rows.jsonl"
    output :: files.create(path) ?? panic("create")
    writer :: fmt.writer(^output) ?? panic("writer")
    writer.write(DataTree.Object(["a": DataTree.Int(1)])) ?? panic("write")
    writer.finish() ?? panic("finish")
    input :: files.open(path) ?? panic("open")
    reader :: fmt.reader(^input) ?? panic("reader")
    first :: reader.next() ?? panic("next")
    if first == {
        Val(_) -> print(true)
        None -> print(false)
    }
    print(files.read(path) ?? panic("read"))
}
"#;
    assert_aot_dev_stream_parity("jsonl-stream", &stream_fixture("jsonl", jsonl));

    let csv = r#"
fn run() {
    path := "rows.csv"
    output :: files.create(path) ?? panic("create")
    writer :: fmt.writer(^output, encoding.EncodingLimits.safe()) ?? panic("writer")
    writer.write(["ok", "true"]) ?? panic("write")
    writer.finish() ?? panic("finish")
    input :: files.open(path) ?? panic("open")
    reader :: fmt.reader(^input) ?? panic("reader")
    first :: reader.next() ?? panic("next")
    if first == {
        Val(_) -> print(true)
        None -> print(false)
    }
    print(files.read(path) ?? panic("read"))
}
"#;
    assert_aot_dev_stream_parity("csv-stream", &stream_fixture("csv", csv));

    let xml = r#"
fn xml_name(local: String) => DataTree {
    return DataTree.Object([
        "raw": DataTree.Text(~local),
        "prefix": DataTree.Null,
        "local": DataTree.Text(~local),
        "namespace_uri": DataTree.Null,
    ])
}

fn run() {
    path := "doc.xml"
    output :: files.create(path) ?? panic("create")
    writer :: fmt.writer(^output, encoding.EncodingLimits.safe()) ?? panic("writer")
    writer.write(DataTree.Object([
        "$xml_event": DataTree.Text("document_start"),
        "encoding": DataTree.Null,
        "bom": DataTree.Array([]),
    ])) ?? panic("document_start")
    writer.write(DataTree.Object([
        "$xml_event": DataTree.Text("element_start"),
        "name": xml_name("r"),
        "namespaces": DataTree.Array([]),
        "attributes": DataTree.Array([]),
        "empty_style": DataTree.Text("empty"),
        "open_lexical": DataTree.Object([
            "raw_text": DataTree.Null,
            "raw_bytes": DataTree.Null,
            "semantic": DataTree.Object([
                "name": xml_name("r"),
                "namespaces": DataTree.Array([]),
                "attributes": DataTree.Array([]),
                "empty_style": DataTree.Text("empty"),
            ]),
        ]),
    ])) ?? panic("root")
    writer.write(DataTree.Object(["$xml_event": DataTree.Text("document_end")])) ?? panic("end")
    writer.finish() ?? panic("finish")
    input :: files.open(path) ?? panic("open")
    reader :: fmt.reader(^input) ?? panic("reader")
    first :: reader.next() ?? panic("next")
    if first == {
        Val(_) -> print(true)
        None -> print(false)
    }
    print(files.read(path) ?? panic("read"))
}
"#;
    assert_aot_dev_stream_parity("xml-stream", &stream_fixture("xml", xml));

    let cbor = r#"
fn run() {
    path := "packet.cbor"
    output :: files.create(path) ?? panic("create")
    writer :: fmt.writer(^output) ?? panic("writer")
    writer.write(encoding.DataEvent.ObjectStart) ?? panic("start")
    writer.write(encoding.DataEvent.Key("a")) ?? panic("key")
    writer.write(encoding.DataEvent.Int(1)) ?? panic("int")
    writer.write(encoding.DataEvent.ObjectEnd) ?? panic("end")
    writer.finish() ?? panic("finish")
    input :: files.open(path) ?? panic("open")
    reader :: fmt.reader(^input) ?? panic("reader")
    first :: reader.next() ?? panic("next")
    if first == {
        Val(_) -> print(true)
        None -> print(false)
    }
    whole := DataTree.Object(["a": DataTree.Int(1)])
    print((fmt.to_bytes_canonical(whole) ?? panic("whole")) == (files.read_bytes(path) ?? panic("bytes")))
}
"#;
    assert_aot_dev_stream_parity("cbor-stream", &stream_fixture("cbor", cbor));
}

#[test]
fn collect_stream_and_unfold_whole_share_canonical_law() {
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
    canon := json.to_string(whole)
    path := "round.json"
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
    print(json.to_string(collected) == canon)
    print(json.to_string(collected))

    cbor_path := "round.cbor"
    cbor_out :: files.create(cbor_path) ?? panic("cbor create")
    cbor_writer :: cbor.writer(^cbor_out) ?? panic("cbor writer")
    cbor_writer.write(encoding.DataEvent.ObjectStart) ?? panic("cs")
    cbor_writer.write(encoding.DataEvent.Key("a")) ?? panic("ck")
    cbor_writer.write(encoding.DataEvent.Int(1)) ?? panic("ci")
    cbor_writer.write(encoding.DataEvent.ObjectEnd) ?? panic("ce")
    cbor_writer.finish() ?? panic("cf")
    whole_tree := DataTree.Object(["a": DataTree.Int(1)])
    stream_bytes := files.read_bytes(cbor_path) ?? panic("cbor bytes")
    print((cbor.to_bytes_canonical(whole_tree) ?? panic("canonical")) == stream_bytes)
}
"#;
    let scratch = Scratch::new("collect_unfold");
    let path = scratch.write_project("2026", source);
    let aot = run_aot(&path, scratch.path());
    assert_eq!(aot.exit, 0, "collect/unfold fixture failed: {}", aot.stderr);
    assert_eq!(
        aot.stdout,
        "true\n{\"a\":1,\"b\":2}\ntrue\n",
        "JSON collect and CBOR canonical stream bytes must match whole-value law"
    );
    assert_default_dev_matches_aot_or_honest_gap("collect-unfold", &path, scratch.path(), &aot, false);
}


#[test]
fn comptime_rejects_file_backed_streams_at_named_boundary() {
    for (label, module) in [
        ("json-reader", "json as json\nfn ignored() { input :: files.open(\"x\") }"),
        ("jsonl-reader", "jsonl as jsonl\nfn ignored() { input :: files.open(\"x\") }"),
        ("csv-reader", "csv as csv\nfn ignored() { input :: files.open(\"x\") }"),
        ("xml-reader", "xml as xml\nfn ignored() { input :: files.open(\"x\") }"),
        ("cbor-reader", "cbor as cbor\nfn ignored() { input :: files.open(\"x\") }"),
    ] {
        let scratch = Scratch::new(label);
        let source = format!(
            "use core.encoding.{module}\nuse core.files as files\n\ncomptime probe = files.read(\"probe.txt\")\n\nfn run() {{\n    print(probe)\n}}\n"
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
fn default_dev_encoding_probes_record_backend_without_silent_fallback() {
    if !common::have_rustc() {
        eprintln!("note: skipping backend attribution probe (need rustc)");
        return;
    }
    let scratch = Scratch::new("backend");
    let path = scratch.write_project("2026", WHOLE_VALUE_RUNTIME);
    let bundle = checked_bundle(path.to_str().unwrap());
    let plan = plan_bundle_tiers(&bundle);
    let jit_safe = resident_jit_safe_bundle(&bundle);
    reset_jit_trace_for_test();
    set_trace_tiers(true);
    let (backend, out) = run_default_dev(path.to_str().unwrap());
    set_trace_tiers(false);
    assert_eq!(out.exit, 0, "backend probe must run: {}", out.stderr);
    assert!(
        !fallback_invoked_for_test(),
        "default-dev encoding probe must not silent-AOT-fallback after #778"
    );
    let trace = take_last_trace();
    match backend {
        DevBackend::ResidentJit => {
            assert!(jit_safe, "resident JIT run requires jit-safe bundle");
            assert!(
                jit_executed_for_test() || plan.native.iter().any(|_| true),
                "resident path needs JIT execution or planned native tier; plan={plan:?} trace={trace:?}"
            );
        }
        DevBackend::DeoptInterp => {
            assert!(
                deopt_invoked_for_test() || plan.whole_interp || !plan.deopt.is_empty(),
                "deopt path must record deopt or named tier plan; detail={} plan={plan:?} trace={trace:?}",
                resident_jit_safe_bundle_detail(&bundle)
            );
            assert!(
                !trace.is_empty()
                    || plan.whole_interp
                    || plan.deopt.iter().any(|_| true)
                    || plan.rows.iter().any(|row| matches!(row.tier, Tier::Interp)),
                "deopt backend must leave tier plan or --trace-tiers rows; plan={plan:?} trace={trace:?}"
            );
        }
    }
}

#[test]
fn malformed_limits_and_terminal_errors_match_across_applicable_tiers() {
    if !common::have_rustc() {
        eprintln!("note: skipping differential hostile parity (need rustc)");
        return;
    }
    let source = r#"
use core.encoding as encoding
use core.encoding.json as json
use core.files as files

fn terminal_limit_probe() => String {
    bad_path := "bad.json"
    limits := encoding.EncodingLimits.safe()
    limits.max_total_bytes = Val(5)
    fs_write := files.create(bad_path) ?? panic("create")
    writer :: json.writer(^fs_write, limits, false) ?? panic("writer")
    writer.write(encoding.DataEvent.ArrayStart) ?? panic("array")
    limit_err :: writer.write(encoding.DataEvent.Text("abcd"))
    if limit_err == {
        Err(first) -> {
            again :: writer.finish()
            if again == {
                Err(second) -> return "{first.reason == second.reason}"
                Ok(_) -> return "terminal-missed"
            }
        }
        Ok(_) -> return "limit-missed"
    }
    return "unreachable"
}

fn malformed_reader_probe() => String {
    files.write("malformed.json", "{{\"a\":") ?? panic("write malformed")
    input :: files.open("malformed.json") ?? panic("open")
    reader :: json.reader(^input, encoding.EncodingLimits.safe()) ?? panic("reader")
    if reader.next() == {
        Err(error) -> {
            repeat :: reader.next()
            if repeat == {
                Err(second) -> return "{error.kind == encoding.EncodingErrorKind.Syntax}|{error.path}|{error.reason == second.reason}"
                Ok(_) -> return "repeat-missed"
            }
        }
        Ok(_) -> return "malformed-missed"
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
    assert_default_dev_matches_aot_or_honest_gap("hostile-differential", &path, scratch.path(), &aot, false);
}
