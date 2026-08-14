//! M6 phase 2: `jet test` output shape and fail-then-fix flow.

use std::fs;
use std::path::PathBuf;
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
fn jet_bench_example_regions() {
    // D-BENCH1: `jet bench` on a file with `#Bench` blocks times each region
    // and reports `<name>  <ns> ns/iter (...)  <ops> ops/sec`. Timing values
    // are non-deterministic, so this asserts structure: every block runs and
    // every name + the ns/iter and ops/sec labels appear.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    let have_rustc = have_rustc();
    if !have_rustc || !jet.exists() {
        eprintln!("note: rustc not found; skipping jet bench integration");
        return;
    }

    let example = root.join("examples/features/tooling/bench.jet");
    let out = Command::new(&jet)
        .arg("bench")
        .arg("--show-default")
        .arg(&example)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "jet bench examples/features/tooling/bench.jet failed:\nstdout: {}\nstderr: {}",
        stdout,
        String::from_utf8_lossy(&out.stderr)
    );
    for needle in ["fib(10)", "sum to 100", "ns/iter", "ops/sec"] {
        assert!(
            stdout.contains(needle),
            "bench output missing `{}`:\n{}",
            needle,
            stdout
        );
    }
    // One report line per `#Bench` block.
    assert_eq!(
        stdout.lines().filter(|l| l.contains("ns/iter")).count(),
        2,
        "expected exactly two bench region lines:\n{}",
        stdout
    );
}

#[test]
fn jet_bench_para_map_crossover_matrix() {
    // E3-1405: the crossover example owns one map/para_map pair for each
    // count/cost cell. Timing values are host-specific; this proves that the
    // repeatable optimized benchmark emits every named cell and 20-sample
    // report shape without pinning a machine-dependent threshold.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    if !have_rustc() || !jet.exists() {
        return;
    }
    let example = root.join("examples/features/tooling/para_map_crossover_bench.jet");
    let out = Command::new(&jet)
        .arg("bench")
        .arg("--show-default")
        .arg(&example)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "para_map crossover benchmark failed:\nstdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    for count in [64, 256, 1024, 4096] {
        for cost in [1, 32, 256] {
            for method in ["map", "para_map"] {
                let name = format!("{method} n{count} c{cost}");
                assert!(stdout.contains(&name), "benchmark output missing {name}:\n{stdout}");
            }
        }
    }
    assert_eq!(
        stdout.lines().filter(|line| line.contains("ns/iter")).count(),
        24,
        "expected one report line per map/para_map cell:\n{stdout}"
    );
}

#[test]
fn jet_bench_failure_is_not_reported_as_timing() {
    // A failed `require` in a benchmark body must stop the command; silently
    // discarding the body Result would publish a false green timing report.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    if !have_rustc() || !jet.exists() {
        return;
    }
    let fixture = root.join("tests/fixtures/bench_fail.jet");
    let out = Command::new(&jet).arg("bench").arg("--show-default").arg(&fixture).output().unwrap();
    assert!(!out.status.success(), "failed benchmark body unexpectedly passed");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("bench region failed"),
        "missing benchmark failure boundary: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!String::from_utf8_lossy(&out.stdout).contains("ns/iter"));
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
        "require_eq should print the registered test report"
    );
    assert!(
        String::from_utf8_lossy(&bad.stderr).contains("expected 4, got 3"),
        "require_eq report should preserve expected/got"
    );
    assert!(
        String::from_utf8_lossy(&bad.stderr).contains("-->"),
        "require_eq report should preserve source location"
    );

    let good = Command::new(&jet).arg("test").arg("--show-default").arg(&fixed).output().unwrap();
    assert!(good.status.success());
    assert!(
        String::from_utf8_lossy(&good.stdout).contains("pass"),
        "fixed tests should pass"
    );
}

#[test]
fn jet_testing_file_failures_keep_report_context() {
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
    assert!(stderr.contains("-expected\n+actual\n"));
    assert_eq!(stderr.matches("Stop [E3001]").count(), 4, "stderr: {stderr}");
    assert_eq!(stderr.matches("-->").count(), 4, "stderr: {stderr}");

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
    let combined = format!("{stdout}{stderr}").to_ascii_lowercase();
    assert!(!combined.contains("jit"), "release test reported a JIT tier:\n{combined}");
    assert!(
        !combined.contains("interpreter"),
        "release test reported an interpreter tier:\n{combined}"
    );
}

