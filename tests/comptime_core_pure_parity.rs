//! #392 Packet B: remaining deterministic Core constructors/codecs must run
//! through the public REPL/comptime path and agree with generated Rust.

use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use jet::REPL::run_transcript;

mod common;

static SEQ: AtomicU64 = AtomicU64::new(0);

fn exact_values(inputs: &[&str]) -> Vec<String> {
    let output = run_transcript(inputs, None);
    assert!(!output.contains("error ["), "transcript failed:\n{output}");
    output
        .lines()
        .filter(|line| line.contains(" : "))
        .map(str::to_string)
        .collect()
}

#[test]
fn public_transcript_covers_remaining_core_pure_families() {
    let values = exact_values(&[
        "use core.mime as mime",
        "use core.time as time",
        "use core.time.date as date",
        "use core.time.datetime as datetime",
        "use core.science.measurement as measurement",
        "mime.from_extension(\".PNG\") ?? \"none\"",
        "mime.extension(\"Text/HTML; charset=UTF-8\") ?? \"none\"",
        "mime.parse(\"Text/HTML; charset=UTF-8\")",
        "time.period(1, 2, 3)",
        "time.period_days(4)",
        "time.period_months(5)",
        "time.period_years(6)",
        "time.parse_time(\"23:59:58\")",
        "time.parse_rfc3339(\"2024-02-29T12:34:56+02:30\")",
        "date.new(2024, 13, 40)",
        "date.parse(\"2024-02-29\")",
        "datetime.from_timestamp(-1)",
        "time.from_unix_ms(-1)",
        "measurement.from(12.5, 0.25) == measurement.from(12.5, 0.25)",
    ]);
    assert_eq!(
        values,
        [
            "\"image/png\" : String",
            "\"html\" : String",
            "Mime(top: text, sub: html, params: [[charset, UTF-8]]) : Result",
            "Period(years: 1, months: 2, days: 3) : Period",
            "Period(years: 0, months: 0, days: 4) : Period",
            "Period(years: 0, months: 5, days: 0) : Period",
            "Period(years: 6, months: 0, days: 0) : Period",
            "LocalTime(hour: 23, minute: 59, second: 58) : Result",
            "DateTime(secs: 1709201096) : Result",
            "LocalDate(year: 2024, month: 12, day: 31) : LocalDate",
            "LocalDate(year: 2024, month: 2, day: 29) : Result",
            "DateTime(secs: -1) : DateTime",
            "DateTime(secs: -1) : DateTime",
            "true : Bool",
        ]
    );
}

#[test]
fn public_transcript_composes_email_and_codecs_exactly() {
    let values = exact_values(&[
        "use core.email as email",
        "use core.encoding.xml as xml",
        "sender :: email.address(\"Mara <mara@example.com>\") ?? panic(\"sender\")",
        "recipient :: email.address(\"ada@example.net\") ?? panic(\"recipient\")",
        "attachment :: email.attachment(\"note.txt\", \"Text/Plain\", [104, 105]) ?? panic(\"attachment\")",
        "message :: email.message(sender, [recipient], [], \"Hello\", \"body\", \"\", [attachment]) ?? panic(\"message\")",
        "email.envelope(sender, [recipient])",
        "fn serialized(message: Message) -> Bool {\n    if email.serialize(message) == {\n        Ok(_) -> return true\n        Err(_) -> return false\n    }\n    return false\n}",
        "serialized(message)",
        "xml.canonical(xml.parse(\"<r b=\\\"2\\\" a=\\\"1\\\"><x/></r>\") ?? panic(\"xml\"), xml.XMLCanonical.{ mode: .Inclusive11, comments: false, inclusive_prefixes: [] }) ?? panic(\"canonical\")",
    ]);
    assert_eq!(
        values,
        [
            "Envelope(from: Address(display: Mara, mailbox: mara@example.com), recipients: [Address(display: null, mailbox: ada@example.net)]) : Result",
            "true : Bool",
            "\"<r a=\"1\" b=\"2\"><x></x></r>\" : String",
        ]
    );
}

