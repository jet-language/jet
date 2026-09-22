#[test]
fn perf_attach_view_compare_export_share_one_jettrace_truth() {
    let _guard = SELF_ATTACH_LOCK.lock().unwrap();
    let root = temp_workspace();
    let pid = std::process::id().to_string();
    let out = root.join("session.jettrace");

    let attach = run_jet(
        &root,
        &["perf", "attach", &pid, "--out", out.to_str().unwrap()],
    );
    let stderr = String::from_utf8_lossy(&attach.stderr);
    assert!(
        attach.status.success(),
        "attach failed: status={:?} stderr={stderr}",
        attach.status.code()
    );
    assert!(stderr.contains("trace:"), "{stderr}");
    assert!(out.is_file(), "missing {}", out.display());

    let bytes = fs::read(&out).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.contains("\"schema\":\"jet.trace\""), "{text}");
    assert!(text.contains("\"version\":1"), "{text}");
    assert!(text.contains("\"trace_id\":"), "{text}");
    assert!(text.contains("\"capture_policy\":"), "{text}");

    let view = run_jet(&root, &["perf", "view", out.to_str().unwrap()]);
    let stdout = String::from_utf8_lossy(&view.stdout);
    assert!(view.status.success(), "view failed: {}", String::from_utf8_lossy(&view.stderr));
    assert!(stdout.contains("schema jet.trace v1"), "{stdout}");
    assert!(stdout.contains("command attach"), "{stdout}");

    let view_json = run_jet(&root, &["perf", "view", out.to_str().unwrap(), "--json"]);
    let view_json_out = String::from_utf8_lossy(&view_json.stdout);
    assert!(view_json.status.success(), "{}", String::from_utf8_lossy(&view_json.stderr));
    assert!(view_json_out.contains("\"kind\":\"jet.trace.view\""), "{view_json_out}");
    assert!(view_json_out.contains("\"timeline\":"), "{view_json_out}");
    assert!(view_json_out.contains("\"flamegraph\":"), "{view_json_out}");

    let view_html = run_jet(&root, &["perf", "view", out.to_str().unwrap(), "--html"]);
    let html = String::from_utf8_lossy(&view_html.stdout);
    assert!(view_html.status.success(), "{}", String::from_utf8_lossy(&view_html.stderr));
    assert!(html.contains("<!doctype html>"), "{html}");
    assert!(html.contains("flamegraph"), "{html}");
    assert!(html.contains("timeline"), "{html}");

    let no_color = Command::new(jet())
        .current_dir(&root)
        .env("NO_COLOR", "1")
        .args(["perf", "view", out.to_str().unwrap(), "--frames=all"])
        .output()
        .unwrap();
    let no_color_out = String::from_utf8_lossy(&no_color.stdout);
    assert!(no_color.status.success(), "{}", String::from_utf8_lossy(&no_color.stderr));
    assert!(!no_color_out.contains('\u{1b}'), "NO_COLOR leaked ANSI: {no_color_out}");
    assert!(no_color_out.contains("frames all"), "{no_color_out}");
    assert!(no_color_out.contains("generated-frames:"), "{no_color_out}");

    let compare = run_jet(
        &root,
        &[
            "perf",
            "compare",
            out.to_str().unwrap(),
            out.to_str().unwrap(),
        ],
    );
    assert!(
        compare.status.success(),
        "compare failed: {}",
        String::from_utf8_lossy(&compare.stderr)
    );
    assert!(
        String::from_utf8_lossy(&compare.stdout).contains("compare ok"),
        "{}",
        String::from_utf8_lossy(&compare.stdout)
    );

    let export = run_jet(&root, &["perf", "export", out.to_str().unwrap(), "--json"]);
    let exported = String::from_utf8_lossy(&export.stdout);
    assert!(export.status.success(), "export failed: {}", String::from_utf8_lossy(&export.stderr));
    assert!(exported.contains("\"kind\":\"jet.trace.projection\""), "{exported}");
    assert!(exported.contains("\"loss\":"), "{exported}");
    assert!(exported.contains("\"schema\":\"jet.trace\""), "{exported}");
    assert!(exported.contains("--chrome"), "{exported}");
    assert!(!exported.contains("no pprof/otel/chrome payloads"), "{exported}");

    let pprof = run_jet(&root, &["perf", "export", out.to_str().unwrap(), "--pprof"]);
    let pprof_out = String::from_utf8_lossy(&pprof.stdout);
    assert!(pprof.status.success(), "{}", String::from_utf8_lossy(&pprof.stderr));
    assert!(pprof_out.contains("\"kind\":\"jet.trace.pprof-projection\""), "{pprof_out}");
    assert!(pprof_out.contains("\"loss\":"), "{pprof_out}");

    let otel = run_jet(&root, &["perf", "export", out.to_str().unwrap(), "--otel"]);
    assert!(otel.status.success(), "{}", String::from_utf8_lossy(&otel.stderr));
    assert!(
        String::from_utf8_lossy(&otel.stdout).contains("otel-projection"),
        "{}",
        String::from_utf8_lossy(&otel.stdout)
    );

    let chrome = run_jet(&root, &["perf", "export", out.to_str().unwrap(), "--chrome"]);
    assert!(chrome.status.success(), "{}", String::from_utf8_lossy(&chrome.stderr));
    assert!(
        String::from_utf8_lossy(&chrome.stdout).contains("chrome-projection"),
        "{}",
        String::from_utf8_lossy(&chrome.stdout)
    );
    assert!(
        String::from_utf8_lossy(&chrome.stdout).contains("\"traceEvents\":["),
        "{}",
        String::from_utf8_lossy(&chrome.stdout)
    );

    let profile_map = run_jet(
        &root,
        &["perf", "export", out.to_str().unwrap(), "--emit-profile-map"],
    );
    assert!(
        profile_map.status.success(),
        "{}",
        String::from_utf8_lossy(&profile_map.stderr)
    );
    assert!(
        String::from_utf8_lossy(&profile_map.stdout).contains("profile-map-projection"),
        "{}",
        String::from_utf8_lossy(&profile_map.stdout)
    );

    // Identity override path stays available for mismatched hardware/toolchain.
    let overridden = run_jet(
        &root,
        &[
            "perf",
            "compare",
            out.to_str().unwrap(),
            out.to_str().unwrap(),
            "--override-identity",
        ],
    );
    assert!(
        overridden.status.success(),
        "{}",
        String::from_utf8_lossy(&overridden.stderr)
    );
    assert!(
        String::from_utf8_lossy(&overridden.stdout).contains("budgets:"),
        "{}",
        String::from_utf8_lossy(&overridden.stdout)
    );

    let corrupt = root.join("corrupt.jettrace");
    fs::write(&corrupt, b"{\"schema\":\"jet.trace\"}\n").unwrap();
    let bad = run_jet(&root, &["perf", "view", corrupt.to_str().unwrap()]);
    assert_eq!(bad.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&bad.stderr).contains("jettrace"),
        "{}",
        String::from_utf8_lossy(&bad.stderr)
    );

    let _ = fs::remove_dir_all(&root);
}
#[test]
fn perf_view_reads_hash_valid_legacy_capture_policy_v1() {
    let _guard = SELF_ATTACH_LOCK.lock().unwrap();
    let root = temp_workspace();
    let modern_path = root.join("modern.jettrace");
    let attach = run_jet(
        &root,
        &[
            "perf",
            "attach",
            &std::process::id().to_string(),
            "--out",
            modern_path.to_str().unwrap(),
        ],
    );
    assert!(attach.status.success(), "{}", String::from_utf8_lossy(&attach.stderr));
    let modern_bytes = fs::read(&modern_path).unwrap();
    let modern = verify_jettrace(&modern_bytes).unwrap();
    let modern_id = trace_id(&modern).unwrap().to_string();

    let parsed = CanonicalJson::parse_canonical(&modern_bytes).unwrap();
    let CanonicalJson::Object(mut wrapper) = parsed else {
        panic!("modern trace wrapper is not an object")
    };
    let mut content = wrapper.remove("content").unwrap();
    let CanonicalJson::Object(fields) = &mut content else {
        panic!("modern trace content is not an object")
    };
    let CanonicalJson::Object(policy) = fields.get_mut("capture_policy").unwrap() else {
        panic!("modern capture policy is not an object")
    };
    for key in [
        "browser_row_limit",
        "browser_rows_truncated",
        "io_row_limit",
        "io_rows_truncated",
        "native_row_limit",
        "native_rows_truncated",
        "span_row_limit",
        "span_rows_truncated",
        "task_row_limit",
        "task_rows_truncated",
    ] {
        policy.remove(key);
    }
    policy.insert("schema".into(), CanonicalJson::Integer("1".into()));
    let legacy_bytes = jettrace_artifact(content).bytes();
    let legacy = verify_jettrace(&legacy_bytes).unwrap();
    assert_ne!(trace_id(&legacy).unwrap(), modern_id, "legacy trace_id was not recomputed");

    let legacy_path = root.join("legacy-v1.jettrace");
    fs::write(&legacy_path, legacy_bytes).unwrap();
    let view = run_jet(&root, &["perf", "view", legacy_path.to_str().unwrap()]);
    let stdout = String::from_utf8_lossy(&view.stdout);
    assert!(view.status.success(), "{}", String::from_utf8_lossy(&view.stderr));
    assert!(stdout.contains("schema jet.trace v1"), "{stdout}");
    assert!(stdout.contains("command attach"), "{stdout}");
    let _ = fs::remove_dir_all(&root);
}
fn profiler_fixture_source() -> &'static str {
    "fn run() {\n    hot()\n}\nfn hot() {\n}\nfn cold() {\n}\n"
}

