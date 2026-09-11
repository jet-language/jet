//! M6 phase 2: `jet test` output shape and fail-then-fix flow.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use jet_foundation::DataTree::DataTree;
use jet_foundation::JSON::parse_json;

mod common;
#[path = "tir_support/mod.rs"]
mod tir_support;
use common::have_rustc;

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

#[test]
fn jet_test_example_output() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    assert!(jet.exists(), "build the jet binary first (cargo build)");
    let have_rustc = have_rustc();
    if !have_rustc {
        eprintln!("note: rustc not found; skipping jet test integration");
        return;
    }

    let example = root.join("examples/features/tooling/tests.jet");
    let out = Command::new(&jet)
        .arg("test").arg("--show-default").arg("--capture=all")
        .arg(&example)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "jet test examples/features/tooling/tests.jet failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let expected =
        fs::read_to_string(root.join("examples/features/expected/tooling/tests.test.out"))
            .expect("examples/features/expected/tooling/tests.test.out");
    assert_eq!(String::from_utf8_lossy(&out.stdout), expected);
}

#[test]
fn jet_test_expected_fail_tracks_failure_and_unexpected_pass() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    if !have_rustc() || !jet.exists() {
        return;
    }
    let example = root.join("examples/features/tooling/expected_fail.jet");
    let expected =
        fs::read_to_string(root.join("examples/features/expected/tooling/expected_fail.test.out"))
            .expect("expected_fail.test.out");

    let green = Command::new(&jet)
        .args(["test", "--show-default", "--capture=all", "--serial", "--filter=known"])
        .arg(&example)
        .output()
        .unwrap();
    assert!(
        green.status.success(),
        "expected failure should stay green:\n{}",
        String::from_utf8_lossy(&green.stdout)
    );
    assert_eq!(
        String::from_utf8_lossy(&green.stdout),
        "known bug remains expected-fail: expected-fail\n0 passed, 0 failed, 0 skipped, 1 expected-fail\n"
    );

    let out = Command::new(&jet)
        .args(["test", "--show-default", "--capture=all", "--serial"])
        .arg(&example)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "unexpected pass must fail the run");
    assert_eq!(String::from_utf8_lossy(&out.stdout), expected);

    let json = Command::new(&jet)
        .args(["test", "--json", "--serial"])
        .arg(&example)
        .output()
        .unwrap();
    assert_eq!(
        json.status.code(),
        Some(1),
        "JSON run must preserve unexpected-pass failure"
    );
    let json = String::from_utf8_lossy(&json.stdout);
    assert!(
        json.contains("\"expectedFailures\":1"),
        "missing expected-failure count: {json}"
    );
    assert!(
        json.contains("\"unexpectedPasses\":1"),
        "missing unexpected-pass count: {json}"
    );
}

#[test]
fn jet_test_package_collects_imported_module_tests() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    if !have_rustc() || !jet.exists() {
        return;
    }
    let package = root.join("examples/features/tooling/test_package_modules");
    let out = Command::new(&jet)
        .arg("test").arg("--show-default").arg("--capture=all")
        .arg(&package)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "package test failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let expected = fs::read_to_string(
        root.join("examples/features/expected/tooling/test_package_modules.test.out"),
    )
    .expect("test_package_modules.test.out");
    assert_eq!(String::from_utf8_lossy(&out.stdout), expected);
}

#[test]
fn concurrent_jet_test_same_file_is_process_isolated() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    if !have_rustc() || !jet.exists() {
        return;
    }
    let example = root.join("examples/features/tooling/tests.jet");
    let mut children = Vec::new();
    for _ in 0..4 {
        children.push(
            Command::new(&jet)
                .arg("test").arg("--show-default").arg("--capture=all")
                .arg(&example)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("spawn concurrent jet test"),
        );
    }
    for child in children {
        let out = child
            .wait_with_output()
            .expect("wait for concurrent jet test");
        assert!(
            out.status.success(),
            "concurrent jet test failed: {}\nstdout: {}\nstderr: {}",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn jet_test_members_example_output() {
    // D-DOTSCOPE1: `.setup` / `.expect_fail` / `.timeout` / `.skip` scope members.
    // The example exercises all four; the whole-test `.skip` reports `skip` and the
    // summary carries a skipped count.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    let have_rustc = have_rustc();
    if !have_rustc || !jet.exists() {
        return;
    }
    let example = root.join("examples/features/tooling/test_members.jet");
    let out = Command::new(&jet)
        .arg("test").arg("--show-default").arg("--capture=all")
        .arg(&example)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "scope-member example failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let expected =
        fs::read_to_string(root.join("examples/features/expected/tooling/test_members.test.out"))
            .expect("test_members.test.out");
    assert_eq!(String::from_utf8_lossy(&out.stdout), expected);
}

#[test]
fn jet_scope_expect_fail_passing_region_fails() {
    // D-DOTSCOPE1: an `.expect_fail` region that completes cleanly fails the test.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    let have_rustc = have_rustc();
    if !have_rustc || !jet.exists() {
        return;
    }
    let fixture = root.join("tests/fixtures/scope_expect_fail_passes.jet");
    let out = Command::new(&jet)
        .arg("test").arg("--show-default").arg("--capture=all")
        .arg(&fixture)
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "a passing expect_fail region must fail"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stdout.contains("FAIL"), "expected a FAIL line:\n{}", stdout);
    assert!(stderr.contains("[E3001]"), "expected a registered test failure:\n{stderr}");
}

#[test]
fn jet_scope_expect_fail_asserts_runtime_code() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    if !have_rustc() || !jet.exists() {
        return;
    }
    let fixture = root.join("tests/fixtures/scope_expect_fail_code.jet");
    let out = Command::new(&jet)
        .args([
            "test",
            "--show-default",
            fixture.to_str().expect("fixture path"),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "specific expect_fail test failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("expect a specific runtime stop: pass"),
        "missing passing test output: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn jet_scope_setup_failure_fails_test() {
    // D-DOTSCOPE1: a failure inside `.setup` fails the test on the normal path.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    let have_rustc = have_rustc();
    if !have_rustc || !jet.exists() {
        return;
    }
    let fixture = root.join("tests/fixtures/scope_setup_fail.jet");
    let out = Command::new(&jet)
        .arg("test").arg("--show-default").arg("--capture=all")
        .arg(&fixture)
        .output()
        .unwrap();
    assert!(!out.status.success(), "a failing setup must fail the test");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stdout.contains("FAIL"), "expected a FAIL line:\n{}", stdout);
    assert!(
        stderr.contains("setup blew up"),
        "expected the setup failure message:\n{}",
        stderr
    );
}

#[test]
fn jet_scope_timeout_exceeded_fails() {
    // D-DOTSCOPE1: a `.timeout` region over its (1ns) budget fails the test
    // post-hoc — the region runs, then its elapsed time is checked.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    let have_rustc = have_rustc();
    if !have_rustc || !jet.exists() {
        return;
    }
    let fixture = root.join("tests/fixtures/scope_timeout_exceeded.jet");
    let out = Command::new(&jet)
        .arg("test").arg("--show-default").arg("--capture=all")
        .arg(&fixture)
        .output()
        .unwrap();
    assert!(!out.status.success(), "an over-budget timeout must fail");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stdout.contains("FAIL"), "expected a FAIL line:\n{}", stdout);
    assert!(stderr.contains("[E3001]"), "expected a registered timeout failure:\n{stderr}");
}

