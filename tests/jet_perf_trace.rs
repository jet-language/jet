//! D-PERFSESSION1=D C1: `jet perf` writes/reads a versioned `.jettrace` skeleton.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn jet() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

fn run_jet(cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::new(jet())
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("jet {:?} failed to launch: {e}", args))
}

fn temp_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "jet-perf-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn perf_attach_view_compare_export_share_one_jettrace_truth() {
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
fn perf_run_accepts_base_surface_and_writes_jettrace_before_driver() {
    let root = temp_workspace();
    // Missing program: still proves argv acceptance + skeleton write, then the
    // exact `jet run` driver owns the subsequent failure.
    let missing = root.join("missing.jet");
    let output = run_jet(
        &root,
        &["perf", "run", missing.to_str().unwrap(), "--", "--port", "9"],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("trace:"),
        "expected jettrace write before driver failure: {stderr}"
    );
    let trace_line = stderr
        .lines()
        .find(|line| line.starts_with("trace: "))
        .unwrap();
    let path = PathBuf::from(trace_line.trim_start_matches("trace: ").trim());
    assert!(path.is_file(), "trace path missing: {}", path.display());
    assert!(
        path.extension().and_then(|e| e.to_str()) == Some("jettrace")
            || path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".jettrace")),
        "expected .jettrace path, got {}",
        path.display()
    );
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("\"schema\":\"jet.trace\""), "{text}");
    assert!(text.contains("\"command\":\"run\""), "{text}");
    assert!(text.contains("--port"), "{text}");
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