fn profiler_fixture_row(
    clock: &str,
    name: &str,
    start_line: u64,
    end_column: u64,
    sample_count: u64,
    source_sha256: &str,
) -> TraceProfileRow {
    TraceProfileRow {
        clock: clock.into(),
        execution_count: None,
        sample_count,
        sample_weight: sample_count,
        source: TraceProfileSource {
            path: "profile.jet".into(),
            sha256: source_sha256.into(),
            start_line,
            start_column: 1,
            end_line: start_line,
            end_column,
        },
        symbol: JetSymbolRef {
            path: "profile.jet".into(),
            name: name.into(),
        },
    }
}

fn profiler_fixture(state: &str) -> Vec<u8> {
    let source = profiler_fixture_source();
    let source_sha256 = SHA256::sha256_hex(source.as_bytes());
    let hot_wall = profiler_fixture_row("wall", "hot", 4, 7, 37, &source_sha256);
    let hot_cpu = profiler_fixture_row("cpu", "hot", 4, 7, 11, &source_sha256);
    let cold_wall = profiler_fixture_row("wall", "cold", 6, 8, 1, &source_sha256);
    let profile = match state {
        "captured" => TraceProfile {
            method: "sampling".into(),
            status: "captured".into(),
            coverage: "complete".into(),
            rows: vec![hot_wall, hot_cpu, cold_wall],
            row_limit: TRACE_PROFILE_ROW_LIMIT as u64,
            rows_truncated: false,
            overhead_ns: Some(3),
            overhead_status: "measured".into(),
            overhead_reason: "fixture sampling overhead".into(),
            reason: "source-attributed sampling retained hot and cold rows".into(),
        },
        "no_samples" => TraceProfile {
            method: "sampling".into(),
            status: "no_samples".into(),
            coverage: "none".into(),
            rows: Vec::new(),
            row_limit: TRACE_PROFILE_ROW_LIMIT as u64,
            rows_truncated: false,
            overhead_ns: None,
            overhead_status: "not_measured".into(),
            overhead_reason: "no source samples were retained".into(),
            reason: "the observe channel produced no source-attributed samples".into(),
        },
        "unsupported" => TraceProfile::unsupported("source sampling is unsupported for fixture"),
        "truncated" => TraceProfile {
            method: "sampling".into(),
            status: "truncated".into(),
            coverage: "partial".into(),
            rows: vec![hot_wall],
            row_limit: TRACE_PROFILE_ROW_LIMIT as u64,
            rows_truncated: true,
            overhead_ns: None,
            overhead_status: "not_measured".into(),
            overhead_reason: "no matched non-profiled run was captured".into(),
            reason: "sampling observations exceeded the bounded profile sample limit".into(),
        },
        "dropped" => TraceProfile {
            method: "sampling".into(),
            status: "truncated".into(),
            coverage: "partial".into(),
            rows: vec![hot_wall],
            row_limit: TRACE_PROFILE_ROW_LIMIT as u64,
            rows_truncated: true,
            overhead_ns: None,
            overhead_status: "not_measured".into(),
            overhead_reason: "no matched non-profiled run was captured".into(),
            reason: "dropped_rows=1 at bounded profile sample limit".into(),
        },
        _ => panic!("unknown profiler fixture state {state}"),
    };
    let skeleton = TraceSkeleton {
        command: "run".into(),
        argv: vec!["run".into(), "profile.jet".into()],
        toolchain: TraceToolchain {
            jet_version: "fixture".into(),
            compiler_build_id: "profile-fixture".into(),
            stdlib_id: "profile-stdlib".into(),
            runner_id: "profile-runner".into(),
        },
        hardware: TraceHardware {
            cpu_arch: "x86_64".into(),
            logical_cpus: 1,
            os: "fixture".into(),
            target: "profile-target".into(),
        },
        capture_policy: CapturePolicy::default_exclusions(),
        samples: Vec::new(),
        profile,
        allocations: Vec::new(),
        browser: Vec::new(),
        game_frames: Vec::new(),
        game_draw_events: Vec::new(),
        tasks: Vec::new(),
        locks: Vec::new(),
        io: Vec::new(),
        native: Vec::new(),
        spans: Vec::new(),
        receipt_sections: Vec::new(),
        source_identity: vec![SourceIdentity {
            path: "profile.jet".into(),
            sha256: source_sha256,
            symbols: vec![
                ("cold".into(), "fn".into()),
                ("hot".into(), "fn".into()),
                ("run".into(), "fn".into()),
            ],
        }],
        source_maps: vec![TraceSourceMap::jet_with_source("profile.jet", source)],
    };
    build_skeleton_bytes(&skeleton).unwrap()
}

