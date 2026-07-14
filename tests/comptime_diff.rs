//! M9.5 differential battery (permanent CI). For each expression, the same
//! code is evaluated twice — once as `comptime C = e;` (the sema
//! tree-walking interpreter) and once as a runtime `r :: e` (generated
//! Rust). The program prints both; the two lines MUST be byte-identical.
//!
//! Divergence is a P0 miscompile-class bug (S26 rule 6: comptime implements
//! runtime semantics exactly — i64 Int, IEEE f64 Float with S21 display,
//! char-counted Strings (S41), BTreeMap ordering (S38)).

use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

mod common;
use common::{have_rustc, panic_message, strip_vetted_module, test_worker_count};

/// Expressions whose comptime and runtime evaluation must agree. Each is
/// inlined verbatim on both sides, so it must be a self-contained
/// expression with an inferable type.
const CASES: &[&str] = &[
    // Int arithmetic + operator semantics
    "2 + 3 * 4",
    "100 / 7",
    "100 % 7",
    "7 / 2",
    "(0 - 17) % 5",
    "(0 - 17) / 5",
    "1 << 10",
    "255 & 15",
    "12 | 3",
    "6 ^ 3",
    "1000000 * 1000000",
    // Float rounding + S21 "always a decimal" display
    "3.0 / 2.0",
    "10.0 / 4.0",
    "1.0 / 3.0",
    "5.0",
    "2.0 * 2.0",
    "0.1 + 0.2",
    // Bool / comparison
    "3 < 5 && 2 == 2",
    "10 >= 10 || false",
    // String + Char ops (char-counted, S41)
    "\"Hello\".to_upper()",
    "\"WORLD\".to_lower()",
    "\"héllo\".len()",
    "\"  trim me  \".trim()",
    "\"ab\".repeat(3)",
    "\"a,b,c\".split(\",\")",
    "\"hello world\".replace(\"o\", \"0\")",
    // List values, ordering, and methods
    "[1, 2, 3]",
    "[3, 1, 2]",
    "[10, 20, 30][1]",
    "[\"x\", \"y\", \"z\"]",
    // Map ordering via derived lists (BTreeMap is sorted by key)
    "[\"b\": 2, \"a\": 1, \"c\": 3].keys()",
    "[\"b\": 2, \"a\": 1, \"c\": 3].values()",
    "[2: \"two\", 1: \"one\"].keys()",
    // D-BIGINT1 (card #392): arbitrary-precision arithmetic — no overflow,
    // no auto-promotion. comptime must match AOT's limb-based `JetBigInt`
    // byte-for-byte (R12 parity).
    "(BigInt(9223372036854775807) + BigInt(1)).to_string()",
    "(BigInt(\"999999999999999999999999999999\") + BigInt(\"999999999999999999999999999999\")).to_string()",
    "(BigInt(100) - BigInt(1)).to_string()",
    "(BigInt(7) * BigInt(6)).to_string()",
    "BigInt(5).sub(BigInt(3)).to_string()",
    "BigInt(3).neg().to_string()",
];

