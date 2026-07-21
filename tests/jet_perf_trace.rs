//! D-PERFSESSION1=D: `jet perf` writes/reads a versioned `.jettrace`.
//! C1: command family + verify. C2: run/attach capture wall/alloc/tasks with
//! Jet symbol identity from the observe live snapshot.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

mod common;

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
    let root = common::unique_tmp("jet-perf");
    fs::create_dir_all(&root).unwrap();
    root
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let pid = self.0.id();
        let _ = self.0.kill();
        let _ = self.0.wait();
        let _ = fs::remove_file(jet::DevServer::LiveInspect::snapshot_path(pid));
    }
}

fn json_u64_after(haystack: &str, key: &str) -> Option<u64> {
    let needle = format!("\"{key}\":");
    let tail = haystack.split_once(&needle)?.1;
    let digits: String = tail.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// Fail if capture invented 1ns wall or recorded a zero-alloc scrape-success.
fn assert_honest_wall_and_alloc(text: &str) {
    let wall_at = text
        .find("\"domain\":\"wall\"")
        .unwrap_or_else(|| panic!("missing wall sample: {text}"));
    let wall_ns = json_u64_after(&text[wall_at..], "duration_ns")
        .unwrap_or_else(|| panic!("missing wall duration_ns: {text}"));
    assert!(
        wall_ns > 1,
        "fabricated 1ns wall (or zero): duration_ns={wall_ns} in {text}"
    );
    assert!(
        wall_ns >= 1_000_000,
        "wall duration not real observed work: duration_ns={wall_ns} in {text}"
    );

    let alloc_at = text
        .find("\"allocations\":[{")
        .unwrap_or_else(|| panic!("missing allocations object: {text}"));
    let count = json_u64_after(&text[alloc_at..], "count")
        .unwrap_or_else(|| panic!("missing alloc count: {text}"));
    let bytes = json_u64_after(&text[alloc_at..], "bytes")
        .unwrap_or_else(|| panic!("missing alloc bytes: {text}"));
    assert!(
        count > 0,
        "zero alloc count false-green: count={count} bytes={bytes} in {text}"
    );
    assert!(
        bytes > 0,
        "zero alloc bytes false-green: count={count} bytes={bytes} in {text}"
    );
}

/// Fail if tasks are empty, missing root, or missing a spawned child parent link.
fn assert_honest_tasks(text: &str) {
    let tasks_at = text
        .find("\"tasks\":[{")
        .unwrap_or_else(|| panic!("missing non-empty tasks array: {text}"));
    let tasks = &text[tasks_at..];
    assert!(
        tasks.contains("\"parent\":0"),
        "missing root task parent=0: {text}"
    );
    assert!(
        tasks.contains("\"parent\":1") || tasks.contains("\"parent\":2"),
        "missing spawned child parent causality: {text}"
    );
    assert!(
        tasks.contains("\"state\":\"running\"")
            || tasks.contains("\"state\":\"blocked\"")
            || tasks.contains("\"state\":\"queued\""),
        "missing live observe task state: {text}"
    );
    assert!(
        !text.contains("\"tasks\":[]"),
        "empty tasks false-green leaked beside non-empty claim: {text}"
    );
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

#[test]
fn perf_run_captures_wall_and_alloc_into_jettrace() {
    if !common::have_rustc() {
        return;
    }
    let root = temp_workspace();
    fs::create_dir_all(&root).unwrap();
    let source = root.join("session.jet");
    // Non-entry `probe_work` must appear in source_identity; sample symbol is
    // parsed `fn run` only — never a hardcoded invention. Arena stays live in
    // `run` so observe still sees outstanding allocations during sleep.
    // Spawned child sleeps longer than parent so poll sees parent causality.
    fs::write(
        &source,
        r#"use core.mem as mem
use core.tasks as tasks
use core.time as time

fn probe_work() {
    // Present only so source_identity must parse a non-entry function spelling.
}

fn run() {
    arena :: mem.Arena.new()
    x :: arena.alloc(42)
    probe_work()
    child :: tasks.spawn(() => {
        time.sleep(2000)
    })
    print("READY")
    time.sleep(800)
}
"#,
    )
    .unwrap();

    let out = root.join("run-capture.jettrace");
    let output = run_jet(
        &root,
        &[
            "perf",
            "run",
            source.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "perf run failed: status={:?} stderr={stderr}",
        output.status.code()
    );
    assert!(stderr.contains("trace:"), "{stderr}");
    assert!(out.is_file(), "missing {}", out.display());
    let text = fs::read_to_string(&out).unwrap();
    assert_honest_wall_and_alloc(&text);
    assert_honest_tasks(&text);
    assert!(text.contains("\"name\":\"probe_work\""), "parsed fn missing: {text}");
    assert!(text.contains("\"name\":\"run\""), "{text}");
    assert!(text.contains("session.jet"), "{text}");
    assert!(
        !text.contains("\"duration_ns\":1,"),
        "fabricated 1ns wall leaked into trace: {text}"
    );
    assert!(
        !text.contains("\"count\":0,"),
        "zero-alloc scrape-success leaked into trace: {text}"
    );

    let view = run_jet(&root, &["perf", "view", out.to_str().unwrap()]);
    let stdout = String::from_utf8_lossy(&view.stdout);
    assert!(view.status.success(), "{}", String::from_utf8_lossy(&view.stderr));
    assert!(stdout.contains("sample wall"), "{stdout}");
    assert!(stdout.contains("alloc count="), "{stdout}");
    assert!(!stdout.contains("alloc count=0 "), "zero alloc in view: {stdout}");
    assert!(stdout.contains("tasks count="), "{stdout}");
    assert!(stdout.contains("children="), "{stdout}");
    assert!(!stdout.contains("tasks count=0 "), "zero tasks in view: {stdout}");
    assert!(stdout.contains("session.jet#run"), "{stdout}");
    assert!(stdout.contains("command run"), "{stdout}");

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn perf_attach_captures_wall_and_alloc_with_jet_symbol_from_observe() {
    if !common::have_rustc() {
        return;
    }
    let root = temp_workspace();
    fs::create_dir_all(&root).unwrap();
    let source = root.join("live.jet");
    fs::write(
        &source,
        r#"use core.mem as mem
use core.tasks as tasks
use core.time as time

fn probe_work() {
    // Present only so source_identity must parse a non-entry function spelling.
}

fn run() {
    arena :: mem.Arena.new()
    x :: arena.alloc(42)
    probe_work()
    child :: tasks.spawn(() => {
        time.sleep(30000)
    })
    print("READY")
    time.sleep(30000)
}
"#,
    )
    .unwrap();

    let build = run_jet(&root, &["build", source.to_str().unwrap()]);
    assert!(
        build.status.success(),
        "build failed:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let binary = root
        .join("build")
        .join(format!("live{}", std::env::consts::EXE_SUFFIX));
    let mut child = Command::new(&binary)
        .env("JET_OBSERVE", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut ready = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    assert_eq!(
        ready.trim(),
        "READY",
        "observed program did not become ready: {ready:?}"
    );
    let pid = child.id();
    let _guard = ChildGuard(child);

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match jet::DevServer::LiveInspect::read(pid) {
            Ok(snapshot)
                if snapshot.contains("\"arena_allocations\":")
                    && !snapshot.contains("\"arena_allocations\":0")
                    && snapshot.contains("\"parent\":1") =>
            {
                break
            }
            _ if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            other => panic!(
                "observe snapshot never became readable with allocs+child task: {other:?}"
            ),
        }
    }
    // Ensure /proc starttime → wall ticks clear the honest >0 / >=1ms floor.
    std::thread::sleep(Duration::from_millis(50));

    let out = root.join("capture.jettrace");
    let attach = run_jet(
        &root,
        &[
            "perf",
            "attach",
            &pid.to_string(),
            "--source",
            source.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ],
    );
    let stderr = String::from_utf8_lossy(&attach.stderr);
    assert!(
        attach.status.success(),
        "attach capture failed: status={:?} stderr={stderr}",
        attach.status.code()
    );
    assert!(out.is_file(), "missing {}", out.display());
    let text = fs::read_to_string(&out).unwrap();
    assert_honest_wall_and_alloc(&text);
    assert_honest_tasks(&text);
    assert!(text.contains("\"name\":\"probe_work\""), "parsed fn missing: {text}");
    assert!(text.contains("\"name\":\"run\""), "{text}");
    assert!(text.contains("live.jet"), "{text}");
    assert!(
        !text.contains("\"duration_ns\":1,"),
        "fabricated 1ns wall leaked into trace: {text}"
    );
    assert!(
        !text.contains("\"count\":0,"),
        "zero-alloc scrape-success leaked into trace: {text}"
    );

    let view = run_jet(&root, &["perf", "view", out.to_str().unwrap()]);
    let stdout = String::from_utf8_lossy(&view.stdout);
    assert!(view.status.success(), "{}", String::from_utf8_lossy(&view.stderr));
    assert!(stdout.contains("sample wall"), "{stdout}");
    assert!(stdout.contains("alloc count="), "{stdout}");
    assert!(!stdout.contains("alloc count=0 "), "zero alloc in view: {stdout}");
    assert!(stdout.contains("tasks count="), "{stdout}");
    assert!(stdout.contains("children="), "{stdout}");
    assert!(!stdout.contains("tasks count=0 "), "zero tasks in view: {stdout}");
    assert!(stdout.contains("live.jet#run"), "{stdout}");

    let _ = fs::remove_dir_all(&root);
}
