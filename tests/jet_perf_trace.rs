//! D-PERFSESSION1=D: `jet perf` writes/reads a versioned `.jettrace`.
//! C1: command family + verify. C2: run/attach capture
//! wall/alloc/tasks/locks/io/native timing with Jet symbol identity from the
//! observe live snapshot.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use jet_foundation::JetTrace::{jettrace_artifact, trace_id, verify_jettrace};
use jet_foundation::PerformanceBudget::CanonicalJson;

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

fn assert_completed_io_bound_to_tasks(text: &str) {
    let io = text
        .split_once("\"io\":[")
        .and_then(|(_, tail)| tail.split_once("],\"locks\""))
        .map(|(array, _)| array)
        .unwrap_or_else(|| panic!("missing io array: {text}"));
    let tasks = text
        .split_once("\"tasks\":[")
        .and_then(|(_, tail)| tail.split_once("],\"toolchain\""))
        .map(|(array, _)| array)
        .unwrap_or_else(|| panic!("missing tasks array: {text}"));
    let mut spans = 0;
    for span in io.split("},{") {
        let task_id = json_u64_after(span, "task_id")
            .unwrap_or_else(|| panic!("I/O span missing task_id: {span}"));
        let task = tasks
            .split("},{")
            .find(|task| json_u64_after(task, "id") == Some(task_id))
            .unwrap_or_else(|| panic!("I/O task {task_id} missing: {text}"));
        assert!(task.contains("\"state\":\"done\""), "I/O task {task_id} not done: {task}");
        assert!(task.contains("\"wait\":\"\""), "done I/O task {task_id} retained wait: {task}");
        let start = json_u64_after(span, "start_ns").unwrap();
        let end = json_u64_after(span, "end_ns").unwrap();
        assert!(end > start, "completed I/O task {task_id} has no duration: {span}");
        spans += 1;
    }
    assert!(spans > 0, "no completed I/O spans: {text}");
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
            || tasks.contains("\"state\":\"queued\"")
            || tasks.contains("\"state\":\"done\""),
        "missing live observe task state: {text}"
    );
    assert!(
        !text.contains("\"tasks\":[]"),
        "empty tasks false-green leaked beside non-empty claim: {text}"
    );
}

/// Fail if locks empty, not channel, or missing real waiters (idle scrape).
fn assert_honest_locks(text: &str) {
    let locks_at = text
        .find("\"locks\":[{")
        .unwrap_or_else(|| panic!("missing non-empty locks array: {text}"));
    let locks = &text[locks_at..];
    assert!(
        locks.contains("\"kind\":\"channel\""),
        "missing channel lock kind: {text}"
    );
    assert!(
        locks.contains("\"recv_waiters\":1")
            || locks.contains("\"recv_waiters\":2")
            || locks.contains("\"send_waiters\":1")
            || locks.contains("\"send_waiters\":2"),
        "missing contended waiters: {text}"
    );
    assert!(
        !text.contains("\"locks\":[]"),
        "empty locks false-green leaked beside non-empty claim: {text}"
    );
    assert!(
        !locks.contains("\"recv_waiters\":0,\"send_waiters\":0")
            && !locks.contains("\"send_waiters\":0,\"recv_waiters\":0"),
        "idle zero-waiter lock row leaked: {text}"
    );
}

/// Fail if io empty, missing tcp wait, or vacuous non-I/O wait leaked.
fn assert_honest_io(text: &str) {
    let io_at = text
        .find("\"io\":[{")
        .unwrap_or_else(|| panic!("missing non-empty io array: {text}"));
    let after = &text[io_at + "\"io\":[".len()..];
    let end = after
        .find(']')
        .unwrap_or_else(|| panic!("unclosed io array: {text}"));
    let io = &after[..end];
    assert!(
        io.contains("\"kind\":\"tcp\""),
        "missing tcp io kind: {text}"
    );
    assert!(
        io.contains("\"wait\":\"tcp accept\"") || io.contains("\"wait\":\"tcp accept readiness\""),
        "missing real tcp accept wait: {text}"
    );
    assert!(
        io.contains("\"task_id\":"),
        "missing io task_id: {text}"
    );
    let start = json_u64_after(io, "start_ns")
        .unwrap_or_else(|| panic!("missing session-relative io start_ns: {text}"));
    let end = json_u64_after(io, "end_ns")
        .unwrap_or_else(|| panic!("missing session-relative io end_ns: {text}"));
    assert!(end >= start, "io span runs backward: start={start} end={end} in {text}");
    assert!(
        !text.contains("\"io\":[]"),
        "empty io false-green leaked beside non-empty claim: {text}"
    );
    assert!(
        !io.contains("\"wait\":\"time sleep\"") && !io.contains("\"wait\":\"channel "),
        "non-I/O wait leaked into io: {io}"
    );
}

fn assert_honest_native(text: &str, expect_elapsed: bool) {
    let native_at = text
        .find("\"native\":[{")
        .unwrap_or_else(|| panic!("missing native timing: {text}"));
    let native = &text[native_at..];
    assert!(native.contains("\"clock\":\"process_cpu\""), "{text}");
    assert!(native.contains("\"status\":\"captured\""), "{text}");
    assert!(native.contains("\"duration_ns\":"), "{text}");
    let observed = json_u64_after(native, "observed_at_ns")
        .unwrap_or_else(|| panic!("missing native session-relative clock: {text}"));
    if expect_elapsed {
        assert!(observed > 0, "run native observation did not advance: {text}");
    }
    assert!(native.contains("\"task_id\":1"), "{text}");
    assert!(native.contains("\"target\":"), "{text}");
    assert!(text.contains("\"native_row_limit\":1"), "{text}");
    assert!(text.contains("\"native_rows_truncated\":false"), "{text}");
}

