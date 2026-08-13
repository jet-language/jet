#[test]
fn perf_run_accepts_base_surface_and_writes_jettrace_before_driver() {
    let root = temp_workspace();
    let missing = root.join("missing.jet");
    let out = root.join("missing.jettrace");
    let output = run_jet(
        &root,
        &[
            "perf",
            "run",
            missing.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--",
            "--port",
            "9",
        ],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("trace:"),
        "expected jettrace write after base driver: {stderr}"
    );
    assert!(out.is_file(), "trace path missing: {}", out.display());
    let text = fs::read_to_string(&out).unwrap();
    assert!(text.contains("\"schema\":\"jet.trace\""), "{text}");
    assert!(text.contains("\"command\":\"run\""), "{text}");
    assert!(text.contains("--port"), "{text}");
    // Missing source cannot attribute samples; skeleton domains stay empty.
    assert!(text.contains("\"samples\":[]"), "{text}");
    let _ = fs::remove_dir_all(&root);
}
#[test]
fn perf_help_lists_family() {
    let output = run_jet(Path::new("."), &["perf", "help"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert!(stdout.contains("jet perf"), "{stdout}");
    for verb in ["run", "test", "bench", "attach", "view", "compare", "export"] {
        assert!(stdout.contains(verb), "missing {verb} in {stdout}");
    }
}