/// card #392: the `use core.X as alias; alias.method(...)` module-call form
/// needs its own program per case (an inline expression alone can't `use`),
/// so it gets a dedicated differential loop rather than reusing `CASES`.
const MODULE_CASES: &[&str] = &[
    // `("core.string", ...)` was a dead dispatch key (no import resolves to
    // it — `core.text` is the only ratified spelling), so every
    // `text.<method>(...)` call hit E0956. Fixed via `TextLite` (ported
    // verbatim from AOT's `jet_text_*` prelude fns).
    "use core.text as text\ncomptime C = text.trim(\" hi \")\n\nfn run() {\n    r :: text.trim(\" hi \")\n    print(\"{C}\")\n    print(\"{r}\")\n}\n",
    "use core.text as text\ncomptime C = text.upper(\"abc\")\n\nfn run() {\n    r :: text.upper(\"abc\")\n    print(\"{C}\")\n    print(\"{r}\")\n}\n",
    "use core.text as text\ncomptime C = text.words(\"hello world's foo\")[0]\n\nfn run() {\n    r :: text.words(\"hello world's foo\")[0]\n    print(\"{C}\")\n    print(\"{r}\")\n}\n",
    "use core.text as text\ncomptime C = text.pad_start(\"7\", 3, \"0\")\n\nfn run() {\n    r :: text.pad_start(\"7\", 3, \"0\")\n    print(\"{C}\")\n    print(\"{r}\")\n}\n",
    // core.math: previously only sqrt/floor/ceil/round/abs/pow/min/max/clamp/
    // log2/log10 were dispatched; the rest (trig, checked/saturating/
    // wrapping, gcd/lcm) fell to E0956.
    "use core.math as math\ncomptime C = math.sin(0.0)\n\nfn run() {\n    r :: math.sin(0.0)\n    print(\"{C}\")\n    print(\"{r}\")\n}\n",
    "use core.math as math\ncomptime C = math.gcd(12, 18)\n\nfn run() {\n    r :: math.gcd(12, 18)\n    print(\"{C}\")\n    print(\"{r}\")\n}\n",
    "use core.math as math\ncomptime C = math.saturating_add(9223372036854775807, 1)\n\nfn run() {\n    r :: math.saturating_add(9223372036854775807, 1)\n    print(\"{C}\")\n    print(\"{r}\")\n}\n",
    // card #392 pass 3: `core.url` (D-URL1=A), ported verbatim from AOT's
    // `jet_url_*` (`UrlMime.rs` + `MathRandomTime.rs`, see `UrlLite.rs`).
    // Only the plain-`String`-returning free functions go through this
    // rustc-verified differential (parse/from_parts/file/data return a `Url`
    // struct whose instance methods — `.scheme()` etc — are a separate,
    // out-of-scope gap, so there's no printable way to compare their
    // contents byte-for-byte here; those are covered by
    // `tests/repl.rs::repl_core_url_dispatch` instead).
    "use core.url as url\ncomptime C = url.percent_encode(\"a b/c?d#e\")\n\nfn run() {\n    r :: url.percent_encode(\"a b/c?d#e\")\n    print(\"{C}\")\n    print(\"{r}\")\n}\n",
    "use core.url as url\ncomptime C = url.percent_decode(\"a%20b%2Fc\") ?? panic(\"bad\")\n\nfn run() {\n    r :: url.percent_decode(\"a%20b%2Fc\") ?? panic(\"bad\")\n    print(\"{C}\")\n    print(\"{r}\")\n}\n",
    "use core.url as url\ncomptime C = url.percent_decode(\"bad%\") ?? \"fallback\"\n\nfn run() {\n    r :: url.percent_decode(\"bad%\") ?? \"fallback\"\n    print(\"{C}\")\n    print(\"{r}\")\n}\n",
    "use core.url as url\ncomptime C = url.query([[\"a\", \"1\"], [\"b\", \"2 c\"]])\n\nfn run() {\n    r :: url.query([[\"a\", \"1\"], [\"b\", \"2 c\"]])\n    print(\"{C}\")\n    print(\"{r}\")\n}\n",
    // card #392 pass 3: `core.data`'s fixed-signature stats surface, ported
    // verbatim from AOT's `jet_data_*` (`EncodingTraits.rs`, see
    // `DataLite.rs`). `describe`/`status`/`bar_text`/`bar_svg` return/take
    // builtin struct values (`DataSummary`/`DataStatus`/`DataGroup`), which
    // this crate's generic `CtValue::Struct` display can't print the same
    // way AOT's derived struct `Display` does (a pre-existing limit shared
    // by every builtin struct type, not specific to `core.data` — see
    // `UrlLite.rs`'s note) — covered instead by
    // `tests/repl.rs::repl_core_data_dispatch`.
    "use core.data as data\ncomptime C = data.sum([1.0, 2.0, 3.5])\n\nfn run() {\n    r :: data.sum([1.0, 2.0, 3.5])\n    print(\"{C}\")\n    print(\"{r}\")\n}\n",
    "use core.data as data\ncomptime C = data.mean([1.0, 2.0, 3.0, 4.0])\n\nfn run() {\n    r :: data.mean([1.0, 2.0, 3.0, 4.0])\n    print(\"{C}\")\n    print(\"{r}\")\n}\n",
    "use core.data as data\ncomptime C = data.median([5.0, 1.0, 3.0, 2.0])\n\nfn run() {\n    r :: data.median([5.0, 1.0, 3.0, 2.0])\n    print(\"{C}\")\n    print(\"{r}\")\n}\n",
    "use core.data as data\ncomptime C = data.variance([2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0])\n\nfn run() {\n    r :: data.variance([2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0])\n    print(\"{C}\")\n    print(\"{r}\")\n}\n",
    "use core.data as data\ncomptime C = data.stddev([2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0])\n\nfn run() {\n    r :: data.stddev([2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0])\n    print(\"{C}\")\n    print(\"{r}\")\n}\n",
    "use core.data as data\ncomptime C = data.quantile([1.0, 2.0, 3.0, 4.0, 5.0], 0.25)\n\nfn run() {\n    r :: data.quantile([1.0, 2.0, 3.0, 4.0, 5.0], 0.25)\n    print(\"{C}\")\n    print(\"{r}\")\n}\n",
    "use core.data as data\ncomptime C = data.rolling_mean([1.0, 2.0, 3.0, 4.0], 2)\n\nfn run() {\n    r :: data.rolling_mean([1.0, 2.0, 3.0, 4.0], 2)\n    print(\"{C}\")\n    print(\"{r}\")\n}\n",
    "use core.data as data\ncomptime C = data.min([3.0, -1.0, 5.0])\n\nfn run() {\n    r :: data.min([3.0, -1.0, 5.0])\n    print(\"{C}\")\n    print(\"{r}\")\n}\n",
    "use core.data as data\ncomptime C = data.max([3.0, -1.0, 5.0])\n\nfn run() {\n    r :: data.max([3.0, -1.0, 5.0])\n    print(\"{C}\")\n    print(\"{r}\")\n}\n",
    // card #392 pass 4: `core.encoding.{csv,toml,yaml,xml,cbor,jsonl}` +
    // `core.encoding.json.{canonical,events}`, ported verbatim from AOT's
    // `jet_ring_csv_*`/`toml`/`yaml` mods/`jet_std_xml_*`/`jet_cbor_*`/
    // `jet_std_jsonl_*`/`jet_std_json_render_canonical`/`jet_std_json_events`
    // (see `EncodingLite.rs`). Every case round-trips `parse`+`to_string` (or
    // `to_bytes`+`parse`) so both the parser and the renderer sides differ
    // against real generated Rust, not just one direction.
    "use core.encoding.csv as csv\ncomptime C = csv.to_string(csv.parse(\"a,\\\"b,c\\\",\\\"e\\\"\\\"f\\\"\\n\") ?? panic(\"bad\"))\n\nfn run() {\n    r :: csv.to_string(csv.parse(\"a,\\\"b,c\\\",\\\"e\\\"\\\"f\\\"\\n\") ?? panic(\"bad\"))\n    print(\"{C}\")\n    print(\"{r}\")\n}\n",
    "use core.encoding.csv as csv\ncomptime C = csv.parse(\"a,b,c\\n1,2\\n\") ?? panic(\"bad\")\n\nfn run() {\n    r :: csv.parse(\"a,b,c\\n1,2\\n\") ?? panic(\"bad\")\n    print(\"{C}\")\n    print(\"{r}\")\n}\n",
    "use core.encoding.toml as toml\ncomptime C = toml.to_string(toml.parse(\"[a]\\nx = 1\\n\\n[[a.b]]\\ny = 2\\n\\n[[a.b]]\\ny = 3\\n\") ?? panic(\"bad\")).replace(\"\\n\", \"|\")\n\nfn run() {\n    r :: toml.to_string(toml.parse(\"[a]\\nx = 1\\n\\n[[a.b]]\\ny = 2\\n\\n[[a.b]]\\ny = 3\\n\") ?? panic(\"bad\")).replace(\"\\n\", \"|\")\n    print(\"{C}\")\n    print(\"{r}\")\n}\n",
    "use core.encoding.toml as toml\ncomptime C = toml.to_string(toml.parse(\"x = 1.5\\ny = [1, 2, 3]\\nz = {{ a = 1, b = 2 }}\\n\") ?? panic(\"bad\")).replace(\"\\n\", \"|\")\n\nfn run() {\n    r :: toml.to_string(toml.parse(\"x = 1.5\\ny = [1, 2, 3]\\nz = {{ a = 1, b = 2 }}\\n\") ?? panic(\"bad\")).replace(\"\\n\", \"|\")\n    print(\"{C}\")\n    print(\"{r}\")\n}\n",
    "use core.encoding.yaml as yaml\ncomptime C = yaml.to_string(yaml.parse(\"a: &x 1\\nb: *x\\nc:\\n  - 1\\n  - 2\\nd: |\\n  hello\\n  world\\n\") ?? panic(\"bad\")).replace(\"\\n\", \"|\")\n\nfn run() {\n    r :: yaml.to_string(yaml.parse(\"a: &x 1\\nb: *x\\nc:\\n  - 1\\n  - 2\\nd: |\\n  hello\\n  world\\n\") ?? panic(\"bad\")).replace(\"\\n\", \"|\")\n    print(\"{C}\")\n    print(\"{r}\")\n}\n",
    "use core.encoding.xml as xml\ncomptime C = xml.to_string(xml.parse(\"<root xmlns:ns=\\\"http://example\\\"><ns:child id=\\\"1\\\">text</ns:child></root>\") ?? panic(\"bad\"))\n\nfn run() {\n    r :: xml.to_string(xml.parse(\"<root xmlns:ns=\\\"http://example\\\"><ns:child id=\\\"1\\\">text</ns:child></root>\") ?? panic(\"bad\"))\n    print(\"{C}\")\n    print(\"{r}\")\n}\n",
    "use core.encoding.json as json\nuse core.encoding.cbor as cbor\nuse core.encoding.hex as hex\ncomptime C = hex.encode(cbor.to_bytes(json.parse(\"{{\\\"a\\\":1,\\\"b\\\":[1,2,3],\\\"c\\\":-7}}\") ?? panic(\"bad\")) ?? panic(\"bad\"))\n\nfn run() {\n    r :: hex.encode(cbor.to_bytes(json.parse(\"{{\\\"a\\\":1,\\\"b\\\":[1,2,3],\\\"c\\\":-7}}\") ?? panic(\"bad\")) ?? panic(\"bad\"))\n    print(\"{C}\")\n    print(\"{r}\")\n}\n",
    "use core.encoding.json as json\nuse core.encoding.cbor as cbor\ncomptime C = json.to_string(cbor.parse(cbor.to_bytes(json.parse(\"{{\\\"a\\\":1,\\\"b\\\":[1,2,3]}}\") ?? panic(\"bad\")) ?? panic(\"bad\")) ?? panic(\"bad\"))\n\nfn run() {\n    r :: json.to_string(cbor.parse(cbor.to_bytes(json.parse(\"{{\\\"a\\\":1,\\\"b\\\":[1,2,3]}}\") ?? panic(\"bad\")) ?? panic(\"bad\")) ?? panic(\"bad\"))\n    print(\"{C}\")\n    print(\"{r}\")\n}\n",
    "use core.encoding.jsonl as jsonl\ncomptime C = jsonl.to_string(jsonl.parse(\"{{\\\"a\\\":1}}\\n{{\\\"b\\\":2}}\\n\") ?? panic(\"bad\")).replace(\"\\n\", \"|\")\n\nfn run() {\n    r :: jsonl.to_string(jsonl.parse(\"{{\\\"a\\\":1}}\\n{{\\\"b\\\":2}}\\n\") ?? panic(\"bad\")).replace(\"\\n\", \"|\")\n    print(\"{C}\")\n    print(\"{r}\")\n}\n",
    "use core.encoding.json as json\ncomptime C = json.canonical(json.parse(\"{{\\\"b\\\":1,\\\"a\\\":2}}\") ?? panic(\"bad\"))\n\nfn run() {\n    r :: json.canonical(json.parse(\"{{\\\"b\\\":1,\\\"a\\\":2}}\") ?? panic(\"bad\"))\n    print(\"{C}\")\n    print(\"{r}\")\n}\n",
    "use core.encoding.json as json\ncomptime C = json.events(json.parse(\"{{\\\"a\\\":[1,2]}}\") ?? panic(\"bad\")).replace(\"\\n\", \"|\")\n\nfn run() {\n    r :: json.events(json.parse(\"{{\\\"a\\\":[1,2]}}\") ?? panic(\"bad\")).replace(\"\\n\", \"|\")\n    print(\"{C}\")\n    print(\"{r}\")\n}\n",
];