#[test]
fn jet_test_fail_then_fixed() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    let have_rustc = have_rustc();
    if !have_rustc || !jet.exists() {
        return;
    }

    let fail = root.join("tests/fixtures/test_fail.jet");
    let fixed = root.join("tests/fixtures/test_fail.fixed.jet");

    let bad = Command::new(&jet)
        .arg("test").arg("--show-default").arg("--capture=all")
        .arg(&fail)
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(1), "a test failure is not a compiler ICE");
    assert!(
        String::from_utf8_lossy(&bad.stdout).contains("FAIL"),
        "expected a FAIL line, got: {}",
        String::from_utf8_lossy(&bad.stdout)
    );
    assert!(
        String::from_utf8_lossy(&bad.stderr).contains("Stop [E3001]"),
        "assert_eq should print the registered test report"
    );
    let good = Command::new(&jet)
        .arg("test").arg("--show-default").arg("--capture=all")
        .arg(&fixed)
        .output()
        .unwrap();
    assert!(good.status.success());
    assert!(
        String::from_utf8_lossy(&good.stdout).contains("pass"),
        "fixed tests should pass"
    );
}

#[test]
fn criterion_1_2_3_4_6_testing_file_failures_are_typed_and_path_bearing() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    if !have_rustc() || !jet.exists() {
        return;
    }

    let failing = root.join("tests/fixtures/testing_failure_reports.jet");
    let fixed = root.join("tests/fixtures/testing_failure_reports.fixed.jet");
    let bad = Command::new(&jet)
        .arg("test")
        .arg(&failing)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&bad.stdout);
    let stderr = String::from_utf8_lossy(&bad.stderr);

    assert!(!bad.status.success());
    assert_eq!(stdout.matches(": FAIL").count(), 4, "stdout: {stdout}");
    assert!(
        stderr.contains("golden file is missing: tests/fixtures/testing_failure_reports.missing")
    );
    assert!(stderr.contains("golden file cannot be read: tests/fixtures"));
    assert!(stderr.contains("golden file differs: tests/fixtures/testing_failure_reports.golden"));
    assert!(stderr.contains("fixture is missing: tests/fixtures/testing_failure_reports.missing"));
    assert!(stderr.contains("--- expected tests/fixtures/testing_failure_reports.golden"));
    assert!(stderr.contains("+++ actual tests/fixtures/testing_failure_reports.golden"));
    let removal = stderr
        .find("-expected\n")
        .expect("mismatch report must show expected removal");
    let addition = stderr
        .find("+actual\n")
        .expect("mismatch report must show actual addition");
    assert!(
        removal < addition,
        "diff order must be deterministic: {stderr}"
    );
    assert_eq!(
        stderr.matches("Stop [E3001]").count(),
        4,
        "stderr: {stderr}"
    );
    assert_eq!(stderr.matches("-->").count(), 4, "stderr: {stderr}");
    let snapshot = fs::read_to_string(root.join("tests/fixtures/testing_failure_reports.stderr"))
        .expect("testing_failure_reports.stderr");
    let normalized_stderr = stderr.replace(root.to_str().expect("manifest path"), "<repo>");
    assert_eq!(
        normalized_stderr, snapshot,
        "typed testing report snapshot drifted"
    );

    let good = Command::new(&jet).arg("test").arg(&fixed).output().unwrap();
    assert!(
        good.status.success(),
        "fixed testing helpers failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&good.stdout),
        String::from_utf8_lossy(&good.stderr)
    );
}

#[test]
fn release_test_uses_aot_tier_marker() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    if !have_rustc() || !jet.exists() {
        return;
    }
    let example = root.join("examples/features/tooling/property_tests.jet");
    let out = Command::new(&jet)
        .args(["test", "--release", "--trace-tiers"])
        .arg(&example)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "release property test failed:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("tier aot profile=release"),
        "release test did not report its AOT tier:\n{stdout}"
    );
    assert_eq!(
        stdout.matches("tier aot profile=release").count(),
        1,
        "release test must report exactly one AOT tier marker:\n{stdout}"
    );
    let combined = format!("{stdout}{stderr}").to_ascii_lowercase();
    assert!(
        !combined.contains("tier jit"),
        "release test reported a JIT tier:\n{combined}"
    );
    assert!(
        !combined.contains("tier interpreter"),
        "release test reported an interpreter tier:\n{combined}"
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PropertySample {
    case_index: u64,
    seed: u64,
    input: i64,
}

fn parse_property_samples(output: &[u8], engine: &str) -> Vec<PropertySample> {
    let stdout = String::from_utf8_lossy(output);
    stdout
        .lines()
        .filter(|line| line.starts_with("JET_PROP_SAMPLE "))
        .map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            assert_eq!(fields.len(), 5, "malformed property sample marker: {line}");
            assert_eq!(fields[0], "JET_PROP_SAMPLE");
            let expected_engine = format!("engine={engine}");
            assert_eq!(fields[1], expected_engine.as_str());
            let case_index = fields[2]
                .strip_prefix("case=")
                .expect("property sample case field")
                .parse()
                .expect("property sample case index");
            let seed = fields[3]
                .strip_prefix("seed=")
                .expect("property sample seed field")
                .parse()
                .expect("property sample seed");
            let input = fields[4]
                .strip_prefix("input=")
                .expect("property sample input field")
                .parse()
                .expect("property sample input");
            PropertySample {
                case_index,
                seed,
                input,
            }
        })
        .collect()
}

