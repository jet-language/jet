use super::*;

#[test]
fn compiler_probe_rejects_missing_edit_input_and_closed_metric_typos() {
    let dir = compile_latency_budget_project("compile_probe_hostile_inputs");
    fs::remove_file(dir.join("edits/rename-route.patch")).unwrap();
    let missing = Command::new(jet()).args(["budget", "check", "--json"]).current_dir(&dir).output().unwrap();
    assert_eq!(missing.status.code(), Some(1));
    let missing_text = String::from_utf8_lossy(&missing.stdout);
    assert!(missing_text.contains("E2908") || String::from_utf8_lossy(&missing.stderr).contains("E2908"));
    assert!(!dir.join(".jet/perf/reports").exists(), "missing edit input must not produce a report");

    let typo_dir = compile_latency_budget_project("compile_probe_closed_metric");
    let source = fs::read_to_string(typo_dir.join("src/run.jet")).unwrap().replace(".CompileTime(.P95)", ".CompileLatency(.P95)");
    fs::write(typo_dir.join("src/run.jet"), source).unwrap();
    let typo = Command::new(jet()).args(["budget", "check", "--json"]).current_dir(&typo_dir).output().unwrap();
    assert_eq!(typo.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&typo.stdout).contains("E2903") || String::from_utf8_lossy(&typo.stderr).contains("E2903"));
}

#[test]
fn allocation_probe_uses_real_bench_boundaries_and_rejects_forged_cache() {
    use jet_foundation::PerformanceBudget::CanonicalJson;
    let dir = allocation_budget_project("allocation_probe_runtime");
    let run = || Command::new(jet()).args(["bench", "--show-default", "src/main.jet"]).current_dir(&dir).output().unwrap();

    let first = run();
    assert_eq!(first.status.code(), Some(0), "stdout: {}\nstderr: {}", String::from_utf8_lossy(&first.stdout), String::from_utf8_lossy(&first.stderr));
    let reports = dir.join(".jet/perf/reports");
    let paths = || fs::read_dir(&reports).unwrap().map(|entry| entry.unwrap().path()).collect::<Vec<_>>();
    let initial = paths();
    assert_eq!(initial.len(), 1);
    let bytes = fs::read(&initial[0]).unwrap();
    jet_foundation::PerformanceBudget::verify_budget_report(&bytes).unwrap();
    let CanonicalJson::Object(report) = CanonicalJson::parse_canonical(&bytes).unwrap() else { panic!("report") };
    let CanonicalJson::Object(content) = &report["content"] else { panic!("content") };
    let CanonicalJson::Array(measurements) = &content["measurements"] else { panic!("measurements") };
    assert_eq!(measurements.len(), 2);
    for measurement in measurements {
        let CanonicalJson::Object(measurement) = measurement else { panic!("measurement") };
        let CanonicalJson::Object(provider) = &measurement["provider"] else { panic!("provider") };
        assert_eq!(provider["kind"], CanonicalJson::String("AllocationProbe".into()));
        assert_eq!(provider["identity"], CanonicalJson::String("arena".into()));
        assert_eq!(provider["isolation"], CanonicalJson::String("benchmark-process-counter-reset-per-trial".into()));
        assert_eq!(provider["version"], CanonicalJson::String("jet-arena-events-v1-warmup-auto-trials-20".into()));
        let CanonicalJson::Array(samples) = &measurement["samples"] else { panic!("samples") };
        assert_eq!(samples.len(), 20);
        assert!(samples.windows(2).all(|pair| pair[0] == pair[1]), "reset boundary leaked calibration or a prior trial");
        let CanonicalJson::Object(metric) = &measurement["metric"] else { panic!("metric") };
        let expected = match &metric["name"] {
            CanonicalJson::String(name) if name == "AllocationCount" => "1",
            CanonicalJson::String(name) if name == "AllocationBytes" => "8",
            other => panic!("unexpected metric: {other:?}"),
        };
        for sample in samples {
            let CanonicalJson::Object(sample) = sample else { panic!("sample") };
            assert_eq!(sample["den"], CanonicalJson::Integer("1".into()));
            assert_eq!(sample["num"], CanonicalJson::Integer(expected.into()));
        }
    }

    let second = run();
    assert_eq!(second.status.code(), Some(0));
    assert!(!String::from_utf8_lossy(&second.stdout).contains("ns/iter"), "compatible report reran allocation workload");
    assert_eq!(paths(), initial);

    fs::OpenOptions::new().append(true).open(&initial[0]).unwrap().write_all(b"forged").unwrap();
    let third = run();
    assert_eq!(third.status.code(), Some(0), "{}", String::from_utf8_lossy(&third.stderr));
    assert_eq!(paths().len(), 2, "forged report must not satisfy compatible cache identity");
}

