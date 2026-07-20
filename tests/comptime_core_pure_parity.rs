//! #392 Packet B: remaining deterministic Core constructors/codecs must run
//! through the public REPL/comptime path and agree with generated Rust.

use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use jet::Interpreter::{dev_iteration, RunOutcome};
use jet::REPL::run_transcript;

mod common;

static SEQ: AtomicU64 = AtomicU64::new(0);

const PIVOT_DECLS: &str = "use core.data as data\nstruct PivotRow { team: String; bucket: String; score: Float }\nfn pivot_view() -> String {\n    prefix :: \"p\"\n    rows :: [PivotRow.{ team: \"B\", bucket: \"y\", score: 5.0 }, PivotRow.{ team: \"A\", bucket: \"x\", score: 1.5 }, PivotRow.{ team: \"A\", bucket: \"x\", score: 2.5 }]\n    groups :: data.pivot_sum(rows, (row) => \"{prefix}{row.team}\", (row) => row.bucket, (row) => row.score)\n    return \"{groups[0].key}:{groups[0].count}:{groups[0].sum}:{groups[0].mean}|{groups[1].key}:{groups[1].count}:{groups[1].sum}:{groups[1].mean}\"\n}";
const PIVOT_EXPR: &str = "pivot_view()";
const CIVIL_FN: &str = "fn civil_view() -> String {\n    d :: date.parse(\"2024-02-29\") ?? panic(\"date\")\n    other :: date.new(2024, 2, 1)\n    p :: time.period(0, 1, 2)\n    dt :: datetime.from_timestamp(-1)\n    span :: Duration.milliseconds(1500) ?? panic(\"duration\")\n    return \"{d.weekday()}|{d.iso_weekday()}|{d.day_of_year()}|{d.iso_week()}|{d.add_days(1).to_string()}|{d.add_months(12).to_string()}|{d.diff_days(other)}|{d.add_period(p).to_string()}|{d.truncate(\"month\").to_string()}|{d.format(\"EEE yyyy-DDD\")}|{dt.date().to_string()}|{dt.time().to_string()}|{dt.hour()}:{dt.minute()}:{dt.second()}|{dt.format_rfc3339()}|{dt.format(\"yyyy-MM-dd HH:mm:ss\")}|{dt.plus_duration(span).to_timestamp()}|{dt.truncate(\"minute\").to_timestamp()}|{dt.round(\"minute\").to_timestamp()}\"\n}";
const CIVIL_DECLS: &str = "use core.time as time\nuse core.time.date as date\nuse core.time.datetime as datetime\nfn civil_view() -> String {\n    d :: date.parse(\"2024-02-29\") ?? panic(\"date\")\n    other :: date.new(2024, 2, 1)\n    p :: time.period(0, 1, 2)\n    dt :: datetime.from_timestamp(-1)\n    span :: Duration.milliseconds(1500) ?? panic(\"duration\")\n    return \"{d.weekday()}|{d.iso_weekday()}|{d.day_of_year()}|{d.iso_week()}|{d.add_days(1).to_string()}|{d.add_months(12).to_string()}|{d.diff_days(other)}|{d.add_period(p).to_string()}|{d.truncate(\"month\").to_string()}|{d.format(\"EEE yyyy-DDD\")}|{dt.date().to_string()}|{dt.time().to_string()}|{dt.hour()}:{dt.minute()}:{dt.second()}|{dt.format_rfc3339()}|{dt.format(\"yyyy-MM-dd HH:mm:ss\")}|{dt.plus_duration(span).to_timestamp()}|{dt.truncate(\"minute\").to_timestamp()}|{dt.round(\"minute\").to_timestamp()}\"\n}";
const CIVIL_EXPR: &str = "civil_view()";
const CIVIL_DEV_DECLS: &str = "use core.time.date as date\nuse core.time.datetime as datetime\nfn civil_dev_view() -> String {\n    d :: date.parse(\"2024-02-29\") ?? panic(\"date\")\n    other :: date.new(2024, 2, 1)\n    dt :: datetime.from_timestamp(-1)\n    return \"{d.weekday()}|{d.iso_weekday()}|{d.day_of_year()}|{d.iso_week()}|{d.add_days(1).to_string()}|{d.add_months(12).to_string()}|{d.diff_days(other)}|{d.truncate(\"month\").to_string()}|{d.format(\"EEE yyyy-DDD\")}|{dt.date().to_string()}|{dt.time().to_string()}|{dt.hour()}:{dt.minute()}:{dt.second()}|{dt.format_rfc3339()}|{dt.format(\"yyyy-MM-dd HH:mm:ss\")}|{dt.truncate(\"minute\").to_timestamp()}|{dt.round(\"minute\").to_timestamp()}\"\n}";
const MEASUREMENT_FN: &str = "fn measurement_math() -> String {\n    a :: measurement.from(3.0, 4.0)\n    b :: measurement.from(0.0, 3.0)\n    q :: measurement.from(8.0, 0.0).div(measurement.from(2.0, 0.0))\n    return \"{a.add(b).value()}|{a.add(b).uncertainty()}|{a.sub(b).value()}|{a.sub(b).uncertainty()}|{a.mul(b).value()}|{a.mul(b).uncertainty()}|{q.value()}|{q.uncertainty()}\"\n}";
const MEASUREMENT_DECLS: &str = "use core.science.measurement as measurement\nfn measurement_math() -> String {\n    a :: measurement.from(3.0, 4.0)\n    b :: measurement.from(0.0, 3.0)\n    q :: measurement.from(8.0, 0.0).div(measurement.from(2.0, 0.0))\n    return \"{a.add(b).value()}|{a.add(b).uncertainty()}|{a.sub(b).value()}|{a.sub(b).uncertainty()}|{a.mul(b).value()}|{a.mul(b).uncertainty()}|{q.value()}|{q.uncertainty()}\"\n}";
const MEASUREMENT_EXPR: &str = "measurement_math()";
const SCALAR_DECLS: &str = r#"fn scalar_view() -> String {
    i8: I8 :: -12
    i16: I16 :: -1234
    i32: I32 :: -123456
    u8: U8 :: 255
    u16: U16 :: 1234
    u32: U32 :: 123456
    u64: U64 :: 123456789
    nan :: Float.parse("NaN") ?? 0.0
    infinity :: Float.parse("inf") ?? 0.0
    return "{"a@b@c".after("@")}|{"a@b@c".before("@")}|{"no-sep".after("@")}|{"no-sep".before("@")}|{"é🙂".bytes()}|{"aé🙂z".slice(1, 2)}|{nan.is_nan()}|{infinity.is_infinite()}|{1.0.is_finite()}|{i8.to_string()}|{i16.to_string()}|{i32.to_string()}|{u8.to_string()}|{u16.to_string()}|{u32.to_string()}|{u64.to_string()}"
}"#;
const SCALAR_EXPR: &str = "scalar_view()";
const SCALAR_EXPECTED: &str = "b@c|a|no-sep|no-sep|[195, 169, 240, 159, 153, 130]|é🙂|true|true|true|-12|-1234|-123456|255|1234|123456|123456789";
const RNG_DECLS: &str = r#"use core.random as random
fn rng_view() -> String {
    rng := random.rng(99)
    items := ["a", "b", "c", "d"]
    weights := [1.0, 2.0, 3.0, 4.0]
    deck := [1, 2, 3, 4, 5]
    int_draw :: rng.int(1, 100)
    float_draw :: rng.float()
    range_draw :: rng.float_range(-2.0, 2.0)
    coin :: rng.bool()
    chance :: rng.bool(0.25)
    normal :: rng.normal(1.0, 2.0)
    exponential :: rng.exponential(1.5)
    bytes :: rng.bytes(4)
    picked :: rng.pick(items) ?? "none"
    weighted :: rng.weighted_pick(items, weights) ?? "none"
    sample :: rng.sample(items, 2)
    rng.shuffle(&deck)
    child := rng.split()
    child_draw :: child.int(1, 100)
    after_split :: rng.int(1, 100)
    return "{int_draw}|{float_draw}|{range_draw}|{coin}|{chance}|{normal}|{exponential}|{bytes}|{picked}|{weighted}|{sample}|{deck}|{child_draw}|{after_split}"
}"#;
const RNG_EXPECTED: &str = "4|0.0316577610861849|1.3390388981797772|true|true|-0.6237918784672982|0.21210139132324568|[62, 20, 83, 254]|b|c|[c, a]|[1, 2, 5, 3, 4]|71|87";

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
        "measurement.from(12.5, 0.25).value()",
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
            "12.5 : Float",
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