#[test]
fn comptime_matches_runtime() {
    let have_rustc = have_rustc();
    if !have_rustc {
        eprintln!("note: rustc not found; skipping comptime differential battery");
        return;
    }
    let next = Arc::new(AtomicUsize::new(0));
    let failures = Arc::new(Mutex::new(Vec::new()));
    let workers = test_worker_count(16).min(CASES.len().max(1));
    let mut handles = Vec::new();
    for _ in 0..workers {
        let next = Arc::clone(&next);
        let failures = Arc::clone(&failures);
        handles.push(std::thread::spawn(move || {
            loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= CASES.len() {
                    break;
                }
                if let Err(payload) = std::panic::catch_unwind(|| check_comptime_case(i, CASES[i]))
                {
                    failures.lock().unwrap().push(panic_message(payload));
                }
            }
        }));
    }
    for handle in handles {
        handle.join().unwrap();
    }
    let failures = failures.lock().unwrap();
    if !failures.is_empty() {
        panic!("{}", failures.join("\n\n"));
    }
}

fn check_comptime_case(i: usize, expr: &str) {
    let src = format!(
        "comptime C = {e}\n\nfn run() {{\n    r :: {e}\n    print(\"{{C}}\")\n    print(\"{{r}}\")\n}}\n",
        e = expr
    );
    check_comptime_src(i, expr, &src);
}