#[test]
fn budget_stale_history_is_persisted_rendered_and_bootstrap_appends() {
    use jet_foundation::PerformanceBudget::CanonicalJson;
    let dir = benchmark_budget_project("budget_stale_history");
    let source = fs::read_to_string(dir.join("src/main.jet")).unwrap().replace("enforcement: .Warn", "enforcement: .Fail");
    fs::write(dir.join("src/main.jet"), source).unwrap();
    let first = Command::new(jet()).args(["budget","update","--baseline","ci/linux","--bootstrap","--reason","initial benchmark","--yes","--json"]).current_dir(&dir).output().unwrap();
    assert_eq!(first.status.code(),Some(0),"stdout: {}\nstderr: {}",String::from_utf8_lossy(&first.stdout),String::from_utf8_lossy(&first.stderr));
    let (first_id, stale_state_id) = age_budget_baseline(&dir, "ci/linux");

    let check = Command::new(jet()).args(["budget","check","--json"]).current_dir(&dir).output().unwrap();
    assert_eq!(check.status.code(),Some(1),"stdout: {}\nstderr: {}",String::from_utf8_lossy(&check.stdout),String::from_utf8_lossy(&check.stderr));
    let CanonicalJson::Object(check) = CanonicalJson::parse_canonical(&check.stdout).unwrap() else { panic!("command") };
    assert_eq!(check["status"],CanonicalJson::String("stale".into()));
    assert_eq!(check["failure_kind"],CanonicalJson::String("evidence".into()));
    let CanonicalJson::Array(results)=&check["results"] else { panic!("results") };
    let CanonicalJson::Object(result)=&results[0] else { panic!("result") };
    assert_eq!(result["stale"],CanonicalJson::Bool(true));
    assert_eq!(result["status"],CanonicalJson::String("stale".into()));
    assert_eq!(result["evidence"],CanonicalJson::String("unavailable".into()));
    assert_eq!(result["baseline_report_ids"],CanonicalJson::Array(vec![CanonicalJson::String(first_id.clone())]));
    let CanonicalJson::String(reason)=&result["reason"] else { panic!("reason") };
    assert!(reason.contains("compatible history is stale"),"{reason}");
    assert!(reason.contains("policy limit is 2592000 seconds"),"{reason}");
    let CanonicalJson::Object(report)=&check["report"] else { panic!("report") };
    jet_foundation::PerformanceBudget::verify_budget_report(&CanonicalJson::Object(report.clone()).bytes()).unwrap();
    let CanonicalJson::Object(content)=&report["content"] else { panic!("content") };
    let CanonicalJson::Array(measurements)=&content["measurements"] else { panic!("measurements") };
    let CanonicalJson::Object(measurement)=&measurements[0] else { panic!("measurement") };
    let CanonicalJson::Object(history)=&measurement["history"] else { panic!("history") };
    assert_eq!(history["state_id"],CanonicalJson::String(stale_state_id.clone()));
    assert_eq!(history["report_ids"],CanonicalJson::Array(vec![CanonicalJson::String(first_id.clone())]));
    assert_eq!(measurement["baseline"],CanonicalJson::Null,"stale samples must not be pooled");
    let CanonicalJson::Object(decision)=&measurement["decision"] else { panic!("decision") };
    let CanonicalJson::Object(trend)=&decision["trend"] else { panic!("trend") };
    assert_eq!(trend["report_ids"],CanonicalJson::Array(vec![CanonicalJson::String(first_id.clone())]));

    let human = Command::new(jet()).args(["budget","check","--annotations","none"]).current_dir(&dir).output().unwrap();
    assert_eq!(human.status.code(),Some(1));
    let human = String::from_utf8(human.stderr).unwrap();
    assert!(human.contains("Error [E2906]: performance budget parse has no usable evidence"),"{human}");
    assert!(human.contains("compatible history is stale"),"{human}");
    assert!(human.contains("budgets stale: 1 baseline stale · report "),"{human}");

    let bootstrap = Command::new(jet()).args(["budget","update","--baseline","ci/linux","--bootstrap","--reason","refresh stale benchmark","--yes","--json"]).current_dir(&dir).output().unwrap();
    assert_eq!(bootstrap.status.code(),Some(0),"stdout: {}\nstderr: {}",String::from_utf8_lossy(&bootstrap.stdout),String::from_utf8_lossy(&bootstrap.stderr));
    let CanonicalJson::Object(bootstrap)=CanonicalJson::parse_canonical(&bootstrap.stdout).unwrap() else { panic!("bootstrap") };
    assert_eq!(bootstrap["applied"],CanonicalJson::Bool(true));
    assert_eq!(bootstrap["status"],CanonicalJson::String("stale".into()));
    let CanonicalJson::Object(report)=&bootstrap["report"] else { panic!("report") };
    let CanonicalJson::String(second_id)=&report["report_id"] else { panic!("report id") };
    let manifest = CanonicalJson::parse_canonical(&fs::read(dir.join(".jet/perf/baselines/names/ci/linux.json")).unwrap()).unwrap();
    let CanonicalJson::Object(wrapper)=manifest else { panic!("manifest") };
    let CanonicalJson::Object(content)=&wrapper["content"] else { panic!("content") };
    let CanonicalJson::Array(generations)=&content["generations"] else { panic!("generations") };
    assert_eq!(generations.len(),2);
    let CanonicalJson::Object(second)=&generations[1] else { panic!("generation") };
    let CanonicalJson::Object(audit)=&second["audit"] else { panic!("audit") };
    assert_eq!(audit["kind"],CanonicalJson::String("bootstrap".into()));
    assert_eq!(audit["prior_state_id"],CanonicalJson::String(stale_state_id));
    assert_eq!(audit["prior_head_report_id"],CanonicalJson::String(first_id));
    assert_eq!(second["report_id"],CanonicalJson::String(second_id.clone()));
}