#[test]
fn public_transcript_covers_civil_and_measurement_value_methods_exactly() {
    let values = exact_values(&[
        "use core.time as time",
        "use core.time.date as date",
        "use core.time.datetime as datetime",
        CIVIL_FN,
        CIVIL_EXPR,
        "use core.science.measurement as measurement",
        MEASUREMENT_FN,
        MEASUREMENT_EXPR,
    ]);
    assert_eq!(
        values,
        [
            "\"2|4|60|9|2024-03-01|2025-02-28|28|2024-03-31|2024-02-01|Thu 2024-060|1969-12-31|23:59:59|23:59:59|1969-12-31T23:59:59Z|1969-12-31 23:59:59|0|-60|0\" : String",
            "\"3.0|5.0|3.0|5.0|0.0|9.0|4.0|0.0\" : String",
        ]
    );
}

fn parity_source(expression: &str, imports: &str) -> String {
    format!(
        "{imports}\ncomptime expected = {expression}\n\nfn run() {{\n    actual :: {expression}\n    print(\"{{expected}}\")\n    print(\"{{actual}}\")\n}}\n"
    )
}

fn check_aot_comptime(label: &str, source: &str) -> String {
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
    lines[0].to_string()
}

fn check_dev_tiers(label: &str, source: &str, expected: &str) {
    check_dev_tiers_with_boundary(label, source, expected, false);
}