/// Shared by `check_comptime_case` (single-expression cases) and
/// `check_comptime_module_case` (card #392's `use core.X as a; a.f(...)`
/// cases, which need a full program — a bare `use` isn't an expression).
fn check_comptime_src(i: usize, label: &str, src: &str) {
    let compiled = match jet::compile(src) {
        Ok(c) => c,
        Err(diags) => panic!(
            "case {} `{}` failed the front end:\n{}",
            i,
            label,
            jet::render_diagnostics("comptime_diff.jet", src, &diags)
        ),
    };
    // D-BIGINT1 (card #392): a `BigInt` case pulls the Top-tier prelude
    // module, which shares a file with `jet_atomic_windows`'s vetted FFI
    // internals (I1 gate, `JET_VETTED_UNSAFE_BEGIN/END` markers) — strip it
    // before the I1 check, same as `golden.rs::strip_vetted_prelude_modules`.
    let user_code = common::strip_vetted_prelude_modules(&compiled.rust);
    assert!(
        !user_code.contains("unsafe"),
        "case `{}` generated unsafe outside the vetted prelude",
        label
    );

    let dir = std::env::temp_dir();
    let rs = dir.join(format!("jet_ctdiff_{}_{}.rs", std::process::id(), i));
    let bin = dir.join(format!("jet_ctdiff_{}_{}", std::process::id(), i));
    fs::write(&rs, &compiled.rust).unwrap();
    let out = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&rs)
        .arg("-o")
        .arg(&bin)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "I2 violated: rustc rejected generated code for `{}`:\n{}",
        label,
        String::from_utf8_lossy(&out.stderr)
    );
    let run = Command::new(&bin).output().unwrap();
    assert!(run.status.success(), "case `{}` panicked at runtime", label);
    let stdout = String::from_utf8_lossy(&run.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        lines.len(),
        2,
        "case `{}` printed {} lines, expected 2",
        label,
        lines.len()
    );
    assert_eq!(
        lines[0], lines[1],
        "DIVERGENCE for `{}`: comptime gave {:?}, runtime gave {:?} — this is a P0 miscompile",
        label, lines[0], lines[1]
    );
}

