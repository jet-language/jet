//! M6 phase 2: `jet test` output shape and fail-then-fix flow.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use jet_foundation::JSON::{parse_json, JSONValue};

mod common;
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
        .arg("test")
        .arg("--show-default")
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
    let expected = fs::read_to_string(
        root.join("examples/features/expected/tooling/expected_fail.test.out"),
    )
    .expect("expected_fail.test.out");

    let green = Command::new(&jet)
        .args(["test", "--show-default", "--serial", "--filter=known"])
        .arg(&example)
        .output()
        .unwrap();
    assert!(green.status.success(), "expected failure should stay green:\n{}", String::from_utf8_lossy(&green.stdout));
    assert_eq!(
        String::from_utf8_lossy(&green.stdout),
        "known bug remains expected-fail: expected-fail\n0 passed, 0 failed, 0 skipped, 1 expected-fail\n"
    );

    let out = Command::new(&jet)
        .args(["test", "--show-default", "--serial"])
        .arg(&example)
        .output()
        .unwrap();
    assert!(!out.status.success(), "unexpected pass must fail the run");
    assert_eq!(String::from_utf8_lossy(&out.stdout), expected);

    let json = Command::new(&jet)
        .args(["test", "--json", "--serial"])
        .arg(&example)
        .output()
        .unwrap();
    assert!(!json.status.success(), "JSON run must preserve unexpected-pass failure");
    let json = String::from_utf8_lossy(&json.stdout);
    assert!(json.contains("\"expectedFailures\":1"), "missing expected-failure count: {json}");
    assert!(json.contains("\"unexpectedPasses\":1"), "missing unexpected-pass count: {json}");
}

#[test]
fn jet_test_package_collects_imported_module_tests() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    if !have_rustc() || !jet.exists() {
        return;
    }
    let package = root.join("examples/features/tooling/test_package_modules");
    let out = Command::new(&jet).arg("test").arg("--show-default").arg(&package).output().unwrap();
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
                .arg("test")
                .arg("--show-default")
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
        .arg("test")
        .arg("--show-default")
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
        .arg("test")
        .arg("--show-default")
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
    assert!(
        stderr.contains("expected this region to fail, but it passed"),
        "expected the expect_fail message:\n{}",
        stderr
    );
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
        .args(["test", "--show-default", fixture.to_str().expect("fixture path")])
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
        .arg("test")
        .arg("--show-default")
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
        .arg("test")
        .arg("--show-default")
        .arg(&fixture)
        .output()
        .unwrap();
    assert!(!out.status.success(), "an over-budget timeout must fail");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stdout.contains("FAIL"), "expected a FAIL line:\n{}", stdout);
    assert!(
        stderr.contains("timeout: region took"),
        "expected the timeout message:\n{}",
        stderr
    );
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

    let bad = Command::new(&jet).arg("test").arg("--show-default").arg(&fail).output().unwrap();
    assert!(!bad.status.success());
    assert!(
        String::from_utf8_lossy(&bad.stdout).contains("FAIL"),
        "expected a FAIL line, got: {}",
        String::from_utf8_lossy(&bad.stdout)
    );
    assert!(
        String::from_utf8_lossy(&bad.stderr).contains("Stop [E3001]"),
        "assert_eq should print the registered test report"
    );
    assert!(
        String::from_utf8_lossy(&bad.stderr).contains("expected 4, got 3"),
        "assert_eq report should preserve expected/got"
    );
    assert!(
        String::from_utf8_lossy(&bad.stderr).contains("-->"),
        "assert_eq report should preserve source location"
    );

    let good = Command::new(&jet).arg("test").arg("--show-default").arg(&fixed).output().unwrap();
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
    let bad = Command::new(&jet).arg("test").arg(&failing).output().unwrap();
    let stdout = String::from_utf8_lossy(&bad.stdout);
    let stderr = String::from_utf8_lossy(&bad.stderr);

    assert!(!bad.status.success());
    assert_eq!(stdout.matches(": FAIL").count(), 4, "stdout: {stdout}");
    assert!(stderr.contains("golden file is missing: tests/fixtures/testing_failure_reports.missing"));
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
    assert!(removal < addition, "diff order must be deterministic: {stderr}");
    assert_eq!(stderr.matches("Stop [E3001]").count(), 4, "stderr: {stderr}");
    assert_eq!(stderr.matches("-->").count(), 4, "stderr: {stderr}");
    let snapshot = fs::read_to_string(root.join("tests/fixtures/testing_failure_reports.stderr"))
        .expect("testing_failure_reports.stderr");
    let normalized_stderr = stderr.replace(root.to_str().expect("manifest path"), "<repo>");
    assert_eq!(normalized_stderr, snapshot, "typed testing report snapshot drifted");

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
            assert_eq!(
                fields.len(),
                5,
                "malformed property sample marker: {line}"
            );
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