fn parity_source(expression: &str, imports: &str) -> String {
    format!(
        "{imports}\ncomptime expected = {expression}\n\nfn run() {{\n    actual :: {expression}\n    print(\"{{expected}}\")\n    print(\"{{actual}}\")\n}}\n"
    )
}

fn check_aot_comptime(label: &str, source: &str) {
    assert!(common::have_rustc(), "{label} requires rustc");
    let compiled = jet::Driver::compile_generated_src(
        source,
        "comptime_core_pure_parity.jet",
        jet::Sema::CompileMode::Run,
    )
    .unwrap_or_else(|diags| {
        panic!(
            "{label} front-end failure:\n{}",
            jet::render_diagnostics("comptime_core_pure_parity.jet", source, &diags)
        )
    });
    let id = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = common::unique_tmp(&format!("jet_core_pure_{id}"));
    fs::create_dir_all(&dir).unwrap();
    let rust = dir.join("main.rs");
    let binary = dir.join("main");
    fs::write(&rust, &compiled.rust).unwrap();
    let mut rustc = Command::new("rustc");
    rustc
        .args(["--edition", "2021"])
        .arg(&rust)
        .arg("-o")
        .arg(&binary);
    if let Some(link) = &compiled.ffi {
        rustc
            .arg("--extern")
            .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        for directory in link.dependency_dirs().filter(|directory| directory.is_dir()) {
            rustc
                .arg("-L")
                .arg(format!("dependency={}", directory.display()));
        }
    }
    let built = rustc.output().unwrap();
    assert!(
        built.status.success(),
        "rustc rejected {label}:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let run = Command::new(binary).output().unwrap();
    assert!(run.status.success(), "{label} runtime failed");
    let lines = String::from_utf8(run.stdout).unwrap();
    let lines = lines.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 2, "{label} emitted unexpected output: {lines:?}");
    assert_eq!(lines[0], lines[1], "{label} comptime/AOT divergence");
}

#[test]
fn rustc_backed_aot_comptime_differentials_cover_return_shapes() {
    let cases = [
        (
            "option/string",
            parity_source("mime.extension(\"image/png\") ?? \"none\"", "use core.mime as mime"),
        ),
        (
            "result/bytes",
            parity_source(
                "email_wire()",
                "use core.email as email\nuse core.encoding.hex as hex\nfn email_wire() -> String {\n    message :: email.message(email.address(\"a@example.com\") ?? panic(\"a\"), [email.address(\"b@example.com\") ?? panic(\"b\")], [], \"s\", \"body\", \"\", []) ?? panic(\"m\")\n    return hex.encode(email.serialize(message) ?? panic(\"serialize\"))\n}",
            ),
        ),
        (
            "result/string",
            parity_source(
                "xml_canonical()",
                "use core.encoding.xml as xml\nfn xml_canonical() -> String {\n    tree :: xml.parse(\"<r b=\\\"2\\\" a=\\\"1\\\"><x/></r>\") ?? panic(\"xml\")\n    return xml.canonical(tree, xml.XMLCanonical.{ mode: .Inclusive11, comments: false, inclusive_prefixes: [] }) ?? panic(\"canonical\")\n}",
            ),
        ),
        (
            "xml/float-shape-error",
            parity_source(
                "xml_shape_reason(DataTree.Float(1.5))",
                "use core.encoding.xml as xml\nfn xml_shape_reason(tree: DataTree) -> String {\n    if xml.canonical(tree, xml.XMLCanonical.{ mode: .Inclusive11, comments: false, inclusive_prefixes: [] }) == {\n        Ok(_) -> return \"unexpected success\"\n        Err(error) -> return error.reason\n    }\n    return \"unreachable\"\n}",
            ),
        ),
    ];
    for (label, source) in cases {
        check_aot_comptime(label, &source);
    }
}