#[test]
fn comptime_module_calls_match_runtime() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping comptime module-call differential battery");
        return;
    }
    for (i, src) in MODULE_CASES.iter().enumerate() {
        check_comptime_src(1000 + i, src, src);
    }
}

#[test]
fn cbor_generic_whole_decode_matches_comptime_and_aot() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping CBOR whole-value differential");
        return;
    }
    // D-ENC-CBOR-SURFACE1 / #296: current generic whole-value decode,
    // including normal-mode indefinite containers and preferred Float16,
    // is one R12 semantic path at comptime and AOT. This intentionally does
    // not exercise the retired untyped `decode(DataTree)` compatibility arm.
    let src = "use core.encoding.cbor as cbor\ncomptime C = cbor.decode<[Float]>([159, 249, 62, 0, 249, 64, 0, 255]) ?? panic(\"bad\")\n\nfn run() {\n    r: [Float] := cbor.decode<[Float]>([159, 249, 62, 0, 249, 64, 0, 255]) ?? panic(\"bad\")\n    print(\"{C}\")\n    print(\"{r}\")\n}\n";
    check_comptime_src(2000, "generic CBOR indefinite Float16 decode", src);
}

#[test]
fn cbor_current_whole_encode_parse_matches_comptime_and_aot() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping current CBOR whole-value differential");
        return;
    }
    let src = "use core.encoding.json as json\nuse core.encoding.cbor as cbor\nuse core.encoding.hex as hex\ncomptime C = hex.encode(cbor.to_bytes(json.parse(\"{{\\\"a\\\":1,\\\"b\\\":[1,2,3],\\\"c\\\":-7}}\") ?? panic(\"bad json\")) ?? panic(\"bad cbor\"))\ncomptime P = json.to_string(cbor.parse(cbor.to_bytes(json.parse(\"{{\\\"a\\\":1,\\\"b\\\":[1,2,3]}}\") ?? panic(\"bad json\")) ?? panic(\"bad cbor\")) ?? panic(\"bad parse\"))\n\nfn run() {\n    r :: hex.encode(cbor.to_bytes(json.parse(\"{{\\\"a\\\":1,\\\"b\\\":[1,2,3],\\\"c\\\":-7}}\") ?? panic(\"bad json\")) ?? panic(\"bad cbor\"))\n    p :: json.to_string(cbor.parse(cbor.to_bytes(json.parse(\"{{\\\"a\\\":1,\\\"b\\\":[1,2,3]}}\") ?? panic(\"bad json\")) ?? panic(\"bad cbor\")) ?? panic(\"bad parse\"))\n    print(\"{C}|{P}\")\n    print(\"{r}|{p}\")\n}\n";
    check_comptime_src(2001, "current CBOR to_bytes and parse", src);
}