#[test]
fn budget_effect_count_uses_solved_effects_not_import_count() {
    let dir = budget_project("budget_effect_truth", 10);
    fs::write(dir.join("src/main.jet"), r#"use core.files as files
module perf.package {
    budgets: [Budget.{ name: "effects", scope: .Package, metric: .EffectCount, comparison: .Absolute, limit: .AtMost(0) }],
}
fn run() {}
"#).unwrap();
    let out = Command::new(jet()).args(["budget", "check", "--json"]).current_dir(&dir).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    let json = String::from_utf8(out.stdout).unwrap();
    assert!(json.contains("\"budget_id\":\"package:effects\""));
    assert!(json.contains("\"point\":{\"den\":1,\"num\":0}"), "unused core import must not fabricate an effect: {json}");
}

#[test]
fn budget_generated_unsafe_rejects_proxy_before_artifact() {
    let dir = budget_project("budget_unsafe_truth", 10);
    fs::write(dir.join("src/main.jet"), r#"use core.mem as mem
module perf.package {
    budgets: [Budget.{ name: "unsafe", scope: .Package, metric: .GeneratedUnsafe, comparison: .Absolute, limit: .AtMost(0) }],
}
fn run() {}
"#).unwrap();
    let out = Command::new(jet()).args(["budget", "check"]).current_dir(&dir).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("has no exact checked front-end fact; refusing proxy measurement"), "{stderr}");
    assert!(!dir.join(".jet").exists(), "unsupported fact emitted an artifact");
}

#[test]
#[cfg(unix)]
fn budget_path_tools_cannot_forge_provenance() {
    use std::os::unix::fs::PermissionsExt;
    let dir = budget_project("budget_hostile_path", 10);
    let fake = dir.join("fake-bin");
    fs::create_dir(&fake).unwrap();
    for (name, body) in [
        ("rustc", "#!/bin/sh\necho 'host: fake-forged-triple'\n"),
        ("sha256sum", "#!/bin/sh\necho 'ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff  fake'\n"),
        ("shasum", "#!/bin/sh\necho 'ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff  fake'\n"),
    ] {
        let path = fake.join(name);
        fs::write(&path, body).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let out = Command::new(jet()).args(["budget", "check", "--json"]).env("PATH", &fake).current_dir(&dir).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    let json = String::from_utf8(out.stdout).unwrap();
    assert!(!json.contains("fake-forged-triple"), "PATH rustc forged target identity");
    assert!(!json.contains("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"), "PATH digest tool forged compiler identity");
}

#[test]
#[cfg(unix)]
fn budget_unreadable_compiler_identity_rejects_before_artifact() {
    use std::os::unix::fs::PermissionsExt;
    let dir = artifact_budget_project("budget_missing_compiler_identity", 100_000_000);
    let copied = dir.join("jet-unreadable");
    fs::copy(jet(), &copied).unwrap();
    fs::set_permissions(&copied, fs::Permissions::from_mode(0o111)).unwrap();
    let out = output_with_retry(Command::new(&copied).args(["budget", "check"]).current_dir(&dir));
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("cannot hash running compiler executable"), "{stderr}");
    assert!(!dir.join(".jet").exists(), "missing compiler identity emitted an artifact");
    assert!(!dir.join("build/main").exists(), "missing compiler identity started the selected artifact build");
}

#[test]
#[cfg(target_os = "linux")]
fn budget_parallel_child_builds_survive_running_compiler_unlink() {
    use jet_foundation::PerformanceBudget::CanonicalJson;
    use std::os::unix::fs::MetadataExt;
    let dirs = [
        artifact_budget_project("budget_unlinked_compiler_identity_a", 100_000_000),
        artifact_budget_project("budget_unlinked_compiler_identity_b", 100_000_000),
    ];
    let bin_dir = isolated_cwd("budget_unlinked_compiler_binary");
    let copied = bin_dir.join("jet-running-unlinked");
    let cache = bin_dir.join("cache");
    fs::copy(jet(), &copied).unwrap();
    // Pin the artifact before unlink: independent rustc links have different
    // build IDs, and this test owns compiler replacement rather than linking.
    let primed = output_with_retry(Command::new(&copied)
        .arg("build")
        .arg(dirs[0].join("src/main.jet"))
        .current_dir(&dirs[0])
        .env("JET_CACHE_DIR", &cache));
    assert_eq!(primed.status.code(), Some(0), "stdout: {}\nstderr: {}", String::from_utf8_lossy(&primed.stdout), String::from_utf8_lossy(&primed.stderr));
    let seed_artifact = dirs[0].join("build/main");
    let expected_artifact = (
        CanonicalJson::String(jet::SHA256::sha256_file_hex(&seed_artifact).unwrap()),
        CanonicalJson::Integer(fs::metadata(seed_artifact).unwrap().len().to_string()),
    );
    let expected_compiler = env!("JET_COMPILER_BUILD_ID").to_string();
    let expected_stdlib = env!("JET_STDLIB_BUILD_ID").to_string();
    let expected_runner = env!("JET_RUNNER_BUILD_ID").to_string();
    let children = dirs.iter().map(|dir| {
        spawn_with_retry(Command::new(&copied)
            .args(["budget", "check", "--json"])
            .current_dir(dir)
            .env("JET_CACHE_DIR", &cache)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped()))
    }).collect::<Vec<_>>();
    let running_inode = fs::metadata(&copied).unwrap().ino();
    fs::remove_file(&copied).unwrap();
    fs::write(&copied, "replacement compiler inode\n").unwrap();
    assert_ne!(running_inode, fs::metadata(&copied).unwrap().ino());
    for (child, dir) in children.into_iter().zip(&dirs) {
        let out = child.wait_with_output().unwrap();
        assert_eq!(out.status.code(), Some(0), "stdout: {}\nstderr: {}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(out.stderr.is_empty());
        let CanonicalJson::Object(command) = CanonicalJson::parse_canonical(&out.stdout).unwrap() else { panic!("command object") };
        let CanonicalJson::Object(report) = &command["report"] else { panic!("report object") };
        jet_foundation::PerformanceBudget::verify_budget_report(&CanonicalJson::Object(report.clone()).bytes()).unwrap();
        let CanonicalJson::Object(content) = &report["content"] else { panic!("content object") };
        let CanonicalJson::Object(toolchain) = &content["toolchain"] else { panic!("toolchain object") };
        assert_eq!(toolchain["compiler_build_id"], CanonicalJson::String(expected_compiler.clone()));
        assert_eq!(toolchain["runner_id"], CanonicalJson::String(expected_runner.clone()));
        assert_eq!(toolchain["stdlib_id"], CanonicalJson::String(expected_stdlib.clone()));
        let CanonicalJson::Object(subject) = &content["subject"] else { panic!("subject object") };
        let CanonicalJson::Object(artifact) = &subject["artifact"] else { panic!("artifact object") };
        let artifact_path = dir.join("build/main");
        assert_eq!(artifact["sha256"], CanonicalJson::String(jet::SHA256::sha256_file_hex(&artifact_path).unwrap()));
        assert_eq!(artifact["bytes"], CanonicalJson::Integer(fs::metadata(artifact_path).unwrap().len().to_string()));
        assert_eq!((&artifact["sha256"], &artifact["bytes"]), (&expected_artifact.0, &expected_artifact.1), "parallel builds produced different artifact identities");
    }
}

#[test]
fn budget_failure_has_human_github_projection_and_exit_one() {
    let dir = budget_project("budget_failure", 0);
    let out = Command::new(jet()).args(["budget", "check", "--annotations", "github"]).current_dir(&dir).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("Error [E2907]: performance budget public-api regressed"), "{stderr}");
    assert!(stderr.contains("::error file=src/main.jet"), "{stderr}");
    assert!(stderr.contains("performance budget public-api regressed%0AWhy: measured estimator"), "{stderr}");
    assert!(stderr.contains("%0AFix: improve the measured behavior, inspect `jet budget check --verbose`, or record an explicit exception"), "{stderr}");
    assert!(stderr.contains("budgets failed: 1 budget failed · report "), "{stderr}");
}

