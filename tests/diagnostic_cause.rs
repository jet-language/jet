use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use jet_foundation::JSON::{parse_json, JSONValue};

fn report_object(value: &JSONValue) -> &HashMap<String, JSONValue> {
    let JSONValue::Object(report) = value else {
        panic!("report is not an object: {value:?}");
    };
    report
}

fn report_string<'a>(report: &'a HashMap<String, JSONValue>, field: &str) -> &'a str {
    let JSONValue::String(value) = &report[field] else {
        panic!("report field `{field}` is not a string: {:?}", report[field]);
    };
    value
}

fn report_array<'a>(report: &'a HashMap<String, JSONValue>, field: &str) -> &'a [JSONValue] {
    let JSONValue::Array(value) = &report[field] else {
        panic!("report field `{field}` is not an array: {:?}", report[field]);
    };
    value
}

fn report_number(report: &HashMap<String, JSONValue>, field: &str) -> i64 {
    let JSONValue::Number(value) = report[field] else {
        panic!("report field `{field}` is not an integer: {:?}", report[field]);
    };
    value
}

fn run_check(path: &Path) -> (std::process::ExitStatus, String, Vec<JSONValue>) {
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["check", path.to_str().expect("fixture path is UTF-8"), "--json"])
        .env("NO_COLOR", "1")
        .output()
        .expect("jet check should start");
    let stderr = String::from_utf8(output.stderr).expect("jet report is UTF-8");
    let reports = stderr
        .lines()
        .map(|line| parse_json(line).expect("each report line is JSON"))
        .collect();
    (output.status, stderr, reports)
}

fn scratch_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jet_report_cause_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create report-cause scratch directory");
    dir
}

#[test]
fn source_cause_chain_selects_one_root_and_clears_dependents() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/ui/diagnostic_cause_chain.jet");
    let source = fs::read_to_string(&fixture).expect("read cause-chain fixture");
    let dir = scratch_dir();
    let broken = dir.join("broken.jet");
    fs::write(&broken, &source).expect("write broken fixture");

    let (status, _stderr, reports) = run_check(&broken);
    assert_eq!(status.code(), Some(1));
    assert_eq!(reports.len(), 11, "one root plus ten dependents");

    let roots: Vec<_> = reports
        .iter()
        .map(report_object)
        .filter(|report| report_array(report, "cause").is_empty())
        .collect();
    assert_eq!(roots.len(), 1, "machine cause data identifies one root");
    assert_eq!(report_string(roots[0], "code"), "E0956");
    assert_eq!(report_number(roots[0], "clears"), 10);

    for report in reports.iter().map(report_object).filter(|report| {
        !report_array(report, "cause").is_empty()
    }) {
        assert_eq!(report_string(report, "code"), "E2710");
        let cause = report_array(report, "cause");
        assert_eq!(cause.len(), 1);
        assert!(matches!(&cause[0], JSONValue::String(value) if value == "E0956"));
        assert_eq!(report_number(report, "clears"), 0);
    }

    let fixed = source.replace("    x :: undefined_name\n", "");
    assert_ne!(fixed, source);
    fs::write(&broken, fixed).expect("write fixed fixture");
    let (status, stderr, reports) = run_check(&broken);
    assert!(status.success(), "fixed root should pass: {stderr}");
    assert!(reports.is_empty(), "fixed root should clear all reports");
    assert!(stderr.is_empty());

    fs::remove_dir_all(dir).expect("remove report-cause scratch directory");
}