#[test]
fn cbor_canonical_typed_corpus_matches_comptime_and_aot() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping canonical CBOR comptime differential");
        return;
    }
    let src = r#"use core.encoding.json as json
use core.encoding.cbor as cbor
use core.encoding.hex as hex

@[Codable]
struct Packet { id: Int, payload: [U8] }

comptime MAP = hex.encode(cbor.to_bytes_canonical(json.parse("{{\"aa\":1,\"b\":2}}") ?? panic("json")) ?? panic("canonical"))
comptime FLOATS = hex.encode(cbor.to_bytes_canonical([1.5, 100000.0, -0.0]) ?? panic("canonical"))
comptime NAN = hex.encode(cbor.to_bytes_canonical(Float.NAN) ?? panic("canonical"))
comptime TYPED = hex.encode(cbor.to_bytes_canonical(Packet.{ id: 7, payload: [222, 173] }) ?? panic("canonical"))

fn run() {
    map := hex.encode(cbor.to_bytes_canonical(json.parse("{{\"aa\":1,\"b\":2}}") ?? panic("json")) ?? panic("canonical"))
    floats := hex.encode(cbor.to_bytes_canonical([1.5, 100000.0, -0.0]) ?? panic("canonical"))
    nan := hex.encode(cbor.to_bytes_canonical(Float.NAN) ?? panic("canonical"))
    typed := hex.encode(cbor.to_bytes_canonical(Packet.{ id: 7, payload: [222, 173] }) ?? panic("canonical"))
    if MAP != "a261620262616101" { panic("canonical encoded-key order drift") }
    if FLOATS != "83f93e00fa47c35000f98000" { panic("preferred Float width drift") }
    if NAN != "f97e00" { panic("canonical NaN drift") }
    if TYPED != "a262696407677061796c6f616442dead" { panic("typed byte-string drift") }
    print("{MAP}|{FLOATS}|{NAN}|{TYPED}")
    print("{map}|{floats}|{nan}|{typed}")
}
"#;
    check_comptime_src(2002, "canonical CBOR map/float/typed corpus", src);
}