fn check_dev_tiers_with_boundary(
    label: &str,
    source: &str,
    expected: &str,
    force_interpreter: bool,
) {
    let id = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = common::unique_tmp(&format!("jet_core_pure_dev_{id}"));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{label}.jet"));
    fs::write(&path, source).unwrap();
    let path = path.to_string_lossy();
    for (tier, interpreter_only) in [("interpreter", true), ("default-dev", false)] {
        match dev_iteration(&path, force_interpreter && interpreter_only, interpreter_only) {
            RunOutcome::Ran { stdout, stderr, exit_code } => {
                assert_eq!(exit_code, 0, "{label} {tier} exit");
                assert_eq!(stderr, "", "{label} {tier} stderr");
                assert_eq!(
                    stdout,
                    format!("{expected}\n{expected}\n"),
                    "{label} {tier} stdout"
                );
            }
            RunOutcome::Problems(diags) => panic!("{label} {tier} failed: {diags:?}"),
        }
    }
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
        (
            "mime/observable-methods",
            parity_source(
                "mime_view()",
                "use core.mime as mime\nfn mime_view() -> String {\n    value :: mime.parse(\"Text/HTML; charset=UTF-8\") ?? panic(\"mime\")\n    return \"{value.media_type()}|{value.subtype()}|{value.essence()}|{value.param(\"charset\") ?? \"none\"}|{value.params()}|{value.to_string()}\"\n}",
            ),
        ),
        (
            "date/observable-methods",
            parity_source(
                "date_view()",
                "use core.time.date as date\nfn date_view() -> String {\n    parsed :: date.parse(\"2024-02-29\") ?? panic(\"date\")\n    clamped :: date.new(2024, 13, 40)\n    return \"{parsed.year()}-{parsed.month()}-{parsed.day()}|{parsed.to_string()}|{clamped.to_string()}\"\n}",
            ),
        ),
        (
            "time/observable-methods",
            parity_source(
                "time_view()",
                "use core.time as time\nfn time_view() -> String {\n    local :: time.parse_time(\"23:59:58\") ?? panic(\"time\")\n    datetime :: time.from_unix_ms(-1)\n    period :: time.period(1, 2, 3)\n    return \"{local.hour()}:{local.minute()}:{local.second()}|{local.to_string()}|{datetime.to_timestamp()}|{datetime.to_unix_ms()}|{period.to_string()}\"\n}",
            ),
        ),
        (
            "measurement/observable-methods",
            parity_source(
                "measurement_view()",
                "use core.science.measurement as measurement\nfn measurement_view() -> String {\n    value :: measurement.from(12.5, 0.25)\n    return \"{value.value()}|{value.uncertainty()}\"\n}",
            ),
        ),
    ];
    for (label, source) in cases {
        let _ = check_aot_comptime(label, &source);
    }
}