fn property_sample_digest(samples: &[PropertySample]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for sample in samples {
        let line = format!("{}:{}:{}\n", sample.case_index, sample.seed, sample.input);
        for byte in line.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    format!("{hash:016x}")
}

fn property_distribution(samples: &[PropertySample]) -> [usize; 4] {
    let explicit_landmarks = [
        0,
        1,
        -1,
        2,
        -2,
        42,
        -42,
        99,
        100,
        255,
        256,
        512,
        1024,
        i64::MIN,
        i64::MAX,
    ];
    let mut counts = [0usize; 4];
    for sample in samples {
        if sample.input == 42 {
            counts[0] += 1;
        }
        if sample.input == 0 || sample.input == 1 || sample.input == -1 {
            counts[1] += 1;
        }
        if sample.input == i64::MIN || sample.input == i64::MAX {
            counts[2] += 1;
        }
        if !explicit_landmarks.contains(&sample.input) {
            counts[3] += 1;
        }
    }
    counts
}

fn report_field<'a>(value: &'a DataTree, key: &str) -> &'a DataTree {
    value
        .get(key)
        .unwrap_or_else(|error| panic!("missing JSON field `{key}`: {error}"))
}

fn report_object<'a>(value: &'a DataTree, key: &str) -> &'a DataTree {
    let field = report_field(value, key);
    if !matches!(field, DataTree::Object(_)) {
        panic!("JSON field `{key}` must be an object: {field:?}");
    }
    field
}

fn report_array<'a>(value: &'a DataTree, key: &str) -> &'a [DataTree] {
    report_field(value, key)
        .as_array()
        .unwrap_or_else(|error| panic!("JSON field `{key}` must be an array: {error}"))
}

fn report_text(value: &DataTree) -> &str {
    value
        .as_str()
        .unwrap_or_else(|error| panic!("JSON field must be text: {error}"))
}

fn report_int(value: &DataTree) -> i64 {
    match value {
        DataTree::Int(value) => *value,
        DataTree::Number(value) => value
            .parse()
            .unwrap_or_else(|_| panic!("JSON field must be an integer: {value}")),
        _ => panic!("JSON field must be an integer: {value:?}"),
    }
}

fn report_float(value: &DataTree) -> f64 {
    match value {
        DataTree::Float(value) => *value,
        DataTree::Int(value) => *value as f64,
        DataTree::Number(value) => value
            .parse()
            .unwrap_or_else(|_| panic!("JSON field must be numeric: {value}")),
        _ => panic!("JSON field must be numeric: {value:?}"),
    }
}

#[test]
fn property_generator_distribution_report_is_reproducible() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    if !have_rustc() || !jet.exists() {
        return;
    }
    let report_path = root.join("tests/fixtures/property-generator-distribution.json");
    let report_source = fs::read_to_string(&report_path).expect("property distribution report");
    let report = parse_json(&report_source).expect("valid property report JSON");
    if report.as_object().is_err() {
        panic!("property report must be an object");
    }
    assert_eq!(report_int(report_field(&report, "schema")), 1);

    let predicates = report_array(&report, "predicates");
    let predicate_ids: Vec<&str> = predicates
        .iter()
        .map(|predicate| report_text(report_field(predicate, "id")))
        .collect();
    assert_eq!(
        predicate_ids,
        [
            "int_eq_42",
            "int_small_anchor",
            "int_extreme",
            "int_random_fallback",
        ]
    );
    let predicate_expressions: Vec<&str> = predicates
        .iter()
        .map(|predicate| report_text(report_field(predicate, "expression")))
        .collect();
    assert_eq!(
        predicate_expressions,
        [
            "x == 42",
            "x == 0 || x == 1 || x == -1",
            "x == Int.MIN || x == Int.MAX",
            "x is outside the 15 explicit i64 landmarks",
        ]
    );

    let engines = report_array(&report, "engines");
    assert!(engines.iter().any(|engine| report_text(engine) == "jet_test"));
    assert!(engines.iter().any(|engine| report_text(engine) == "jet_fuzz"));
    let comparison = report_object(&report, "comparison");
    for field in [
        "predicates",
        "seeds",
        "sample_counts",
        "hit_rates",
        "sample_stream",
    ] {
        assert_eq!(report_text(report_field(comparison, field)), "identical");
    }

    let seeds = report_object(&report, "seeds");
    let sample_counts = report_object(&report, "sample_counts");
    let test_seed = report_int(report_field(seeds, "jet_test"));
    let fuzz_seed = report_int(report_field(seeds, "jet_fuzz"));
    assert_eq!(test_seed, fuzz_seed, "engines use different base seeds");
    let seed = u64::try_from(test_seed).expect("property seed must fit u64");
    let test_count = report_int(report_field(sample_counts, "jet_test"));
    let fuzz_count = report_int(report_field(sample_counts, "jet_fuzz"));
    assert_eq!(
        test_count, fuzz_count,
        "engines use different sample counts"
    );
    let sample_count = usize::try_from(test_count).expect("sample count must fit usize");

    let hit_rates = report_object(&report, "hit_rates");
    let test_rates = report_object(hit_rates, "jet_test");
    let fuzz_rates = report_object(hit_rates, "jet_fuzz");
    let predicate_names = [
        "int_eq_42",
        "int_small_anchor",
        "int_extreme",
        "int_random_fallback",
    ];
    for id in predicate_names {
        let test_rate = report_float(report_field(test_rates, id));
        let fuzz_rate = report_float(report_field(fuzz_rates, id));
        assert_eq!(test_rate, fuzz_rate, "engines disagree for {id}");
    }

    let expected_hit_counts = report_object(&report, "hit_counts");
    let expected_digests = report_object(&report, "sample_digests");
    let card_ids = report_array(&report, "finding_card_ids");
    assert!(card_ids.iter().any(|id| report_text(id) == "#1905"));
    let findings = report_array(&report, "findings");
    assert!(!findings.is_empty(), "distribution findings must be carded");
    for finding in findings {
        let ids = report_array(finding, "finding_card_ids");
        assert!(ids.iter().any(|id| report_text(id) == "#1905"));
    }

    let fixture = root.join("tests/fixtures/property-generator-distribution.jet");
    let seed_arg = seed.to_string();
    let test_out = Command::new(&jet)
        .args(["test", "--serial"])
        .arg(&fixture)
        .env("JET_PROP_SEED", &seed_arg)
        .env("JET_PROP_TRACE", "1")
        .output()
        .expect("run jet test property generator fixture");
    assert!(
        test_out.status.success(),
        "jet test property distribution failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&test_out.stdout),
        String::from_utf8_lossy(&test_out.stderr)
    );

    let corpus = common::unique_tmp("jet_property_generator_distribution");
    let _ = fs::remove_dir_all(&corpus);
    fs::create_dir_all(&corpus).expect("create empty fuzz corpus");
    let iterations_arg = format!("--iterations={sample_count}");
    let corpus_arg = format!("--corpus={}", corpus.display());
    let fuzz_out = Command::new(&jet)
        .args(["test", "--grade=generated"])
        .arg(&iterations_arg)
        .arg(format!("--seed={seed}"))
        .arg(&corpus_arg)
        .arg(&fixture)
        .arg("generator_contract")
        .env("JET_PROP_TRACE", "1")
        .output()
        .expect("run generated property test fixture");
    let _ = fs::remove_dir_all(&corpus);
    assert!(
        fuzz_out.status.success(),
        "generated property distribution failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&fuzz_out.stdout),
        String::from_utf8_lossy(&fuzz_out.stderr)
    );

    let test_samples = parse_property_samples(&test_out.stdout, "jet_test");
    let fuzz_samples = parse_property_samples(&fuzz_out.stdout, "jet_fuzz");
    assert_eq!(test_samples.len(), sample_count);
    assert_eq!(fuzz_samples.len(), sample_count);
    for (expected_case, sample) in test_samples.iter().enumerate() {
        assert_eq!(sample.case_index, expected_case as u64);
    }
    for (expected_case, sample) in fuzz_samples.iter().enumerate() {
        assert_eq!(sample.case_index, expected_case as u64);
    }
    assert_eq!(
        test_samples, fuzz_samples,
        "jet test and generated tests produced different case seeds or inputs"
    );

    let test_counts = property_distribution(&test_samples);
    let fuzz_counts = property_distribution(&fuzz_samples);
    assert_eq!(
        test_counts, fuzz_counts,
        "engines disagree on predicate hits"
    );
    for (index, id) in predicate_names.iter().enumerate() {
        let test_counts_by_id = report_object(expected_hit_counts, "jet_test");
        let expected_test_count = report_int(report_field(test_counts_by_id, id));
        assert_eq!(
            usize::try_from(expected_test_count).expect("test hit count must fit usize"),
            test_counts[index],
            "wrong hit count for {id}"
        );

        let fuzz_counts_by_id = report_object(expected_hit_counts, "jet_fuzz");
        let expected_fuzz_count = report_int(report_field(fuzz_counts_by_id, id));
        assert_eq!(
            usize::try_from(expected_fuzz_count).expect("fuzz hit count must fit usize"),
            fuzz_counts[index],
            "wrong fuzz hit count for {id}"
        );

        let expected_test_rate = report_float(report_field(test_rates, id));
        let expected_fuzz_rate = report_float(report_field(fuzz_rates, id));
        let observed_rate = test_counts[index] as f64 / sample_count as f64;
        assert!(
            (observed_rate - expected_test_rate).abs() < 1e-12,
            "wrong test rate for {id}"
        );
        assert!(
            (observed_rate - expected_fuzz_rate).abs() < 1e-12,
            "wrong fuzz rate for {id}"
        );
    }

    for (engine, samples) in [("jet_test", &test_samples), ("jet_fuzz", &fuzz_samples)] {
        let expected_digest = report_text(report_field(expected_digests, engine));
        assert_eq!(
            property_sample_digest(samples),
            expected_digest,
            "generator contract drifted for {engine}"
        );
    }
}