#[test]
fn cbor_options_and_hostile_errors_match_comptime_and_aot() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping CBOR option/error differential");
        return;
    }
    let src = r#"use core.encoding.cbor as cbor

fn safe() -> cbor.CBOROptions {
    return cbor.CBOROptions.{ max_depth: 256, max_items: 1000000, max_bytes: 1073741824, require_canonical: false }
}

fn show(bytes: [U8]) -> String {
    if cbor.parse(bytes, safe()) == {
        ok(_) -> return "ok"
        err(e) -> return "{e.byte_offset}|{e.path}|{e.reason}"
    }
    return "unreachable"
}

fn show_strict(bytes: [U8]) -> String {
    if cbor.parse(bytes, cbor.CBOROptions.{ max_depth: 256, max_items: 1000000, max_bytes: 1073741824, require_canonical: true }) == {
        ok(_) -> return "ok"
        err(e) -> return "{e.byte_offset}|{e.path}|{e.reason}"
    }
    return "unreachable"
}
fn show_depth(bytes: [U8]) -> String {
    if cbor.parse(bytes, cbor.CBOROptions.{ max_depth: 1, max_items: 1000000, max_bytes: 1073741824, require_canonical: false }) == {
        ok(_) -> return "ok"
        err(e) -> return "{e.byte_offset}|{e.path}|{e.reason}"
    }
    return "unreachable"
}
fn show_items(bytes: [U8]) -> String {
    if cbor.parse(bytes, cbor.CBOROptions.{ max_depth: 256, max_items: 2, max_bytes: 1073741824, require_canonical: false }) == {
        ok(_) -> return "ok"
        err(e) -> return "{e.byte_offset}|{e.path}|{e.reason}"
    }
    return "unreachable"
}
fn show_bytes(bytes: [U8]) -> String {
    if cbor.parse(bytes, cbor.CBOROptions.{ max_depth: 256, max_items: 1000000, max_bytes: 2, require_canonical: false }) == {
        ok(_) -> return "ok"
        err(e) -> return "{e.byte_offset}|{e.path}|{e.reason}"
    }
    return "unreachable"
}
fn show_alloc(bytes: [U8]) -> String {
    if cbor.parse(bytes, cbor.CBOROptions.{ max_depth: 256, max_items: 1000000, max_bytes: 3, require_canonical: false }) == {
        ok(_) -> return "ok"
        err(e) -> return "{e.byte_offset}|{e.path}|{e.reason}"
    }
    return "unreachable"
}

fn show_ints(bytes: [U8]) -> String {
    if cbor.decode<[Int]>(bytes, safe()) == {
        ok(_) -> return "ok"
        err(e) -> return "{e.byte_offset}|{e.path}|{e.reason}"
    }
    return "unreachable"
}

comptime MALFORMED = show([255])
comptime TRUNCATED = show([129])
comptime NONCANONICAL = show_strict([24, 1])
comptime UNSUPPORTED = show([192, 1])
comptime MISMATCH = show_ints([129, 97, 120])
comptime DEPTH = show_depth([129, 129, 1])
comptime ITEMS = show_items([130, 1, 2])
comptime BYTES = show_bytes([130, 1, 2])
comptime ALLOC = show_alloc([130, 1, 2])

