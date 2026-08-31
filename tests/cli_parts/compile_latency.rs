use super::*;

/// #677 / D-PERFBUDGET-COMPILE1=C: the typed compile-latency proof.
///
/// This case lives in its own Cargo target, not beside the rest of `cli_parts`,
/// because the ratified policy makes it inherently expensive: `one fixed warmup,
/// twenty samples` per workload (docs/spec/performance-budget-decisions.md:37),
/// three cache scenarios, and two commands (`budget update --bootstrap` for the
/// baseline, `budget check` for the candidate) is 128 real child production
/// lens invocations. The `cli` target measured 307 s of *other* work
/// (docs/spec/roadmap.md:168); hosting this proof there
/// spent the whole 900 s suite guard and aborted the binary, taking every
/// unrelated `cli` case down with it. Splitting the target is the same remedy
/// #2020 applied to the corpus batteries (tests/dev.rs:17): each heavy proof
/// gets its own budget instead of a longer shared deadline.
///
/// The compiles this test pays for are the ones the criteria require. Per
/// command: 21 cold `Clean` builds, 1 cold + 20 warm `NoChange` builds, and 1
/// primed clean build plus 21 measured patched rebuilds for `Edit`
/// (Source/BudgetProviders.rs `compile_latency_samples`).
#[test]
fn budget_check_measures_typed_compile_workloads_and_records_provenance() {
    use jet_foundation::PerformanceBudget::CanonicalJson;
    let dir = compile_latency_budget_project("budget_compile_latency");
    let bootstrap = Command::new(jet())
        .args([
            "budget",
            "update",
            "--baseline",
            "ci/linux-x64",
            "--bootstrap",
            "--reason",
            "initial compile latency",
            "--yes",
            "--json",
        ])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(
        bootstrap.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&bootstrap.stdout),
        String::from_utf8_lossy(&bootstrap.stderr)
    );
    let CanonicalJson::Object(bootstrap_command) =
        CanonicalJson::parse_canonical(&bootstrap.stdout).unwrap()
    else {
        panic!("bootstrap command")
    };
    assert_eq!(
        *canonical_field(&bootstrap_command["budget"], "applied"),
        CanonicalJson::Bool(true)
    );
    let CanonicalJson::Object(bootstrap_report) =
        canonical_field(&bootstrap_command["budget"], "report")
    else {
        panic!("bootstrap report")
    };
    let CanonicalJson::Object(bootstrap_content) = &bootstrap_report["content"] else {
        panic!("bootstrap content")
    };
    let CanonicalJson::Array(bootstrap_measurements) = &bootstrap_content["measurements"] else {
        panic!("bootstrap measurements")
    };
    assert_eq!(bootstrap_measurements.len(), 3);
    for measurement in bootstrap_measurements {
        let CanonicalJson::Object(measurement) = measurement else {
            panic!("bootstrap measurement")
        };
        assert_eq!(
            measurement["unit"],
            CanonicalJson::String("Duration".into())
        );
        let CanonicalJson::Object(statistics) = &measurement["statistics"] else {
            panic!("bootstrap statistics")
        };
        assert_eq!(statistics["count"], CanonicalJson::Integer("20".into()));
    }
    let check = Command::new(jet())
        .args(["budget", "check", "--json"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(
        check.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&check.stdout),
        String::from_utf8_lossy(&check.stderr)
    );
    let CanonicalJson::Object(command) = CanonicalJson::parse_canonical(&check.stdout).unwrap()
    else {
        panic!("command")
    };
    let CanonicalJson::Object(report) = canonical_field(&command["budget"], "report") else {
        panic!("report")
    };
    let CanonicalJson::Object(content) = &report["content"] else {
        panic!("content")
    };
    let CanonicalJson::Array(measurements) = &content["measurements"] else {
        panic!("measurements")
    };
    assert_eq!(measurements.len(), 3);
    let mut cache_states = std::collections::BTreeSet::new();
    for measurement in measurements {
        let CanonicalJson::Object(measurement) = measurement else {
            panic!("measurement")
        };
        let CanonicalJson::Object(provider) = &measurement["provider"] else {
            panic!("provider")
        };
        assert_eq!(
            provider["kind"],
            CanonicalJson::String("CompilerProbe".into())
        );
        assert_eq!(
            measurement["unit"],
            CanonicalJson::String("Duration".into())
        );
        let CanonicalJson::Array(samples) = &measurement["samples"] else {
            panic!("samples")
        };
        assert_eq!(samples.len(), 20);
        let CanonicalJson::Object(statistics) = &measurement["statistics"] else {
            panic!("statistics")
        };
        assert_eq!(statistics["count"], CanonicalJson::Integer("20".into()));
        let CanonicalJson::Object(compile) = &measurement["compile"] else {
            panic!("compile metadata")
        };
        let CanonicalJson::String(cache_state) = &compile["cache_state"] else {
            panic!("cache state")
        };
        cache_states.insert(cache_state.clone());
        let CanonicalJson::Integer(edit_bytes) = &compile["edit_bytes"] else {
            panic!("edit byte count")
        };
        assert_eq!(cache_state == "Edit", edit_bytes != "0");
        assert_eq!(compile["warmups"], CanonicalJson::Integer("1".into()));
        assert_eq!(compile["samples"], CanonicalJson::Integer("20".into()));
        if cache_state == "Clean" || cache_state == "NoChange" || cache_state == "Edit" {
            assert_eq!(
                compile["backend"],
                CanonicalJson::String("cranelift-jit".into())
            );
            assert_eq!(compile["profile"], CanonicalJson::String("dev".into()));
        }
        for key in [
            "source_tree_sha256",
            "compiler_digest",
            "core_digest",
            "clock",
            "target",
            "profile",
            "backend",
            "linker",
            "host",
            "cache_state",
            "warmups",
            "samples",
            "variance",
            "phase_totals",
            "sample_records",
            "edit_bytes",
            "edit_sha256",
            "workload_bytes",
        ] {
            assert!(
                compile.contains_key(key),
                "missing compile metadata field {key}"
            );
        }
        assert_eq!(
            compile["clock"],
            CanonicalJson::String("process_cpu".into())
        );
        assert!(
            compile.contains_key("peak_rss_bytes"),
            "missing compile metadata field peak_rss_bytes"
        );
    }
    assert_eq!(
        cache_states,
        ["Clean", "NoChange", "Edit"]
            .into_iter()
            .map(String::from)
            .collect()
    );
    let report_path = fs::read_dir(dir.join(".jet/perf/reports"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    jet_foundation::PerformanceBudget::verify_budget_report(&fs::read(report_path).unwrap())
        .unwrap();
}