#[test]
fn perf_run_keeps_completed_socket_echo_io_span() {
    if !common::have_rustc() {
        return;
    }
    let root = temp_workspace();
    let source = root.join("socket_echo.jet");
    fs::write(
        &source,
        r#"use core.net as net
use core.tasks as tasks
use core.time as time

fn run() {
    listener :: net.tcp_listen("127.0.0.1:0") ?? panic("bind")
    address :: net.socket_to_string(net.listener_local_socket_addr(listener) ?? panic("address"))
    server :: tasks.spawn(take(listener) () => {
        stream :: listener.accept() ?? panic("accept")
        message :: stream.read_text(16) ?? panic("read")
        stream.write_all("echo:{message}".bytes()) ?? panic("write")
    })
    // Cross two 100 ms observe publications before completing the wait.
    time.sleep(250)
    client :: net.tcp_connect(address) ?? panic("connect")
    client.write_all("ping".bytes()) ?? panic("write")
    print(client.read_text(16) ?? panic("read"))
    server.join()
}
"#,
    )
    .unwrap();
    let out = root.join("socket-echo.jettrace");
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
    assert!(
        output.status.success(),
        "socket echo did not complete normally: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "echo:ping");
    let text = fs::read_to_string(&out).unwrap();
    assert_honest_io(&text);
    assert_completed_io_bound_to_tasks(&text);
    let _ = fs::remove_dir_all(&root);
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
fn perf_view_reads_hash_valid_legacy_capture_policy_v1() {
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
        "io_row_limit",
        "io_rows_truncated",
        "native_row_limit",
        "native_rows_truncated",
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
    // Child blocks on channel receive so poll sees contention + parent link.
    fs::write(
        &source,
        r#"use core.mem as mem
use core.net as net
use core.tasks as tasks
use core.time as time

fn probe_work() {
    // Present only so source_identity must parse a non-entry function spelling.
}

fn run() {
    // Channel setup before arena so spawn panic cannot capture the arena view.
    (ready_sender, ready) :: tasks.channel<Int>()
    (hold_sender, blocked) :: tasks.channel<Int>(1)
    child :: tasks.spawn(take(ready_sender, blocked) () => {
        ready_sender.send(1)
        blocked.receive() ?? panic("closed")
    })
    child.detach()
    // Second child blocks on accept with no client — real observe I/O wait.
    listener :: net.tcp_listen("127.0.0.1:0") ?? panic("bind")
    io_child :: tasks.spawn(take(listener) () => {
        _ :: listener.accept() ?? panic("accept")
    })
    io_child.detach()
    ready.receive() ?? panic("closed")
    // hold_sender stays live so the blocked receive keeps real waiters.
    arena :: mem.Arena.new()
    x :: arena.alloc(42)
    probe_work()
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
    assert_honest_locks(&text);
    assert_honest_io(&text);
    assert_honest_native(&text, true);
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
    assert!(stdout.contains("locks count="), "{stdout}");
    assert!(stdout.contains("waiters="), "{stdout}");
    assert!(!stdout.contains("locks count=0 "), "zero locks in view: {stdout}");
    assert!(stdout.contains("io count="), "{stdout}");
    assert!(!stdout.contains("io count=0 "), "zero io in view: {stdout}");
    assert!(stdout.contains("native process_cpu="), "{stdout}");
    assert!(stdout.contains("target="), "{stdout}");
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
use core.net as net
use core.tasks as tasks
use core.time as time

fn probe_work() {
    // Present only so source_identity must parse a non-entry function spelling.
}

fn run() {
    // Channel setup before arena so spawn panic cannot capture the arena view.
    (ready_sender, ready) :: tasks.channel<Int>()
    (hold_sender, blocked) :: tasks.channel<Int>(1)
    child :: tasks.spawn(take(ready_sender, blocked) () => {
        ready_sender.send(1)
        blocked.receive() ?? panic("closed")
    })
    child.detach()
    // Second child blocks on accept with no client — real observe I/O wait.
    listener :: net.tcp_listen("127.0.0.1:0") ?? panic("bind")
    io_child :: tasks.spawn(take(listener) () => {
        _ :: listener.accept() ?? panic("accept")
    })
    io_child.detach()
    ready.receive() ?? panic("closed")
    // hold_sender stays live so the blocked receive keeps real waiters.
    arena :: mem.Arena.new()
    x :: arena.alloc(42)
    probe_work()
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
                    && snapshot.contains("\"parent\":1")
                    && snapshot.contains("\"recv_waiters\":1")
                    && (snapshot.contains("\"wait\":\"tcp accept\"")
                        || snapshot.contains("\"wait\":\"tcp accept readiness\"")) =>
            {
                break
            }
            _ if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            other => panic!(
                "observe snapshot never became readable with allocs+child+contention+io: {other:?}"
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
    assert_honest_locks(&text);
    assert_honest_io(&text);
    assert_honest_native(&text, false);
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
    assert!(stdout.contains("locks count="), "{stdout}");
    assert!(stdout.contains("waiters="), "{stdout}");
    assert!(!stdout.contains("locks count=0 "), "zero locks in view: {stdout}");
    assert!(stdout.contains("io count="), "{stdout}");
    assert!(!stdout.contains("io count=0 "), "zero io in view: {stdout}");
    assert!(stdout.contains("native process_cpu="), "{stdout}");
    assert!(stdout.contains("live.jet#run"), "{stdout}");

    let _ = fs::remove_dir_all(&root);
}
