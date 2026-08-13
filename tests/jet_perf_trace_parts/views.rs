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