#[test]
fn jet_property_test_passes() {
    // D-TEST1: a parameterized `#Test fn` is a property test. The example's three
    // properties all hold, so every line passes and the run succeeds.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    let have_rustc = have_rustc();
    if !have_rustc || !jet.exists() {
        return;
    }
    let example = root.join("examples/features/tooling/property_tests.jet");
    let out = Command::new(&jet)
        .arg("test").arg("--show-default").arg("--capture=all")
        .arg(&example)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "property test example failed:\nstdout: {}\nstderr: {}",
        stdout,
        String::from_utf8_lossy(&out.stderr)
    );
    for needle in [
        "reverse_twice_is_identity: pass",
        "reverse_keeps_length: pass",
        "reverse of a known list: pass",
        "3 passed, 0 failed",
    ] {
        assert!(stdout.contains(needle), "missing `{}`:\n{}", needle, stdout);
    }
}

#[test]
fn jet_property_test_shrinks_failure() {
    // D-TEST1: a failing property is shrunk to a minimal counterexample. The
    // fixture asserts `n < 50`; the runner must report the boundary value `50`.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    let have_rustc = have_rustc();
    if !have_rustc || !jet.exists() {
        return;
    }
    let fixture = root.join("tests/fixtures/prop_shrink.jet");
    let out = Command::new(&jet)
        .arg("test").arg("--show-default").arg("--capture=all")
        .arg(&fixture)
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "a failing property must exit nonzero"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stdout.contains("always_small: FAIL"),
        "expected a FAIL line:\n{}",
        stdout
    );
    assert!(
        stderr.contains("n = 50"),
        "expected the shrunk counterexample `n = 50`:\n{}",
        stderr
    );
}

#[test]
fn jet_property_test_rejects_ungeneratable_param() {
    // D-TEST1: a property-test parameter whose type has no generator fires E0613.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }
    let fixture = root.join("tests/fixtures/prop_bad_type.jet");
    let out = Command::new(&jet)
        .arg("test").arg("--show-default").arg("--capture=all")
        .arg(&fixture)
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "an ungeneratable param must be rejected"
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(combined.contains("E0613"), "expected E0613:\n{}", combined);
}

#[test]
fn jet_doctest_passes() {
    // D-TEST4: `jet test` discovers and runs `///` doctests. The example's
    // `// =>` expectations all hold.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    let have_rustc = have_rustc();
    if !have_rustc || !jet.exists() {
        return;
    }
    let example = root.join("examples/features/comptime/doctests.jet");
    let out = Command::new(&jet)
        .arg("test").arg("--show-default").arg("--capture=all")
        .arg(&example)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "doctest example failed:\nstdout: {}\nstderr: {}",
        stdout,
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains("doctest at") && stdout.contains("pass"),
        "no doctest pass line:\n{}",
        stdout
    );
}