#[test]
fn perf_source_profile_view_preserves_ranges_and_coverage_states() {
    let root = temp_workspace();
    let source_sha256 = SHA256::sha256_hex(profiler_fixture_source().as_bytes());
    let captured_path = root.join("captured-profile.jettrace");
    let captured = profiler_fixture("captured");
    verify_jettrace(&captured).unwrap();
    let captured_text = String::from_utf8(captured.clone()).unwrap();
    assert!(captured_text.contains("\"method\":\"sampling\""), "{captured_text}");
    assert!(captured_text.contains("\"execution_count\":null"), "{captured_text}");
    assert!(captured_text.contains("\"sample_count\":37"), "{captured_text}");
    assert!(captured_text.contains("\"sample_weight\":37"), "{captured_text}");
    assert!(captured_text.contains(&format!("\"sha256\":\"{source_sha256}\"")), "{captured_text}");
    assert!(captured_text.contains("\"start_line\":4"), "{captured_text}");
    assert!(captured_text.contains("\"end_column\":7"), "{captured_text}");
    fs::write(&captured_path, captured).unwrap();

    let view = run_jet(&root, &["perf", "view", captured_path.to_str().unwrap()]);
    let view_out = String::from_utf8_lossy(&view.stdout);
    assert!(view.status.success(), "{}", String::from_utf8_lossy(&view.stderr));
    assert!(
        view_out.contains("profile method=sampling status=captured coverage=complete rows=3 clocks=cpu,wall overhead=measured"),
        "{view_out}"
    );
    assert!(view_out.contains("profile.jet#hot profile:wall samples=37"), "{view_out}");
    assert!(view_out.contains("profile.jet#cold profile:wall samples=1"), "{view_out}");
    assert!(!view_out.contains("samples=0"), "{view_out}");

    let view_json = run_jet(&root, &["perf", "view", captured_path.to_str().unwrap(), "--json"]);
    let view_json_out = String::from_utf8_lossy(&view_json.stdout);
    assert!(view_json.status.success(), "{}", String::from_utf8_lossy(&view_json.stderr));
    for marker in [
        "\"kind\":\"jet.trace.view\"",
        "\"method\":\"sampling\"",
        "\"execution_count\":null",
        "\"sample_count\":37",
        "\"sample_weight\":37",
        "\"start_line\":4",
        "\"end_column\":7",
        "\"coverage\":\"complete\"",
    ] {
        assert!(view_json_out.contains(marker), "{marker}: {view_json_out}");
    }
    assert!(view_json_out.contains(&format!("\"sha256\":\"{source_sha256}\"")), "{view_json_out}");

    let profile_map = run_jet(
        &root,
        &["perf", "export", captured_path.to_str().unwrap(), "--emit-profile-map"],
    );
    let profile_map_out = String::from_utf8_lossy(&profile_map.stdout);
    assert!(profile_map.status.success(), "{}", String::from_utf8_lossy(&profile_map.stderr));
    for marker in [
        "\"kind\":\"jet.trace.profile-map-projection\"",
        "\"source_identity\":[{\"path\":\"profile.jet\"",
        "\"source_maps\":[{\"kind\":\"jet\"",
        "\"method\":\"sampling\"",
        "\"sample_weight\":37",
        "\"start_line\":4",
        "\"coverage\":\"complete\"",
    ] {
        assert!(profile_map_out.contains(marker), "{marker}: {profile_map_out}");
    }

    for (state, summary) in [
        ("no_samples", "status=no_samples coverage=none rows=0 clocks=none"),
        ("unsupported", "status=unsupported coverage=unsupported rows=0 clocks=none"),
        ("truncated", "status=truncated coverage=partial rows=1 clocks=wall"),
        ("dropped", "status=truncated coverage=partial rows=1 clocks=wall"),
    ] {
        let path = root.join(format!("{state}.jettrace"));
        let bytes = profiler_fixture(state);
        verify_jettrace(&bytes).unwrap();
        fs::write(&path, bytes).unwrap();
        let view = run_jet(&root, &["perf", "view", path.to_str().unwrap()]);
        let view_out = String::from_utf8_lossy(&view.stdout);
        assert!(view.status.success(), "{state}: {}", String::from_utf8_lossy(&view.stderr));
        assert!(
            view_out.contains(&format!("profile method=sampling {summary}")),
            "{state}: {view_out}"
        );
        assert!(!view_out.contains("coverage=complete"), "{state}: {view_out}");
        assert!(!view_out.contains("samples=0"), "{state}: {view_out}");
        match state {
            "no_samples" | "unsupported" => {
                assert!(!view_out.contains("profile:wall"), "{state}: {view_out}");
            }
            "truncated" | "dropped" => {
                assert!(view_out.contains("profile.jet#hot profile:wall samples=37"), "{state}: {view_out}");
            }
            _ => unreachable!(),
        }
        if state == "dropped" {
            let view_json = run_jet(&root, &["perf", "view", path.to_str().unwrap(), "--json"]);
            let view_json_out = String::from_utf8_lossy(&view_json.stdout);
            assert!(view_json.status.success(), "{}", String::from_utf8_lossy(&view_json.stderr));
            assert!(view_json_out.contains("\"reason\":\"dropped_rows=1 at bounded profile sample limit\""));
            assert!(view_json_out.contains("\"rows_truncated\":true"));
            assert!(!view_json_out.contains("\"coverage\":\"complete\""));
        }
    }

    let _ = fs::remove_dir_all(&root);
}
