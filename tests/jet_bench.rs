//! `jet bench` target, filter, JSON, and profile contract.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use jet_foundation::JSON::{parse_json, JSONValue};

mod common;

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

#[test]
fn jet_bench_directory_recurses_and_filter_selects_regions() {
    if !common::have_rustc() {
        return;
    }
    let scratch = common::Scratch::new("bench_directory");
    fs::create_dir_all(scratch.join("nested")).unwrap();
    let source = r#"fn run() {}

#Bench("needle") {
    require_eq(1, 1)
}

#Bench("other") {
    require_eq(2, 2)
}
"#;
    fs::write(scratch.join("root.jet"), source).unwrap();
    fs::write(scratch.join("nested/child.jet"), source).unwrap();

    let all = Command::new(jet_bin())
        .args(["bench", "--show-default", "."])
        .current_dir(&scratch.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(all.status.success(), "directory bench failed:\nstdout:\n{}\nstderr:\n{}", String::from_utf8_lossy(&all.stdout), String::from_utf8_lossy(&all.stderr));
    let all_stdout = String::from_utf8_lossy(&all.stdout);
    assert_eq!(all_stdout.matches("ns/iter").count(), 4, "directory bench must run every region: {all_stdout}");
    assert_eq!(all_stdout.matches("== ").count(), 2, "multi-file bench needs one heading per file: {all_stdout}");
    assert!(all_stdout.contains("root.jet::needle"), "root region lost path qualification: {all_stdout}");
    assert!(all_stdout.contains("nested/child.jet::other"), "nested region lost path qualification: {all_stdout}");

    let filtered = Command::new(jet_bin())
        .args(["bench", "--show-default", ".", "--filter=needle"])
        .current_dir(&scratch.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(filtered.status.success(), "filtered directory bench failed:\nstdout:\n{}\nstderr:\n{}", String::from_utf8_lossy(&filtered.stdout), String::from_utf8_lossy(&filtered.stderr));
    let filtered_stdout = String::from_utf8_lossy(&filtered.stdout);
    assert_eq!(filtered_stdout.matches("ns/iter").count(), 2, "filter must select one region per file: {filtered_stdout}");
    assert!(!filtered_stdout.contains("::other"), "filter ran an unselected region: {filtered_stdout}");
}

#[test]
fn jet_bench_json_has_one_release_profile_record_per_region() {
    if !common::have_rustc() {
        return;
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = common::Scratch::new("bench_json");
    let source = root.join("examples/features/tooling/bench.jet");
    fs::copy(source, scratch.join("bench.jet")).unwrap();

    let output = Command::new(jet_bin())
        .args(["bench", "--show-default", "bench.jet", "--json"])
        .current_dir(&scratch.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(output.status.success(), "JSON bench failed:\nstdout:\n{}\nstderr:\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let records: Vec<_> = stdout
        .lines()
        .map(|line| parse_json(line).unwrap_or_else(|_| panic!("bench JSON line does not parse: {line}")))
        .collect();
    assert_eq!(records.len(), 2, "bench JSON must emit one record per region: {stdout}");
    for record in records {
        let JSONValue::Object(record) = record else { panic!("bench JSON record is not an object") };
        assert!(matches!(record.get("profile"), Some(JSONValue::String(profile)) if profile == "release"));
        assert!(matches!(record.get("name"), Some(JSONValue::String(name)) if name == "bench.jet::fib(10)" || name == "bench.jet::sum to 100"), "unexpected benchmark record: {record:?}");
        assert!(record.contains_key("mean_ns"), "JSON record missing timing field: {record:?}");
    }
}