#[test]
fn budget_imported_declaration_reports_owning_module_location() {
    let dir = budget_project("budget_imported_source", 10);
    fs::write(dir.join("src/main.jet"), "module perf_defs;\nfn run() {}\n").unwrap();
    fs::write(dir.join("src/perf_defs.jet"), r#"module perf.package {
    budgets: [Budget.{ name: "imported-api", scope: .Package, metric: .PublicApiItems, comparison: .Absolute, limit: .AtMost(0) }],
}
pub fn imported() {}
"#).unwrap();
    let out = Command::new(jet()).args(["budget", "check", "--annotations", "github"]).current_dir(&dir).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains(" --> src/perf_defs.jet:2:15"), "{stderr}");
    assert!(stderr.contains("::error file=src/perf_defs.jet,line=2,col=15,title=Jet E2907::"), "{stderr}");
    let report = fs::read_dir(dir.join(".jet/perf/reports")).unwrap().next().unwrap().unwrap().path();
    let report = fs::read_to_string(report).unwrap();
    assert!(report.contains("\"source\":\"src/perf_defs.jet:2\""), "{report}");
}

#[test]
fn budget_update_is_plan_first_and_yes_applies_once() {
    use jet_foundation::PerformanceBudget::CanonicalJson;
    let dir = budget_project("budget_update", 10);
    let args = ["budget", "update", "--baseline", "ci/linux", "--bootstrap", "--reason", "initial evidence", "--json"];
    let plan = Command::new(jet()).args(args).current_dir(&dir).output().unwrap();
    assert_eq!(plan.status.code(), Some(0), "{}", String::from_utf8_lossy(&plan.stderr));
    let CanonicalJson::Object(plan)=jet_foundation::PerformanceBudget::CanonicalJson::parse_canonical(&plan.stdout).unwrap() else{panic!("command")};
    assert_eq!(plan["applied"],CanonicalJson::Bool(false));
    let CanonicalJson::Object(plan)=&plan["plan"] else{panic!("plan")};assert_eq!(plan["requires_confirmation"],CanonicalJson::Bool(false));let CanonicalJson::Array(rows)=&plan["rows"] else{panic!("rows")};assert_eq!(rows.len(),2);let CanonicalJson::Object(report)=&rows[0] else{panic!("report row")};let CanonicalJson::Object(baseline)=&rows[1] else{panic!("baseline row")};assert_eq!(report["operation"],CanonicalJson::String("create".into()));assert_eq!(report["artifact"],CanonicalJson::String("report".into()));assert_eq!(baseline["operation"],CanonicalJson::String("advance".into()));assert_eq!(baseline["artifact"],CanonicalJson::String("baseline".into()));
    assert!(!dir.join(".jet").exists(),"JSON plan-only mutated workspace");

    let applied = Command::new(jet()).args(args).arg("--yes").current_dir(&dir).output().unwrap();
    assert_eq!(applied.status.code(), Some(0), "{}", String::from_utf8_lossy(&applied.stderr));
    let applied = String::from_utf8(applied.stdout).unwrap();
    assert!(applied.contains("\"applied\":true"));
    assert!(dir.join(".jet/perf/baselines/names/ci/linux.json").is_file());
}