#[test]
fn jet_doctest_mismatch_fires_e2901() {
    // D-TEST4: a `// =>` claim that doesn't match the produced value fires E2901
    // and fails the run.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    let have_rustc = have_rustc();
    if !have_rustc || !jet.exists() {
        return;
    }
    let fixture = root.join("tests/fixtures/doctest_fail.jet");
    let out = Command::new(&jet)
        .arg("test").arg("--show-default").arg("--capture=all")
        .arg(&fixture)
        .output()
        .unwrap();
    assert!(!out.status.success(), "a wrong doctest must exit nonzero");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E2901"), "expected E2901:\n{}", stderr);
}

#[test]
fn jet_test_coverage_reports_hit_and_miss() {
    // D-COV1: `jet test --coverage` reports function and branch coverage. The
    // fixture calls `used` from a test but never `unused`, so the report must
    // mark one HIT and one MISS for functions.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    let have_rustc = have_rustc();
    if !have_rustc || !jet.exists() {
        return;
    }
    let fixture = root.join("tests/fixtures/coverage.jet");
    let out = Command::new(&jet)
        .arg("test").arg("--show-default").arg("--capture=all")
        .arg("--coverage")
        .arg(&fixture)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "coverage run failed:\n{}", stdout);
    assert!(
        stdout.contains("HIT") && stdout.contains("used"),
        "missing HIT used:\n{}",
        stdout
    );
    assert!(
        stdout.contains("MISS") && stdout.contains("unused"),
        "missing MISS unused:\n{}",
        stdout
    );
    assert!(
        stdout.contains("1/2 functions covered"),
        "wrong summary:\n{}",
        stdout
    );
}

#[test]
fn jet_test_coverage_reports_branch_taken_and_not_taken_in_text_and_json() {
    // D-COV1: text and JSON must expose the same stable branch ID and outcome
    // counts, including the uncovered side of the fixture's `if`.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    if !have_rustc() || !jet.exists() {
        return;
    }
    let fixture = root.join("tests/fixtures/coverage.jet");
    let run = |json: bool| {
        let mut command = Command::new(&jet);
        command.arg("test").arg("--coverage");
        if json {
            command.arg("--json");
        }
        command.arg(&fixture).output().unwrap()
    };
    let text_output = run(false);
    assert!(
        text_output.status.success(),
        "coverage text run failed:\n{}\n{}",
        String::from_utf8_lossy(&text_output.stdout),
        String::from_utf8_lossy(&text_output.stderr)
    );
    let fixture_path = fixture.to_string_lossy().into_owned();
    let qualified_fixture_path = format!(
        "{}::coverage.jet::",
        root.join("tests/fixtures").display()
    );
    let text = String::from_utf8_lossy(&text_output.stdout)
        .replace(&fixture_path, "tests/fixtures/coverage.jet")
        .replace(&qualified_fixture_path, "main::");
    let compact = |value: &str| value.split_whitespace().collect::<Vec<_>>().join(" ");
    let text = compact(&text);
    assert!(
        text.contains("1/2 branches covered (50%)"),
        "missing branch coverage summary:\n{text}"
    );

    let json_output = run(true);
    assert!(
        json_output.status.success(),
        "coverage JSON run failed:\n{}\n{}",
        String::from_utf8_lossy(&json_output.stdout),
        String::from_utf8_lossy(&json_output.stderr)
    );
    let json = String::from_utf8_lossy(&json_output.stdout)
        .replace(&fixture_path, "tests/fixtures/coverage.jet")
        .replace(&qualified_fixture_path, "main::");
    let report = json
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| parse_json(line).expect("coverage command emits valid JSON records"))
        .find(|report| report.get("coverage").is_ok())
        .expect("coverage status record");
    let coverage = report_object(&report, "coverage");
    let branches = report_array(coverage, "branches");
    assert_eq!(branches.len(), 2, "test-harness branches must not count as source coverage");
    let outcome = |name: &str| {
        branches
            .iter()
            .find(|branch| report_text(report_field(branch, "outcome")) == name)
            .unwrap_or_else(|| panic!("missing branch outcome {name}: {branches:?}"))
    };
    let taken = outcome("taken");
    let not_taken = outcome("not-taken");
    let id = report_text(report_field(taken, "id"));
    assert_eq!(id, report_text(report_field(not_taken, "id")));
    for (branch, outcome, hits, state) in [
        (taken, "taken", 1, "HIT"),
        (not_taken, "not-taken", 0, "MISS"),
    ] {
        assert_eq!(report_text(report_field(branch, "function")), "used");
        assert_eq!(report_int(report_field(branch, "hits")), hits);
        let row = format!("BRANCH {id} {outcome} {state} hits={hits}");
        assert!(text.contains(&compact(&row)), "text and JSON coverage differ:\n{row}\n{text}");
    }
}