fn run() {
    malformed_wire: [U8] := [255]
    truncated_wire: [U8] := [129]
    noncanonical_wire: [U8] := [24, 1]
    unsupported_wire: [U8] := [192, 1]
    mismatch_wire: [U8] := [129, 97, 120]
    depth_wire: [U8] := [129, 129, 1]
    items_wire: [U8] := [130, 1, 2]
    malformed := show(malformed_wire)
    truncated := show(truncated_wire)
    noncanonical := show_strict(noncanonical_wire)
    unsupported := show(unsupported_wire)
    mismatch := show_ints(mismatch_wire)
    depth := show_depth(depth_wire)
    items := show_items(items_wire)
    bytes := show_bytes(items_wire)
    alloc := show_alloc(items_wire)
    print("{MALFORMED}~{TRUNCATED}~{NONCANONICAL}~{UNSUPPORTED}~{MISMATCH}~{DEPTH}~{ITEMS}~{BYTES}~{ALLOC}")
    print("{malformed}~{truncated}~{noncanonical}~{unsupported}~{mismatch}~{depth}~{items}~{bytes}~{alloc}")
}
"#;
    check_comptime_src(2003, "CBOR options and hostile error projection", src);
}
#[test]
fn local_comptime_is_literal_data() {
    let stdout = compile_and_run(
        r#"
fn build() -> [Int] {
    xs: [Int] := []
    loop i in 1..3 {
        xs.push(i * 10)
    }
    return xs
}

fn run() {
    comptime xs = build()
    print("{xs}")
    print("{xs[1]}")
}
"#,
    );
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        vec!["[10, 20, 30]", "20"]
    );
}

#[test]
fn struct_and_enum_comptime_values_round_trip() {
    let stdout = compile_and_run(
        r#"
struct Pair {
    left: Int
    right: String
}

enum Light {
    Red
    Green
}

comptime P = Pair.{left: 7, right: "seven"}
comptime L = Light.Green

fn run() {
    p :: Pair.{left: 7, right: "seven"}
    l :: Light.Green
    print("{P.left}")
    print("{p.left}")
    print("{P.right}")
    print("{p.right}")
    print("{L == Light.Green}")
    print("{l == Light.Green}")
}
"#,
    );
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        vec!["7", "7", "seven", "seven", "true", "true"]
    );
}

#[test]
fn if_expr_comptime_matches_runtime() {
    let stdout = compile_and_run(
        r#"
comptime C = if 3 > 2 { 10 } else { 20 }
comptime D = if 1 > 2 { 10 } else { 20 }

fn run() {
    c :: if 3 > 2 { 10 } else { 20 }
    d :: if 1 > 2 { 10 } else { 20 }
    print("{C}")
    print("{c}")
    print("{D}")
    print("{d}")
}
"#,
    );
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        vec!["10", "10", "20", "20"]
    );
}

#[test]
fn fan_out_comptime_matches_runtime() {
    let stdout = compile_and_run(
        r#"
fn double(x: Int) -> Int {
    return x * 2
}

comptime C = double.[1, 2, 3]

fn run() {
    c :: double.[1, 2, 3]
    print("{C}")
    print("{c}")
}
"#,
    );
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        vec!["[2, 4, 6]", "[2, 4, 6]"]
    );
}

fn compile_and_run(src: &str) -> String {
    let have_rustc = have_rustc();
    if !have_rustc {
        eprintln!("note: rustc not found; skipping comptime run fixture");
        return String::new();
    }
    let compiled = match jet::compile(src) {
        Ok(c) => c,
        Err(diags) => panic!(
            "fixture failed the front end:\n{}",
            jet::render_diagnostics("comptime_fixture.jet", src, &diags)
        ),
    };
    let dir = std::env::temp_dir();
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let rs = dir.join(format!("jet_ct_fixture_{}.rs", id));
    let bin = dir.join(format!("jet_ct_fixture_{}", id));
    fs::write(&rs, &compiled.rust).unwrap();
    let out = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&rs)
        .arg("-o")
        .arg(&bin)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "rustc rejected generated code:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let run = Command::new(&bin).output().unwrap();
    assert!(run.status.success(), "fixture panicked at runtime");
    String::from_utf8(run.stdout).unwrap()
}