#[test]
fn budget_json_projection_is_exact_and_tool_failure_uses_null_report_fields() {
    use jet_foundation::PerformanceBudget::CanonicalJson;
    let dir = budget_project("budget_json_exact", 10);
    let out = Command::new(jet()).args(["budget", "check", "--json"]).current_dir(&dir).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stderr.is_empty());
    let CanonicalJson::Object(command) = CanonicalJson::parse_canonical(&out.stdout).unwrap() else { panic!("command object") };
    assert_eq!(command.keys().map(String::as_str).collect::<Vec<_>>(), ["applied","command","diagnostics","exit_code","failure_kind","plan","report","report_path","results","schema","status","version"]);
    let CanonicalJson::Array(results) = &command["results"] else { panic!("results") };
    let CanonicalJson::Object(result) = &results[0] else { panic!("result") };
    assert_eq!(result.keys().map(String::as_str).collect::<Vec<_>>(), ["baseline_report_ids","budget_id","comparison","diagnostic_code","direction","enforcement","evidence","lower95","metric","point","reason","source","stale","status","trend","unit","upper95"]);
    let CanonicalJson::Object(source) = &result["source"] else { panic!("source") };
    assert_eq!(source.keys().map(String::as_str).collect::<Vec<_>>(), ["column","line","path"]);
    let CanonicalJson::Object(comparison) = &result["comparison"] else { panic!("comparison") };
    assert_eq!(comparison.keys().map(String::as_str).collect::<Vec<_>>(), ["direction","kind","limit"]);

    fs::write(dir.join("src/main.jet"), "fn run( {\n").unwrap();
    let invalid = Command::new(jet()).args(["budget", "check", "--json"]).current_dir(&dir).output().unwrap();
    assert_eq!(invalid.status.code(), Some(1));
    assert!(invalid.stderr.is_empty());
    let CanonicalJson::Object(invalid) = CanonicalJson::parse_canonical(&invalid.stdout).unwrap() else { panic!("compiler failure object") };
    assert_eq!(invalid["failure_kind"], CanonicalJson::String("compiler".into()));
    let CanonicalJson::Array(diagnostics) = &invalid["diagnostics"] else { panic!("diagnostics") };
    let CanonicalJson::Object(diagnostic) = &diagnostics[0] else { panic!("diagnostic") };
    let CanonicalJson::Object(source) = &diagnostic["source"] else { panic!("diagnostic source") };
    assert_eq!(source.keys().map(String::as_str).collect::<Vec<_>>(), ["column","end_column","end_line","line","path"]);
    assert_eq!(source["path"], CanonicalJson::String("src/main.jet".into()));
    assert!(matches!(source["end_line"], CanonicalJson::Integer(_)));
    assert!(matches!(source["end_column"], CanonicalJson::Integer(_)));

    let empty = isolated_cwd("budget_json_tool_failure");
    let failed = Command::new(jet()).args(["budget", "check", "--json"]).current_dir(&empty).output().unwrap();
    assert_eq!(failed.status.code(), Some(1));
    assert!(failed.stderr.is_empty());
    let CanonicalJson::Object(failure) = CanonicalJson::parse_canonical(&failed.stdout).unwrap() else { panic!("failure object") };
    assert_eq!(failure["status"], CanonicalJson::String("fail".into()));
    assert_eq!(failure["failure_kind"], CanonicalJson::String("tool".into()));
    assert_eq!(failure["report"], CanonicalJson::Null);
    assert_eq!(failure["report_path"], CanonicalJson::Null);
    assert_eq!(failure["plan"], CanonicalJson::Null);
}

#[test]
fn budget_non_tty_plan_only_creates_no_artifact_or_baseline() {
    let dir = budget_project("budget_non_tty_cancel", 10);
    let out = Command::new(jet()).args(["budget", "update", "--baseline", "ci/linux", "--bootstrap", "--reason", "initial evidence"]).current_dir(&dir).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("plan only; pass -y or --yes to apply in a non-interactive shell"), "{stderr}");
    assert!(!dir.join(".jet").exists(), "plan-only update mutated workspace");
}

#[test]
#[cfg(unix)]
fn budget_tty_confirmation_cancel_and_yes_control_mutation() {
    let cancelled = budget_project("budget_tty_no", 10);
    let (code, transcript) = run_budget_update_pty(&cancelled, b"n\n");
    assert_eq!(code, 0, "{transcript}");
    assert!(transcript.contains("Apply? [y/N]"), "{transcript}");
    assert!(transcript.contains("plan cancelled; no baseline changed"), "{transcript}");
    assert!(!cancelled.join(".jet").exists(), "TTY cancel mutated workspace");

    let applied = budget_project("budget_tty_yes", 10);
    let (code, transcript) = run_budget_update_pty(&applied, b"yes\n");
    assert_eq!(code, 0, "{transcript}");
    assert!(transcript.contains("Apply? [y/N]"), "{transcript}");
    assert_eq!(transcript.matches("+ report ").count(), 1, "plan/apply duplicated report row: {transcript}");
    assert_eq!(transcript.matches("~ baseline ").count(), 1, "plan/apply duplicated baseline row: {transcript}");
    assert!(applied.join(".jet/perf/baselines/names/ci/linux.json").is_file(), "TTY yes did not apply");
    assert!(applied.join(".jet/perf/reports").read_dir().unwrap().next().is_some(), "TTY yes omitted report artifact");
}

#[test]
fn budget_surface_is_generated_into_help_completions_and_man() {
    let help = Command::new(jet()).arg("help").output().unwrap();
    assert!(String::from_utf8(help.stdout).unwrap().contains("budget"));
    let completions = Command::new(jet()).args(["self", "completions", "bash"]).output().unwrap();
    let completions = String::from_utf8(completions.stdout).unwrap();
    assert!(completions.contains("budget"));
    let man = Command::new(jet()).args(["self", "man"]).output().unwrap();
    let man = String::from_utf8(man.stdout).unwrap();
    assert!(man.contains("budget"));
    for flag in ["--annotations","--baseline","--bootstrap","--accept-regression","--reason","--yes","-y"] {
        assert!(completions.contains(flag),"completion omitted {flag}");
        assert!(man.contains(flag),"man page omitted {flag}");
    }
}

#[test]
fn exit_code_ok_check() {
    let p = std::env::temp_dir().join("jet_cli_ok.jet");
    fs::write(&p, "fn run() {\n    print(\"hi\");\n}\n").unwrap();
    let out = Command::new(jet()).arg("check").arg(&p).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "clean check should exit 0");
}

#[test]
fn exit_code_user_error_check() {
    let p = bad_file(&line!().to_string());
    let out = Command::new(jet()).arg("check").arg(&p).output().unwrap();
    assert_eq!(out.status.code(), Some(1), "a program error should exit 1");
}