#[test]
fn test_target_does_not_reintroduce_retired_command() {
    // D-CLAIM-BENCH1=A: the ordinary test target cannot revive the retired command.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    let example = root.join("examples/features/tooling/test_target/run.jet");
    let out = Command::new(&jet)
        .args(["bench", example.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("jet test --measure"));
}

#[test]
fn jet_new_creates_project() {
    let jet = jet_bin();
    let dir = common::unique_tmp("jet_new_test");
    let parent = dir.parent().expect("scratch project parent").to_path_buf();
    let _ = fs::remove_dir_all(&dir);
    let name = dir.file_name().unwrap().to_string_lossy();
    let out = Command::new(&jet)
        .arg("new")
        .arg(&*name)
        .current_dir(&parent)
        .output()
        .unwrap();
    assert!(out.status.success(), "jet new failed");
    assert!(
        dir.join("package.jet").exists(),
        "package.jet must be created by jet new"
    );
    let run = dir.join("run.jet");
    assert!(run.exists(), "run.jet must be created by jet new");
    assert!(
        !dir.join("main.jet").exists(),
        "jet new must not create main.jet"
    );
    let source = fs::read_to_string(&run).unwrap();
    assert!(
        source.contains("#CLI")
            && source.contains("struct GreetingArgs")
            && source.contains("fn run(args: GreetingArgs)")
            && source.contains("print("),
        "jet new must emit the typed CLI starter: {source}"
    );
    let explicit = Command::new(&jet)
        .args(["run", "run.jet"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        explicit.status.success(),
        "explicit run.jet target failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&explicit.stdout),
        String::from_utf8_lossy(&explicit.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&explicit.stdout), "hello, world\n");
    let bare = Command::new(&jet)
        .arg("run")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        bare.status.success(),
        "bare run target failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&bare.stdout),
        String::from_utf8_lossy(&bare.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&bare.stdout), "hello, world\n");
    let duplicate = Command::new(&jet)
        .args(["new", &*name])
        .current_dir(&parent)
        .output()
        .unwrap();
    assert!(
        !duplicate.status.success(),
        "jet new must reject an existing project"
    );
    assert!(dir.join(".gitignore").exists());
    let _ = fs::remove_dir_all(&dir);
}

// D-TESTKIT1=A (c308 pass 2): directory recursion, filter/shuffle/serial, and
// generated tests (corpus persistence, minimization, deterministic seeded PRNG).

#[test]
fn jet_test_dir_recurses_into_subdirectories() {
    // Gap #2: `jet test <dir>` used to read only the immediate directory
    // (Source/CmdCompile.rs:711-721); it must now walk subdirectories too.
    let jet = jet_bin();
    let have_rustc = have_rustc();
    if !have_rustc || !jet.exists() {
        return;
    }
    let dir = common::unique_tmp("jet_test_recurse");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("nested/deeper")).unwrap();
    tir_support::write_test_package(&dir, tir_support::TIR_TEST_PACKAGE);
    fs::write(dir.join("a.jet"), "#Test(\"top level\") { assert(true) }\n").unwrap();
    fs::write(
        dir.join("nested/b.jet"),
        "#Test(\"one level down\") { assert(true) }\n",
    )
    .unwrap();
    fs::write(
        dir.join("nested/deeper/c.jet"),
        "#Test(\"two levels down\") { assert(true) }\n",
    )
    .unwrap();
    let out = Command::new(&jet)
        .arg("test").arg("--show-default").arg("--capture=all")
        .arg(&dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "recursive test dir run failed:\nstdout: {}\nstderr: {}",
        stdout,
        String::from_utf8_lossy(&out.stderr)
    );
    for needle in [
        "top level: pass",
        "one level down: pass",
        "two levels down: pass",
    ] {
        assert!(stdout.contains(needle), "missing `{}`:\n{}", needle, stdout);
    }
    let _ = fs::remove_dir_all(&dir);
}

/// #2066: a real `jet new` project in a temp directory — the out-of-the-box
/// shape (`package.jet`, `run.jet` with no tests) each bare-`jet test` case
/// below adds its own member files to.
fn bare_package_project(label: &str, jet: &Path) -> PathBuf {
    let dir = common::unique_tmp(&format!("jet_test_{label}"));
    let parent = dir.parent().expect("scratch project parent").to_path_buf();
    let _ = fs::remove_dir_all(&dir);
    let name = dir.file_name().unwrap().to_string_lossy().to_string();
    let created = Command::new(jet)
        .arg("new")
        .arg(&name)
        .current_dir(parent)
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "jet new failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&created.stdout),
        String::from_utf8_lossy(&created.stderr)
    );
    fs::write(dir.join("run.jet"), "fn run() {}\n").unwrap();
    dir
}