#[test]
fn rustc_backed_civil_and_measurement_methods_match_comptime_exactly() {
    let civil = "2|4|60|9|2024-03-01|2025-02-28|28|2024-03-31|2024-02-01|Thu 2024-060|1969-12-31|23:59:59|23:59:59|1969-12-31T23:59:59Z|1969-12-31 23:59:59|0|-60|0";
    let measurement = "3.0|5.0|3.0|5.0|0.0|9.0|4.0|0.0";
    let civil_source = parity_source(CIVIL_EXPR, CIVIL_DECLS);
    let civil_dev = "2|4|60|9|2024-03-01|2025-02-28|28|2024-02-01|Thu 2024-060|1969-12-31|23:59:59|23:59:59|1969-12-31T23:59:59Z|1969-12-31 23:59:59|-60|0";
    let civil_dev_source = parity_source("civil_dev_view()", CIVIL_DEV_DECLS);
    let measurement_source = parity_source(MEASUREMENT_EXPR, MEASUREMENT_DECLS);
    assert_eq!(
        check_aot_comptime("civil/all-deterministic-methods", &civil_source),
        civil
    );
    assert_eq!(
        check_aot_comptime("measurement/arithmetic", &measurement_source),
        measurement
    );
    check_dev_tiers("civil", &civil_dev_source, civil_dev);
    check_dev_tiers("measurement", &measurement_source, measurement);
}

#[test]
fn rustc_backed_datetime_and_measurement_display_are_exact() {
    assert_eq!(
        check_aot_comptime(
            "datetime/negative-unix-ms-display",
            &parity_source(
                "time.from_unix_ms(-1).to_string()",
                "use core.time as time",
            ),
        ),
        "1969-12-31 23:59:59 UTC"
    );
    assert_eq!(
        check_aot_comptime(
            "measurement/interpolation-display",
            "use core.science.measurement as measurement\ncomptime value = measurement.from(12.5, 0.25)\ncomptime expected = \"{value}\"\n\nfn run() {\n    actual :: measurement.from(12.5, 0.25)\n    print(expected)\n    print(actual)\n}\n",
        ),
        "12.5 ± 0.25"
    );
}

#[test]
fn rustc_backed_pivot_sum_invokes_capturing_closures_exactly() {
    assert_eq!(
        check_aot_comptime(
            "data/pivot-sum-closures",
            &parity_source(PIVOT_EXPR, PIVOT_DECLS),
        ),
        "pA|x:2:4.0:2.0|pB|y:1:5.0:5.0"
    );
}

#[test]
fn rustc_backed_scalar_value_methods_match_all_execution_tiers_exactly() {
    let source = parity_source(SCALAR_EXPR, SCALAR_DECLS);
    assert_eq!(
        check_aot_comptime("scalar/value-methods", &source),
        SCALAR_EXPECTED
    );
    check_dev_tiers("scalar", &source, SCALAR_EXPECTED);
}

#[test]
fn rustc_backed_seeded_rng_methods_match_all_execution_tiers_exactly() {
    let source = parity_source("rng_view()", RNG_DECLS);
    assert_eq!(check_aot_comptime("rng/all-methods", &source), RNG_EXPECTED);
    // `core.random` keeps its ambient-effect E2201 boundary. `try_anyway`
    // proves the seeded handle itself is interpreter-resident; default dev
    // proves its normal AOT fallback remains byte-identical.
    check_dev_tiers_with_boundary("rng", &source, RNG_EXPECTED, true);
}