#[test]
fn property_generator_distribution_report_is_reproducible() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    if !have_rustc() || !jet.exists() {
        return;
    }
    let report_path = root.join("tests/fixtures/property-generator-distribution.json");
    let report_text = fs::read_to_string(&report_path).expect("property distribution report");
    let JSONValue::Object(report) = parse_json(&report_text).expect("valid property report JSON") else {
        panic!("property distribution report must be an object");
    };
    assert!(matches!(report.get("schema"), Some(JSONValue::Number(1))));

    let JSONValue::Array(predicates) = report.get("predicates").expect("predicates") else {
        panic!("predicates must be an array");
    };
    let predicate_ids: Vec<&str> = predicates
        .iter()
        .map(|predicate| {
            let JSONValue::Object(predicate) = predicate else {
                panic!("predicate must be an object");
            };
            match predicate.get("id") {
                Some(JSONValue::String(id)) => id.as_str(),
                _ => panic!("predicate id must be a string"),
            }
        })
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
        .map(|predicate| {
            let JSONValue::Object(predicate) = predicate else {
                panic!("predicate must be an object");
            };
            match predicate.get("expression") {
                Some(JSONValue::String(expression)) => expression.as_str(),
                _ => panic!("predicate expression must be a string"),
            }
        })
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
    let JSONValue::Array(engines) = report.get("engines").expect("engines") else {
        panic!("engines must be an array");
    };
    assert!(engines.iter().any(|engine| matches!(engine, JSONValue::String(engine) if engine == "jet_test")));
    assert!(engines.iter().any(|engine| matches!(engine, JSONValue::String(engine) if engine == "jet_fuzz")));
    let JSONValue::Object(comparison) = report.get("comparison").expect("comparison") else {
        panic!("comparison must be an object");
    };
    for field in [
        "predicates",
        "seeds",
        "sample_counts",
        "hit_rates",
        "sample_stream",
    ] {
        assert!(matches!(comparison.get(field), Some(JSONValue::String(value)) if value == "identical"));
    }

    let JSONValue::Object(seeds) = report.get("seeds").expect("seeds") else {
        panic!("seeds must be an object");
    };
    let JSONValue::Object(sample_counts) = report.get("sample_counts").expect("sample_counts") else {
        panic!("sample_counts must be an object");
    };
    let JSONValue::Number(test_seed) = seeds.get("jet_test").expect("jet_test seed") else {
        panic!("jet_test seed must be a number");
    };
    let JSONValue::Number(fuzz_seed) = seeds.get("jet_fuzz").expect("jet_fuzz seed") else {
        panic!("jet_fuzz seed must be a number");
    };
    assert_eq!(test_seed, fuzz_seed, "engines use different base seeds");
    let seed = u64::try_from(*test_seed).expect("property seed must fit u64");
    let JSONValue::Number(test_count) = sample_counts
        .get("jet_test")
        .expect("jet_test sample count")
    else {
        panic!("jet_test sample count must be a number");
    };
    let JSONValue::Number(fuzz_count) = sample_counts
        .get("jet_fuzz")
        .expect("jet_fuzz sample count")
    else {
        panic!("jet_fuzz sample count must be a number");
    };
    assert_eq!(test_count, fuzz_count, "engines use different sample counts");
    let sample_count = usize::try_from(*test_count).expect("sample count must fit usize");

    let JSONValue::Object(hit_rates) = report.get("hit_rates").expect("hit_rates") else {
        panic!("hit_rates must be an object");
    };
    let JSONValue::Object(test_rates) = hit_rates.get("jet_test").expect("jet_test hit rates") else {
        panic!("jet_test hit rates must be an object");
    };
    let JSONValue::Object(fuzz_rates) = hit_rates.get("jet_fuzz").expect("jet_fuzz hit rates") else {
        panic!("jet_fuzz hit rates must be an object");
    };
    let predicate_names = [
        "int_eq_42",
        "int_small_anchor",
        "int_extreme",
        "int_random_fallback",
    ];
    for id in predicate_names {
        let Some(JSONValue::Flt(test_rate)) = test_rates.get(id) else {
            panic!("missing jet_test rate for {id}");
        };
        let Some(JSONValue::Flt(fuzz_rate)) = fuzz_rates.get(id) else {
            panic!("missing jet_fuzz rate for {id}");
        };
        assert_eq!(test_rate, fuzz_rate, "engines disagree for {id}");
    }

    let JSONValue::Object(expected_hit_counts) =
        report.get("hit_counts").expect("hit_counts")
    else {
        panic!("hit_counts must be an object");
    };
    let JSONValue::Object(expected_digests) =
        report.get("sample_digests").expect("sample_digests")
    else {
        panic!("sample_digests must be an object");
    };

    let JSONValue::Array(card_ids) = report
        .get("finding_card_ids")
        .expect("finding card ids")
    else {
        panic!("finding_card_ids must be an array");
    };
    assert!(card_ids.iter().any(|id| matches!(id, JSONValue::String(id) if id == "#1905")));
    let JSONValue::Array(findings) = report.get("findings").expect("findings") else {
        panic!("findings must be an array");
    };
    assert!(!findings.is_empty(), "distribution findings must be carded");
    for finding in findings {
        let JSONValue::Object(finding) = finding else {
            panic!("finding must be an object");
        };
        let Some(JSONValue::Array(ids)) = finding.get("finding_card_ids") else {
            panic!("finding card ids missing from finding");
        };
        assert!(ids.iter().any(|id| matches!(id, JSONValue::String(id) if id == "#1905")));
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

    let corpus = std::env::temp_dir().join(format!(
        "jet-property-generator-distribution-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&corpus);
    fs::create_dir_all(&corpus).expect("create empty fuzz corpus");
    let iterations_arg = format!("--iterations={sample_count}");
    let corpus_arg = format!("--corpus={}", corpus.display());
    let fuzz_out = Command::new(&jet)
        .arg("fuzz")
        .arg(&fixture)
        .arg("generator_contract")
        .arg(&iterations_arg)
        .arg(format!("--seed={seed}"))
        .arg(&corpus_arg)
        .env("JET_PROP_TRACE", "1")
        .output()
        .expect("run jet fuzz property generator fixture");
    let _ = fs::remove_dir_all(&corpus);
    assert!(
        fuzz_out.status.success(),
        "jet fuzz property distribution failed:\nstdout: {}\nstderr: {}",
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
        "jet test and jet fuzz generated different case seeds or inputs"
    );

    let test_counts = property_distribution(&test_samples);
    let fuzz_counts = property_distribution(&fuzz_samples);
    assert_eq!(test_counts, fuzz_counts, "engines disagree on predicate hits");
    for (index, id) in predicate_names.iter().enumerate() {
        let JSONValue::Object(test_counts_by_id) =
            expected_hit_counts.get("jet_test").expect("jet_test hit counts")
        else {
            panic!("jet_test hit counts must be an object");
        };
        let JSONValue::Number(expected_test_count) =
            test_counts_by_id.get(*id).expect("jet_test predicate count")
        else {
            panic!("jet_test predicate count must be a number");
        };
        assert_eq!(*expected_test_count as usize, test_counts[index], "wrong hit count for {id}");

        let JSONValue::Object(fuzz_counts_by_id) =
            expected_hit_counts.get("jet_fuzz").expect("jet_fuzz hit counts")
        else {
            panic!("jet_fuzz hit counts must be an object");
        };
        let JSONValue::Number(expected_fuzz_count) =
            fuzz_counts_by_id.get(*id).expect("jet_fuzz predicate count")
        else {
            panic!("jet_fuzz predicate count must be a number");
        };
        assert_eq!(*expected_fuzz_count as usize, fuzz_counts[index], "wrong fuzz hit count for {id}");

        let JSONValue::Flt(expected_test_rate) = test_rates.get(*id).expect("test rate") else {
            panic!("test rate must be a number");
        };
        let JSONValue::Flt(expected_fuzz_rate) = fuzz_rates.get(*id).expect("fuzz rate") else {
            panic!("fuzz rate must be a number");
        };
        let observed_rate = test_counts[index] as f64 / sample_count as f64;
        assert!((observed_rate - expected_test_rate).abs() < 1e-12, "wrong test rate for {id}");
        assert!((observed_rate - expected_fuzz_rate).abs() < 1e-12, "wrong fuzz rate for {id}");
    }

    for (engine, samples) in [("jet_test", &test_samples), ("jet_fuzz", &fuzz_samples)] {
        let JSONValue::String(expected_digest) =
            expected_digests.get(engine).expect("engine sample digest")
        else {
            panic!("sample digest must be a string");
        };
        assert_eq!(
            property_sample_digest(samples),
            expected_digest.as_str(),
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
        .arg("test")
        .arg("--show-default")
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
        .arg("test")
        .arg("--show-default")
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
        .arg("test")
        .arg("--show-default")
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
        .arg("test")
        .arg("--show-default")
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
        .arg("test")
        .arg("--show-default")
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
        .arg("test")
        .arg("--show-default")
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
    let text = String::from_utf8_lossy(&text_output.stdout).replace(
        &fixture_path,
        "tests/fixtures/coverage.jet",
    );
    let compact = |value: &str| value.split_whitespace().collect::<Vec<_>>().join(" ");
    let text = compact(&text);
    let text_golden = fs::read_to_string(root.join("tests/fixtures/coverage.text.golden"))
        .expect("coverage.text.golden");
    for row in text_golden.lines().filter(|line| !line.trim().is_empty()) {
        assert!(text.contains(&compact(row)), "text golden row missing: {row}\n{text}");
    }
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
    let json = String::from_utf8_lossy(&json_output.stdout).replace(
        &fixture_path,
        "tests/fixtures/coverage.jet",
    );
    let json_golden = fs::read_to_string(root.join("tests/fixtures/coverage.json.golden"))
        .expect("coverage.json.golden");
    assert!(json.contains("\"schema_version\":1"), "missing coverage schema:\n{json}");
    for row in json_golden.lines().filter(|line| !line.trim().is_empty()) {
        assert!(json.contains(row.trim()), "JSON golden row missing: {row}\n{json}");
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
    let dir = std::env::temp_dir().join(format!("jet_new_test_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let name = dir.file_name().unwrap().to_string_lossy();
    let out = Command::new(&jet)
        .arg("new")
        .arg(&*name)
        .current_dir(std::env::temp_dir())
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
        source.contains("fn run()") && source.contains("print("),
        "jet new must emit an executable fn run template: {source}"
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
        .current_dir(std::env::temp_dir())
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
// `jet fuzz` (corpus persistence, minimization, deterministic seeded PRNG).

#[test]
fn jet_test_dir_recurses_into_subdirectories() {
    // Gap #2: `jet test <dir>` used to read only the immediate directory
    // (Source/CmdCompile.rs:711-721); it must now walk subdirectories too.
    let jet = jet_bin();
    let have_rustc = have_rustc();
    if !have_rustc || !jet.exists() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_test_recurse_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("nested/deeper")).unwrap();
    fs::write(
        dir.join("a.jet"),
        "#Test(\"top level\") { assert(true) }\n",
    )
    .unwrap();
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
    let out = Command::new(&jet).arg("test").arg("--show-default").arg(&dir).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "recursive test dir run failed:\nstdout: {}\nstderr: {}",
        stdout,
        String::from_utf8_lossy(&out.stderr)
    );
    for needle in ["top level: pass", "one level down: pass", "two levels down: pass"] {
        assert!(stdout.contains(needle), "missing `{}`:\n{}", needle, stdout);
    }
    let _ = fs::remove_dir_all(&dir);
}

/// #2066: a real `jet new` project in a temp directory — the out-of-the-box
/// shape (`package.jet`, `run.jet` with no tests) each bare-`jet test` case
/// below adds its own member files to.
fn bare_package_project(label: &str, jet: &Path) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("jet_test_{}_{}", label, std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let name = dir.file_name().unwrap().to_string_lossy().to_string();
    let created = Command::new(jet)
        .arg("new")
        .arg(&name)
        .current_dir(std::env::temp_dir())
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "jet new failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&created.stdout),
        String::from_utf8_lossy(&created.stderr)
    );
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
    let out = Command::new(&jet).arg("test").current_dir(&dir).output().unwrap();
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
fn bare_jet_test_reports_no_tests_once_for_a_testless_package() {
    // #2066 criterion 2: E0601 fires only when the whole package has zero
    // `#Test` blocks — once for the package, not once per member file.
    let jet = jet_bin();
    if !have_rustc() || !jet.exists() {
        return;
    }
    let dir = bare_package_project("bare_testless", &jet);
    fs::write(dir.join("math.jet"), "fn double(n: Int) Int -> (n * 2)\n").unwrap();
    let out = Command::new(&jet)
        .arg("test")
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
    let out = Command::new(&jet).arg("test").current_dir(&dir).output().unwrap();
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
        .arg("test")
        .arg("--show-default")
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
        .arg("test")
        .arg("--show-default")
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
        .arg("test")
        .arg("--show-default")
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
    let dir = std::env::temp_dir().join(format!(
        "jet_fuzz_corpus_{}_{}",
        label,
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    dir
}

#[test]
fn jet_fuzz_example_clean_run_output() {
    // I5: examples/features/tooling/fuzz_demo.jet is the executable spec for
    // `jet fuzz` — fixed `--seed`/`--iterations` so the clean-run report is
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
        .arg("fuzz")
        .arg(&example)
        .arg("reverse_twice_is_identity")
        .arg("--iterations=500")
        .arg("--seed=1")
        .arg(format!("--corpus={}", corpus.display()))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "fuzz_demo.jet must fuzz clean:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let expected = fs::read_to_string(
        root.join("examples/features/expected/tooling/fuzz_demo.fuzz.out"),
    )
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
        .arg("fuzz")
        .arg(&example)
        .output()
        .unwrap();
    assert!(!out.status.success(), "ambiguous target must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("multiple property tests") && stderr.contains("jet fuzz <file> <name>"),
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
        .arg("fuzz")
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
        .arg("fuzz")
        .arg(&fixture)
        .arg("--seed=7")
        .arg(format!("--corpus={}", corpus_a.display()))
        .output()
        .unwrap();

    let corpus_b = fuzz_corpus_dir("det_b");
    let out_b = Command::new(&jet)
        .arg("fuzz")
        .arg(&fixture)
        .arg("--seed=7")
        .arg(format!("--corpus={}", corpus_b.display()))
        .output()
        .unwrap();

    assert!(!out_a.status.success(), "the fixture's property always fails");
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
    assert_eq!(entries_a.len(), 1, "expected exactly one saved corpus entry");
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
        .arg("fuzz")
        .arg(&fixture)
        .arg("--seed=7")
        .arg(format!("--corpus={}", corpus.display()))
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
        .arg("fuzz")
        .arg(&fixture)
        .arg("--seed=999")
        .arg(format!("--corpus={}", corpus.display()))
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