#[test]
fn property_generator_distribution_report_is_reproducible() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
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
    let JSONValue::Array(engines) = report.get("engines").expect("engines") else {
        panic!("engines must be an array");
    };
    assert!(engines.iter().any(|engine| matches!(engine, JSONValue::String(engine) if engine == "jet_test")));
    assert!(engines.iter().any(|engine| matches!(engine, JSONValue::String(engine) if engine == "jet_fuzz")));
    let JSONValue::Object(comparison) = report.get("comparison").expect("comparison") else {
        panic!("comparison must be an object");
    };
    for field in ["predicates", "seeds", "sample_counts", "hit_rates"] {
        assert!(matches!(comparison.get(field), Some(JSONValue::String(value)) if value == "identical"));
    }

    let JSONValue::Object(seeds) = report.get("seeds").expect("seeds") else {
        panic!("seeds must be an object");
    };
    let JSONValue::Object(sample_counts) = report.get("sample_counts").expect("sample_counts") else {
        panic!("sample_counts must be an object");
    };
    for (field, expected) in [("seeds", 168), ("sample_counts", 200)] {
        let values = if field == "seeds" { seeds } else { sample_counts };
        assert!(matches!(values.get("jet_test"), Some(JSONValue::Number(value)) if *value == expected));
        assert!(matches!(values.get("jet_fuzz"), Some(JSONValue::Number(value)) if *value == expected));
    }

    let JSONValue::Object(hit_rates) = report.get("hit_rates").expect("hit_rates") else {
        panic!("hit_rates must be an object");
    };
    let JSONValue::Object(test_rates) = hit_rates.get("jet_test").expect("jet_test hit rates") else {
        panic!("jet_test hit rates must be an object");
    };
    let JSONValue::Object(fuzz_rates) = hit_rates.get("jet_fuzz").expect("jet_fuzz hit rates") else {
        panic!("jet_fuzz hit rates must be an object");
    };
    for (id, expected) in [
        ("int_eq_42", 0.03),
        ("int_small_anchor", 0.10),
        ("int_extreme", 0.075),
        ("int_random_fallback", 0.54),
    ] {
        let Some(JSONValue::Flt(test_rate)) = test_rates.get(id) else {
            panic!("missing jet_test rate for {id}");
        };
        let Some(JSONValue::Flt(fuzz_rate)) = fuzz_rates.get(id) else {
            panic!("missing jet_fuzz rate for {id}");
        };
        assert_eq!(test_rate, fuzz_rate, "engines disagree for {id}");
        assert!((*test_rate - expected).abs() < f64::EPSILON, "wrong rate for {id}");
    }

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

    let codegen = fs::read_to_string(root.join("crates/jet-codegen/src/Codegen/mod.rs"))
        .expect("property generator source");
    assert!(codegen.contains("match rng.below(32)"));
    assert!(codegen.contains("5 => 42"));
    assert!(codegen.contains("let case_seed = driver_rng.next_u64();"));
    assert!(codegen.contains("let seed = driver_rng.next_u64();"));
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
    for row in json_golden.lines().filter(|line| !line.trim().is_empty()) {
        assert!(json.contains(row.trim()), "JSON golden row missing: {row}\n{json}");
    }
}

#[test]
fn jet_bench_target_integration() {
    // c80 / D-TGT2: a package whose package.jet declares `target: benchmark` runs
    // its `#Bench` regions via the existing `jet bench` engine (no new mechanism).
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    let have_rustc = have_rustc();
    if !have_rustc || !jet.exists() {
        eprintln!("note: rustc not found; skipping jet bench target integration");
        return;
    }
    let example = root.join("examples/features/tooling/bench_target/main.jet");
    // Isolated cwd: this fixture's stem is `main`. `jet` writes `build/<stem>.*`
    // relative to its own cwd (Source/CmdCompile.rs `bin_path`/`stem`/`build`),
    // keyed only by stem — a concurrent test compiling a different `main.jet`
    // from the shared repo-root cwd would race this one on `build/main.rs`.
    let cwd = std::env::temp_dir().join(format!("jet_bench_target_cwd_{}", std::process::id()));
    let _ = fs::remove_dir_all(&cwd);
    fs::create_dir_all(&cwd).unwrap();
    let out = Command::new(&jet)
        .arg("bench")
        .arg("--show-default")
        .arg(&example)
        .current_dir(&cwd)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "jet bench examples/features/tooling/bench_target/main.jet failed:\nstdout: {}\nstderr: {}",
        stdout,
        String::from_utf8_lossy(&out.stderr)
    );
    for needle in ["sum_to(1000)", "ns/iter", "ops/sec"] {
        assert!(
            stdout.contains(needle),
            "bench output missing `{}`:\n{}",
            needle,
            stdout
        );
    }
    // Exactly one `#Bench` region in this example.
    assert_eq!(
        stdout.lines().filter(|l| l.contains("ns/iter")).count(),
        1,
        "expected exactly one bench region line:\n{}",
        stdout
    );
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
    assert!(dir.join("run.jet").exists(), "run.jet must be created by jet new");
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
        "#Test(\"top level\") { require(true) }\n",
    )
    .unwrap();
    fs::write(
        dir.join("nested/b.jet"),
        "#Test(\"one level down\") { require(true) }\n",
    )
    .unwrap();
    fs::write(
        dir.join("nested/deeper/c.jet"),
        "#Test(\"two levels down\") { require(true) }\n",
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
    // (same tier as `jet bench`'s "can't find the file").
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