#[test]
fn exit_code_no_args_starts_repl() {
    // c6vz465: bare `jet` starts the REPL — exit 0 after EOF on piped stdin.
    let out = Command::new(jet()).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "no args should start REPL (exit 0)"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("interactive REPL"),
        "bare jet should print REPL banner:\n{}",
        stdout
    );
}

#[test]
fn exit_code_unknown_subcommand_is_usage() {
    // A typo'd subcommand is a usage error (exit 2) and teaches E2101.
    let out = Command::new(jet()).arg("buld").output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "unknown subcommand should exit 2"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E2101"), "should cite E2101:\n{}", stderr);
    assert!(
        stderr.contains("build"),
        "should suggest `build`:\n{}",
        stderr
    );
}

#[test]
fn gc_report_missing_trace_has_registered_human_and_json_diagnostics() {
    let root = std::env::temp_dir().join(format!(
        "jet_gc_report_missing_{}_{}",
        std::process::id(),
        line!()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();

    let human = Command::new(jet())
        .args(["gc", "report"])
        .current_dir(&root)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(human.status.code(), Some(1));
    assert!(human.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&human.stderr);
    assert!(stderr.contains("Error [E2110]: GC trace cannot be reported"), "{stderr}");
    assert!(stderr.contains("run `jet run --gc-trace <file.jet>`"), "{stderr}");
    assert!(!stderr.contains('\u{1b}'), "{stderr}");
    check_snapshot(
        "gc_report_missing_trace_e2110.txt",
        &stderr.replace(root.to_str().unwrap(), "WORKSPACE"),
    );

    let json = Command::new(jet())
        .args(["gc", "report", "--json"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert_eq!(json.status.code(), Some(1));
    assert!(json.stderr.is_empty());
    let stdout = String::from_utf8_lossy(&json.stdout);
    assert!(stdout.contains("\"code\":\"E2110\""), "{stdout}");
    assert!(stdout.contains("\"severity\":\"error\""), "{stdout}");
    assert!(!stdout.contains('\u{1b}'), "{stdout}");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn frequency_ring_groups_execute_real_handlers() {
    let out = Command::new(jet()).args(["self", "man"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains(".TH JET 1"));

    let out = Command::new(jet()).args(["inspect", "semindex"]).output().unwrap();
    assert_ne!(out.status.code(), Some(2), "group must reach semindex handler");
    assert!(String::from_utf8_lossy(&out.stderr).contains("needs an entry file"));

    let out = Command::new(jet()).args(["hangar", "generations"]).output().unwrap();
    assert_ne!(out.status.code(), Some(2), "existing grouped handler must remain live");
}

#[test]
fn shape6_groups_inspect_and_registry_while_rejecting_bare_actions() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let hello = root.join("examples/features/basics/hello.jet");
    let dossier = Command::new(jet())
        .args(["inspect", "dossier"])
        .arg(&hello)
        .output()
        .unwrap();
    assert!(
        dossier.status.success(),
        "grouped dossier did not reach its handler: {}",
        String::from_utf8_lossy(&dossier.stderr)
    );
    assert!(String::from_utf8_lossy(&dossier.stdout).contains("run"));

    let empty = isolated_cwd("shape6_registry_publish");
    let publish = Command::new(jet())
        .args(["registry", "publish"])
        .current_dir(&empty)
        .output()
        .unwrap();
    assert_eq!(publish.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&publish.stderr).contains("no package.jet found"),
        "grouped publish did not reach its handler: {}",
        String::from_utf8_lossy(&publish.stderr)
    );

    for (bare, canonical) in [
        ("dossier", "jet inspect dossier"),
        ("publish", "jet registry publish"),
    ] {
        let out = Command::new(jet()).arg(bare).output().unwrap();
        assert_eq!(out.status.code(), Some(2));
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("E2101") && stderr.contains(canonical), "{stderr}");
    }
}

#[test]
fn shape_cli_entry_type_drives_shell_inputs_but_remains_optional() {
    let dir = isolated_cwd("shape_cli_entry_source");
    fs::write(
        dir.join("typed.jet"),
        r#"#CLI
struct RunArgs {
    #Doc("person to greet") name: String
    retries: Int = 2
    verbose: Bool
}

fn run(args: RunArgs) {
    print(args.name)
    print(args.retries)
    print(args.verbose)
}
"#,
    )
    .unwrap();
    let typed = Command::new(jet())
        .args(["run", "--release", "typed.jet", "--", "--name", "Ada", "--verbose"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        typed.status.success(),
        "typed entry failed: {}",
        String::from_utf8_lossy(&typed.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&typed.stdout), "Ada\n2\ntrue\n");

    let help = Command::new(jet())
        .args(["run", "--release", "typed.jet", "--", "--help"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(help.status.success());
    let help = String::from_utf8_lossy(&help.stdout);
    for field_fact in ["--name", "person to greet", "--retries", "--verbose"] {
        assert!(help.contains(field_fact), "typed help missing {field_fact}: {help}");
    }
    assert_eq!(
        help.lines()
            .filter(|line| line.trim_start().starts_with("--help"))
            .count(),
        1,
        "generated and Core help both claimed --help:\n{help}"
    );

    let dossier = Command::new(jet())
        .args(["inspect", "dossier", "typed.jet", "run", "--json"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        dossier.status.success(),
        "typed command dossier failed: {}",
        String::from_utf8_lossy(&dossier.stderr)
    );
    let dossier = String::from_utf8(dossier.stdout).unwrap();
    for projected in [
        "\"entry_type\":\"RunArgs\"",
        "\"flag\":\"--name\"",
        "\"value_type\":\"String\"",
        "\"required\":true",
        "\"help\":\"person to greet\"",
        "\"flag\":\"--retries\"",
        "\"default\":\"2\"",
        "\"flag\":\"--verbose\"",
        "\"shape\":\"flag\"",
        "\"completion_words\":[\"--help\",\"name\",\"--name\",\"--retries\",\"--verbose\"]",
    ] {
        assert!(
            dossier.contains(projected),
            "typed command dossier omitted {projected}: {dossier}"
        );
    }

    for shell in ["bash", "zsh", "fish", "powershell"] {
        let completion = Command::new(jet())
            .args(["self", "completions", shell, "--for", "build/typed"])
            .current_dir(&dir)
            .output()
            .unwrap();
        assert!(
            completion.status.success(),
            "{shell} external completion failed: {}",
            String::from_utf8_lossy(&completion.stderr)
        );
        let script = String::from_utf8(completion.stdout).unwrap();
        for flag in ["help", "name", "retries", "verbose"] {
            assert!(script.contains(flag), "{shell} script omitted {flag}: {script}");
        }
        assert!(!script.contains("Ada"), "completion queried a live value: {script}");
        check_snapshot(&format!("shape_cli_for_{shell}.txt"), &script);
    }

    fs::write(dir.join("plain.jet"), "fn run() { print(\"plain\") }\n").unwrap();
    let plain = Command::new(jet())
        .args(["run", "--release", "plain.jet"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        plain.status.success(),
        "plain fn run() became invalid: {}",
        String::from_utf8_lossy(&plain.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&plain.stdout), "plain\n");
    let plain_completion = Command::new(jet())
        .args(["self", "completions", "bash", "--for", "build/plain"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(plain_completion.status.success());
    let plain_script = String::from_utf8(plain_completion.stdout).unwrap();
    assert!(plain_script.contains("--help"));
    assert!(!plain_script.contains("--name"));
    check_snapshot("shape_cli_for_plain.txt", &plain_script);
}

#[test]
fn typed_cli_field_markers_add_short_and_env_inputs_with_pinned_precedence() {
    let dir = isolated_cwd("typed_cli_short_env");
    fs::write(
        dir.join("typed.jet"),
        r#"#CLI
struct RunArgs {
    #[Doc("print extra detail"), Short("v")] verbose: Bool
    #[Doc("port to listen on"), Short("p"), Env("JET_TYPED_PORT")] port: Int = 3000
}

fn run(args: RunArgs) {
    print(args.verbose)
    print(args.port)
}
"#,
    )
    .unwrap();

    let run = |args: &[&str], env: Option<&str>, release: bool| {
        let mut command = Command::new(jet());
        command.arg("run");
        if release {
            command.arg("--release");
        }
        command.arg("typed.jet").arg("--").args(args).current_dir(&dir);
        match env {
            Some(value) => {
                command.env("JET_TYPED_PORT", value);
            }
            None => {
                command.env_remove("JET_TYPED_PORT");
            }
        }
        command.output().unwrap()
    };

    for release in [false, true] {
        let env_fallback = run(&["-v"], Some("4100"), release);
        assert!(
            env_fallback.status.success(),
            "typed CLI env fallback failed: {}",
            String::from_utf8_lossy(&env_fallback.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&env_fallback.stdout), "true\n4100\n");

        let long_wins = run(&["--port", "4200"], Some("4100"), release);
        assert!(long_wins.status.success());
        assert_eq!(String::from_utf8_lossy(&long_wins.stdout), "false\n4200\n");

        let short_wins = run(&["-p", "4300"], Some("4100"), release);
        assert!(short_wins.status.success());
        assert_eq!(String::from_utf8_lossy(&short_wins.stdout), "false\n4300\n");

        let default = run(&[], None, release);
        assert!(default.status.success());
        assert_eq!(String::from_utf8_lossy(&default.stdout), "false\n3000\n");
    }

    let help = run(&["--help"], None, true);
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    for fact in [
        "-v, --verbose",
        "-p, --port PORT",
        "[env: JET_TYPED_PORT]",
        "[default: 3000]",
    ] {
        assert!(help.contains(fact), "typed CLI help omitted {fact}: {help}");
    }
}

#[test]
fn typed_cli_entry_accepts_an_imported_argument_type() {
    let dir = isolated_cwd("shape_cli_imported_entry_type");
    fs::write(
        dir.join("args.jet"),
        r#"#CLI
pub struct RunArgs {
    #Doc("person to greet") pub name: String
    pub retries: Int = 2
    pub verbose: Bool
}
"#,
    )
    .unwrap();
    fs::write(
        dir.join("run.jet"),
        r#"use "args"

fn run(args: RunArgs) {
    print(args.name)
    print(args.retries)
    print(args.verbose)
}
"#,
    )
    .unwrap();

    let run = Command::new(jet())
        .args(["run", "--release", "run.jet", "--", "--name", "Ada", "--verbose"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "imported typed entry failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "Ada\n2\ntrue\n");

    let dossier = Command::new(jet())
        .args(["inspect", "dossier", "run.jet", "run", "--json"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        dossier.status.success(),
        "imported typed command dossier failed: {}",
        String::from_utf8_lossy(&dossier.stderr)
    );
    let dossier = String::from_utf8(dossier.stdout).unwrap();
    for fact in [
        "\"entry_type\":\"RunArgs\"",
        "\"flag\":\"--name\"",
        "\"default\":\"2\"",
        "\"flag\":\"--verbose\"",
    ] {
        assert!(dossier.contains(fact), "imported CLI dossier omitted {fact}: {dossier}");
    }
}

#[test]
fn typed_cli_entry_accepts_an_imported_subcommand_type() {
    let dir = isolated_cwd("shape_cli_imported_subcommand_type");
    fs::write(
        dir.join("commands.jet"),
        r#"#CLI
pub struct ServeArgs { pub port: Int }

#CLI
pub struct ImportArgs { pub file: String }

pub enum Cmd { Serve(ServeArgs) Import(ImportArgs) }
"#,
    )
    .unwrap();
    fs::write(
        dir.join("run.jet"),
        r#"use "commands"

fn run(cmd: Cmd) {}
"#,
    )
    .unwrap();

    let run = Command::new(jet())
        .args(["run", "--release", "run.jet", "--", "serve", "--port", "8080"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "imported subcommand entry failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );

    let dossier = Command::new(jet())
        .args(["inspect", "dossier", "run.jet", "run", "--json"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(dossier.status.success());
    let dossier = String::from_utf8(dossier.stdout).unwrap();
    for fact in [
        "\"entry_type\":\"Cmd\"",
        "\"name\":\"serve\"",
        "\"flag\":\"--port\"",
        "\"name\":\"import\"",
        "\"flag\":\"--file\"",
    ] {
        assert!(dossier.contains(fact), "imported subcommand dossier omitted {fact}: {dossier}");
    }
}

#[test]
fn colliding_imported_cli_type_resolution_stays_in_codegen_sync() {
    let dir = isolated_cwd("shape_cli_ambiguous_imported_type");
    fs::write(
        dir.join("cli.jet"),
        "#CLI\npub struct RunArgs { pub name: String }\n",
    )
    .unwrap();
    fs::write(
        dir.join("plain.jet"),
        "pub struct RunArgs { pub name: String }\n",
    )
    .unwrap();
    fs::write(
        dir.join("run.jet"),
        "use \"plain\"\nuse \"cli\"\nfn run(args: RunArgs) {}\n",
    )
    .unwrap();

    let build = Command::new(jet())
        .args(["build", "run.jet"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&build.stderr);
    assert_ne!(build.status.code(), Some(101), "type ambiguity reached rustc: {stderr}");
    if !build.status.success() {
        assert!(stderr.contains("E1308"), "wrong frontend diagnostic: {stderr}");
    }
    assert!(!stderr.contains("internal compiler error"), "type ambiguity reached rustc: {stderr}");
}

#[test]
fn local_cli_type_wins_over_same_named_import() {
    let dir = isolated_cwd("shape_cli_local_type_precedence");
    fs::write(
        dir.join("other.jet"),
        "pub struct RunArgs { pub name: String }\n",
    )
    .unwrap();
    fs::write(
        dir.join("run.jet"),
        r#"use "other"

#CLI
struct RunArgs { name: String }

fn run(args: RunArgs) { print(args.name) }
"#,
    )
    .unwrap();

    let run = Command::new(jet())
        .args(["run", "--release", "run.jet", "--", "--name", "Ada"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "local CLI type did not win: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "Ada\n");
}

#[test]
fn bare_project_run_prefers_run_jet() {
    let dir = isolated_cwd("run_jet_default_entry");
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.jet"),
        "name: \"entry-default\"\nversion: \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(dir.join("run.jet"), "fn run() { print(\"run.jet\") }\n").unwrap();
    fs::write(
        dir.join("src/main.jet"),
        "fn run() { print(\"legacy main.jet\") }\n",
    )
    .unwrap();

    let run = Command::new(jet())
        .arg("run")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "bare run failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "run.jet\n");
}

#[test]
fn external_completion_metadata_errors_fail_closed() {
    let dir = isolated_cwd("shape_cli_metadata_error");
    fs::write(dir.join("not-a-program"), b"not an executable").unwrap();
    let out = Command::new(jet())
        .args(["self", "completions", "bash", "--for", "not-a-program"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("Error [E2103]:"));
    assert!(stderr.contains("Why:") && stderr.contains("Fix:"));
    check_snapshot("shape_cli_metadata_error_e2103.txt", &stderr);
}

#[test]
fn external_completion_rejects_hostile_files_and_names() {
    let dir = isolated_cwd("shape_cli_hostile_artifacts");
    let oversized = dir.join("oversized");
    fs::File::create(&oversized).unwrap().set_len(512 * 1024 * 1024 + 1).unwrap();
    let oversized_out = Command::new(jet())
        .args(["self", "completions", "bash", "--for", "oversized"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(oversized_out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&oversized_out.stderr).contains("larger than the 512 MiB"));

    #[cfg(unix)]
    {
        let device = Command::new(jet())
            .args(["self", "completions", "bash", "--for", "/dev/null"])
            .output()
            .unwrap();
        assert_eq!(device.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&device.stderr).contains("not a regular file"));

        let fifo = dir.join("program-fifo");
        let made = Command::new("mkfifo").arg(&fifo).status().unwrap();
        assert!(made.success());
        let fifo_out = Command::new(jet())
            .args(["self", "completions", "bash", "--for", "program-fifo"])
            .current_dir(&dir)
            .output()
            .unwrap();
        assert_eq!(fifo_out.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&fifo_out.stderr).contains("not a regular file"));
    }

    let hostile = "safe\nINJECT_COMMAND\nnext";
    fs::write(dir.join(hostile), b"not an executable").unwrap();
    for shell in ["bash", "zsh", "fish", "powershell"] {
        let out = Command::new(jet())
            .args(["self", "completions", shell, "--for", hostile])
            .current_dir(&dir)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert!(out.stdout.is_empty(), "{shell} emitted attacker-controlled script bytes");
        let stderr = String::from_utf8(out.stderr).unwrap();
        assert!(stderr.contains("contains a control character"));
        assert!(!stderr.contains("\nINJECT_COMMAND\n"), "{shell} exposed an executable line: {stderr:?}");
        assert!(stderr.contains("safe\\nINJECT_COMMAND\\nnext"));
    }
}