#[test]
fn bare_jet_test_discovers_tests_in_every_package_module() {
    // #2066: `jet test` with no target used to build only the resolved entry
    // file's harness, so a fresh project with its tests in `math.jet` reported
    // E0601 "no #Test blocks found" and exited 1.
    let jet = jet_bin();
    if !have_rustc() || !jet.exists() {
        return;
    }
    let dir = bare_package_project("bare_package", &jet);
    fs::write(
        dir.join("math.jet"),
        "fn double(n: Int) Int -> (n * 2)\n\n#Test(\"double returns twice the input\") {\n    assert_eq(double(3), 6)\n}\n",
    )
    .unwrap();
    let out = Command::new(&jet)
        .arg("test")
        .current_dir(&dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "bare jet test failed in a fresh project:\nstdout: {}\nstderr: {}",
        stdout,
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains("double returns twice the input: pass"),
        "the sibling module's test never ran:\n{}",
        stdout
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn jet_test_package_directory_aggregates_mixed_modules() {
    let jet = jet_bin();
    if !have_rustc() || !jet.exists() {
        return;
    }
    let dir = bare_package_project("package_directory", &jet);
    fs::write(dir.join("helper.jet"), "fn helper() Int -> 1\n").unwrap();
    fs::write(
        dir.join("math.jet"),
        "#Test(\"mixed package test\") {\n    assert(true)\n}\n",
    )
    .unwrap();
    let out = Command::new(&jet)
        .args(["test", dir.to_str().unwrap()])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "mixed package directory failed:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("mixed package test: pass"),
        "test-bearing module never ran:\n{stdout}"
    );
    assert!(
        !stderr.contains("E0601"),
        "test-free package modules must not emit E0601:\n{stderr}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn jet_test_package_directory_reports_no_tests_once() {
    // #2066 criterion 2: E0601 fires only when the whole package has zero
    // `#Test` blocks — once for the package, not once per member file.
    let jet = jet_bin();
    if !have_rustc() || !jet.exists() {
        return;
    }
    let dir = bare_package_project("bare_testless", &jet);
    fs::write(dir.join("math.jet"), "fn double(n: Int) Int -> (n * 2)\n").unwrap();
    let out = Command::new(&jet)
        .args(["test", dir.to_str().unwrap()])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "a package with no tests must fail:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        stderr
    );
    // The rendered heading carries `[E0601]` once per report; the trailing
    // `jet explain E0601` pointer names the code again, so count headings.
    assert_eq!(
        stderr.matches("[E0601]").count(),
        1,
        "expected exactly one E0601 report for the package:\n{}",
        stderr
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn bare_jet_test_surfaces_a_broken_package_member() {
    // #2066: a member that fails to parse must report its real compile error
    // instead of being silently skipped by package discovery.
    let jet = jet_bin();
    if !have_rustc() || !jet.exists() {
        return;
    }
    let dir = bare_package_project("bare_broken", &jet);
    fs::write(
        dir.join("good.jet"),
        "#Test(\"good member passes\") {\n    assert(true)\n}\n",
    )
    .unwrap();
    fs::write(dir.join("broken.jet"), "fn oops( {\n").unwrap();
    let out = Command::new(&jet)
        .arg("test")
        .current_dir(&dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "a broken package member must fail the run:\nstdout: {}\nstderr: {}",
        stdout,
        stderr
    );
    assert!(
        stderr.contains("broken.jet"),
        "the broken member's error never surfaced:\nstdout: {}\nstderr: {}",
        stdout,
        stderr
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn jet_test_filter_keeps_only_matching_names() {
    // Gap #4: `--filter=<substr>` keeps only tests whose name contains it.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    let have_rustc = have_rustc();
    if !have_rustc || !jet.exists() {
        return;
    }
    let example = root.join("examples/features/tooling/tests.jet");
    let out = Command::new(&jet)
        .arg("test").arg("--show-default").arg("--capture=all")
        .arg("--filter=consistent")
        .arg(&example)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "filtered run failed:\n{}", stdout);
    assert!(
        stdout.contains("double is consistent: pass"),
        "missing the matching test:\n{}",
        stdout
    );
    assert!(
        !stdout.contains("twice the input"),
        "filter should have excluded the non-matching test:\n{}",
        stdout
    );
    assert!(
        stdout.contains("1 passed, 0 failed"),
        "summary should count only the filtered-in test:\n{}",
        stdout
    );
}

#[test]
fn jet_test_shuffle_prints_the_seed_used() {
    // Gap #4: `--shuffle=<seed>` reorders deterministically and always prints
    // the seed, so a shuffle-dependent failure is reproducible.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    let have_rustc = have_rustc();
    if !have_rustc || !jet.exists() {
        return;
    }
    let example = root.join("examples/features/tooling/tests.jet");
    let out = Command::new(&jet)
        .arg("test").arg("--show-default").arg("--capture=all")
        .arg("--shuffle=42")
        .arg(&example)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "shuffled run failed:\n{}", stdout);
    assert!(
        stdout.contains("shuffle: seed=42"),
        "expected the seed line:\n{}",
        stdout
    );
    assert!(
        stdout.contains("2 passed, 0 failed"),
        "shuffling must not change which tests ran:\n{}",
        stdout
    );
}

#[test]
fn jet_test_serial_flag_still_passes() {
    // Gap #3: `--serial` opts out of the parallel default; behavior is
    // otherwise identical for a passing file.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    let have_rustc = have_rustc();
    if !have_rustc || !jet.exists() {
        return;
    }
    let example = root.join("examples/features/tooling/property_tests.jet");
    let out = Command::new(&jet)
        .arg("test").arg("--show-default").arg("--capture=all")
        .arg("--serial")
        .arg(&example)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "serial run failed:\n{}", stdout);
    assert!(
        stdout.contains("3 passed, 0 failed"),
        "serial run should behave like the parallel default:\n{}",
        stdout
    );
}

fn fuzz_corpus_dir(label: &str) -> PathBuf {
    let dir = common::unique_tmp(&format!("jet_fuzz_corpus_{label}"));
    let _ = fs::remove_dir_all(&dir);
    dir
}

#[test]
fn jet_fuzz_example_clean_run_output() {
    // I5: examples/features/tooling/fuzz_demo.jet is the executable spec for
    // generated tests — fixed `--seed`/`--iterations` so the clean-run report is
    // byte-for-byte deterministic (D-TESTKIT1=A gap #1).
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    let have_rustc = have_rustc();
    if !have_rustc || !jet.exists() {
        return;
    }
    let example = root.join("examples/features/tooling/fuzz_demo.jet");
    let corpus = fuzz_corpus_dir("example_demo");
    let out = Command::new(&jet)
        .args(["test", "--grade=generated"])
        .arg("--iterations=500")
        .arg("--seed=1")
        .arg(format!("--corpus={}", corpus.display()))
        .arg(&example)
        .arg("reverse_twice_is_identity")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "fuzz_demo.jet generated tests must pass:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let expected =
        fs::read_to_string(root.join("examples/features/expected/tooling/fuzz_demo.fuzz.out"))
            .expect("examples/features/expected/tooling/fuzz_demo.fuzz.out");
    assert_eq!(String::from_utf8_lossy(&out.stdout), expected);
    let _ = fs::remove_dir_all(&corpus);
}

#[test]
fn jet_fuzz_ambiguous_target_names_candidates() {
    // Gap #1, target selection: a file with more than one property test must
    // name one — this is CLI argument validation, not a compiler diagnostic
    // (same tier as a CLI missing-file argument error).
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    let have_rustc = have_rustc();
    if !have_rustc || !jet.exists() {
        return;
    }
    let example = root.join("examples/features/tooling/property_tests.jet");
    let out = Command::new(&jet)
        .args(["test", "--grade=generated"])
        .arg(&example)
        .output()
        .unwrap();
    assert!(!out.status.success(), "ambiguous target must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("multiple property tests")
            && stderr.contains("jet test --grade=generated <file> <name>"),
        "expected the ambiguous-target message:\n{}",
        stderr
    );
}

#[test]
fn jet_fuzz_no_property_test_errors() {
    // Gap #1, target selection: a file with only unit tests has nothing to fuzz.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    let have_rustc = have_rustc();
    if !have_rustc || !jet.exists() {
        return;
    }
    let example = root.join("examples/features/tooling/tests.jet");
    let out = Command::new(&jet)
        .args(["test", "--grade=generated"])
        .arg(&example)
        .output()
        .unwrap();
    assert!(!out.status.success(), "no property test must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("no property `#Test fn`"),
        "expected the no-property-test message:\n{}",
        stderr
    );
}

#[test]
fn jet_fuzz_deterministic_same_seed_same_corpus() {
    // Gap #1: a fixed `--seed` makes a run fully reproducible — same corpus
    // saved, same failing iteration, same minimized input.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    let have_rustc = have_rustc();
    if !have_rustc || !jet.exists() {
        return;
    }
    let fixture = root.join("tests/fixtures/prop_shrink.jet");

    let corpus_a = fuzz_corpus_dir("det_a");
    let out_a = Command::new(&jet)
        .args(["test", "--grade=generated", "--seed=7"])
        .arg(format!("--corpus={}", corpus_a.display()))
        .arg(&fixture)
        .output()
        .unwrap();

    let corpus_b = fuzz_corpus_dir("det_b");
    let out_b = Command::new(&jet)
        .args(["test", "--grade=generated", "--seed=7"])
        .arg(format!("--corpus={}", corpus_b.display()))
        .arg(&fixture)
        .output()
        .unwrap();

    assert!(
        !out_a.status.success(),
        "the fixture's property always fails"
    );
    assert_eq!(
        out_a.status.code(),
        out_b.status.code(),
        "same seed must reproduce the same exit code"
    );
    // Compare everything except the `saved:` line, which legitimately differs
    // (the two runs use different `--corpus` directories).
    let strip_saved = |s: &str| -> String {
        s.lines()
            .filter(|l| !l.trim_start().starts_with("saved:"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(
        strip_saved(&String::from_utf8_lossy(&out_a.stdout)),
        strip_saved(&String::from_utf8_lossy(&out_b.stdout)),
        "same seed must reproduce the same stdout (minimized input, iteration count)"
    );
    let stdout_a = String::from_utf8_lossy(&out_a.stdout);
    assert!(
        stdout_a.contains("minimized input: n = 50"),
        "expected the shrunk boundary value:\n{}",
        stdout_a
    );

    // Corpus entries for the same seed are identical (same failing seed saved).
    let entries_a: Vec<String> = fs::read_dir(&corpus_a)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
                .collect()
        })
        .unwrap_or_default();
    let entries_b: Vec<String> = fs::read_dir(&corpus_b)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(
        entries_a.len(),
        1,
        "expected exactly one saved corpus entry"
    );
    assert_eq!(
        entries_a, entries_b,
        "same seed must save the same corpus file name"
    );

    let _ = fs::remove_dir_all(&corpus_a);
    let _ = fs::remove_dir_all(&corpus_b);
}

#[test]
fn jet_fuzz_replays_corpus_before_generating_fresh_cases() {
    // Gap #1: a saved failing seed is replayed first on the next run, and a
    // still-reproducing corpus entry is reported (and fails the run) before
    // any fresh case is generated.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    let have_rustc = have_rustc();
    if !have_rustc || !jet.exists() {
        return;
    }
    let fixture = root.join("tests/fixtures/prop_shrink.jet");
    let corpus = fuzz_corpus_dir("replay");

    let first = Command::new(&jet)
        .args(["test", "--grade=generated", "--seed=7"])
        .arg(format!("--corpus={}", corpus.display()))
        .arg(&fixture)
        .output()
        .unwrap();
    assert!(!first.status.success());
    assert!(
        fs::read_dir(&corpus)
            .map(|rd| rd.count() == 1)
            .unwrap_or(false),
        "expected one saved corpus entry after the first run"
    );

    // A second run (different generation seed) must hit the replay path first.
    let second = Command::new(&jet)
        .args(["test", "--grade=generated"])
        .arg("--seed=999")
        .arg(format!("--corpus={}", corpus.display()))
        .arg(&fixture)
        .output()
        .unwrap();
    assert!(!second.status.success());
    let stdout = String::from_utf8_lossy(&second.stdout);
    assert!(
        stdout.contains("corpus replay"),
        "expected the second run to fail on corpus replay, not a fresh case:\n{}",
        stdout
    );

    let _ = fs::remove_dir_all(&corpus);
}

#[test]
fn jet_test_harness_keeps_helper_functions_on_their_own_error_family() {
    // #2350: `--test` emission used a build-wide `test_mode` flag to pick the
    // failure representation, so an ordinary function that happens to live in a
    // file with `#Test` blocks emitted `return Err(<String>)` for `assert`,
    // `assert_eq`, and `?? panic(…)`. Those functions do not return
    // `Result<_, String>`, so the generated Rust failed to build and the whole
    // harness never ran. Only `jet_test_N`/`jet_prop_N` carry the String ABI;
    // every other function keeps the same stop it has under `jet run`.
    // The same emission also bound both `assert_eq` operands by value, which
    // moved a non-Copy value out of a borrowed parameter (E0507); they are read
    // windows now, so a helper may compare its borrowed arguments.
    let jet = jet_bin();
    if !have_rustc() || !jet.exists() {
        return;
    }
    let dir = common::unique_tmp("jet_test_helper_family");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    tir_support::write_test_package(&dir, tir_support::TIR_TEST_PACKAGE);
    let source = dir.join("helpers.jet");
    fs::write(
        &source,
        r#"#Error
struct HelperError {
    message: String
}

fn first(values: [Int]) Int -> {
    return values.get(0) ?? panic("empty list")
}

fn checked(flag: Bool) Bool -> {
    assert(flag, "flag must hold")
    assert_eq(flag, true)
    return true
}

fn port(text: String) Int !HelperError -> {
    number :: text.trim().to_int() ?? panic("not a port number")
    if number < 0 -> return Err(HelperError{message: "negative port"})
    return Ok(number)
}

struct Recorded {
    argv: [String]
}

// #2350 part 2: both operands are read windows into borrowed non-scalar
// parameters. Binding them by value moved out of a shared reference (E0507),
// so the generated Rust never compiled.
fn same_argv(expected: Recorded, oracle_argv: [String]) Bool -> {
    assert_eq(expected.argv, oracle_argv)
    return true
}

#Test("helpers with assert and ?? panic build inside a test harness") {
    assert(checked(true))
    assert_eq(first([7]), 7)
    resolved :: port("  8080  ") ?? -1
    assert_eq(resolved, 8080)
    recorded :: Recorded{argv: ["jet", "run"]}
    assert(same_argv(recorded, ["jet", "run"]))
}

#Test("a failing harness assertion is still a caught failure") {
    assert_eq(first([1]), 2)
}
"#,
    )
    .unwrap();

    let out = Command::new(&jet)
        .args(["test", "--show-default", "--serial"])
        .arg(&source)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stdout.contains("helpers with assert and ?? panic build inside a test harness: pass"),
        "helper functions must not borrow the harness String failure ABI:\nstdout: {}\nstderr: {}",
        stdout,
        stderr
    );
    // The harness ABI itself is unchanged: an assertion in a `#Test` body is
    // reported as one caught failure, not a process-wide stop.
    assert!(
        stdout.contains("a failing harness assertion is still a caught failure: FAIL"),
        "the `#Test` body assertion must still report as a caught failure:\nstdout: {}\nstderr: {}",
        stdout,
        stderr
    );
    assert!(
        !out.status.success(),
        "a failing test must fail the run:\nstdout: {}\nstderr: {}",
        stdout,
        stderr
    );
    let _ = fs::remove_dir_all(&dir);
}
