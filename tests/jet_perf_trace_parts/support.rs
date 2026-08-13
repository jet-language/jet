use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use jet_foundation::JetTrace::{jettrace_artifact, trace_id, verify_jettrace};
use jet_foundation::PerformanceBudget::CanonicalJson;

static SELF_ATTACH_LOCK: Mutex<()> = Mutex::new(());

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

fn unused_local_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn http_request(port: u16, method: &str, path: &str, body: &str) -> Option<(u16, String)> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .ok()?;
    let mut raw = String::new();
    stream.read_to_string(&mut raw).ok()?;
    let (headers, body) = raw.split_once("\r\n\r\n")?;
    let status = headers.lines().next()?.split_whitespace().nth(1)?.parse().ok()?;
    Some((status, body.to_string()))
}

fn wait_http(port: u16, path: &str, accept: impl Fn(&str) -> bool) -> String {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some((200, body)) = http_request(port, "GET", path, "") {
            if accept(&body) {
                return body;
            }
        }
        assert!(
            Instant::now() < deadline,
            "GET {path} did not reach expected state"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn perf_nonce(html: &str) -> Option<&str> {
    let rest = html.split_once("jetPerfNonce = \"")?.1;
    rest.split_once('"').map(|(nonce, _)| nonce)
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let pid = self.0.id();
        let _ = self.0.kill();
        let _ = self.0.wait();
        let _ = fs::remove_file(jet::DevServer::LiveInspect::snapshot_path(pid));
        let _ = fs::remove_file(jet::DevServer::BrowserTrace::relay_path(pid));
        let _ = fs::remove_file(jet::DevServer::BrowserTrace::request_path(pid));
    }
}

fn json_u64_after(haystack: &str, key: &str) -> Option<u64> {
    let needle = format!("\"{key}\":");
    let tail = haystack.split_once(&needle)?.1;
    let digits: String = tail.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    digits.parse().ok()
}

fn json_string_after<'a>(haystack: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\":\"");
    haystack.split_once(&needle)?.1.split_once('"').map(|(value, _)| value)
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

fn assert_honest_native(text: &str, expect_elapsed: bool, expect_cpu_work: bool) {
    let native_at = text
        .find("\"native\":[{")
        .unwrap_or_else(|| panic!("missing native timing: {text}"));
    let native = &text[native_at..];
    assert!(native.contains("\"clock\":\"process_cpu\""), "{text}");
    let observed = json_u64_after(native, "observed_at_ns")
        .unwrap_or_else(|| panic!("missing native session-relative clock: {text}"));
    if expect_elapsed {
        assert!(observed > 0, "run native observation did not advance: {text}");
        let wall_at = text.find("\"domain\":\"wall\"").unwrap();
        let wall = json_u64_after(&text[wall_at..], "duration_ns").unwrap();
        assert!(observed <= wall, "native observation is after trace wall: {text}");
    }
    let target = json_string_after(native, "target")
        .unwrap_or_else(|| panic!("missing native target: {text}"));
    if cfg!(any(target_os = "linux", target_os = "android")) {
        assert!(native.contains("\"status\":\"captured\""), "{text}");
        let duration = json_u64_after(native, "duration_ns")
            .unwrap_or_else(|| panic!("missing captured native duration: {text}"));
        if expect_cpu_work {
            assert!(duration > 0, "deliberate CPU work captured as zero: {text}");
        }
        assert!(native.contains("\"reason\":\"\""), "{text}");
        assert!(native.contains("\"task_id\":1"), "{text}");
    } else {
        assert!(native.contains("\"status\":\"unavailable\""), "{text}");
        assert!(native.contains("\"duration_ns\":null"), "{text}");
        assert!(native.contains("\"task_id\":null"), "{text}");
        assert!(
            native.contains(&format!(
                "\"reason\":\"process CPU timing is unavailable on target {target}\""
            )),
            "{text}"
        );
    }
    assert!(text.contains("\"native_row_limit\":1"), "{text}");
    assert!(text.contains("\"native_rows_truncated\":false"), "{text}");
}

fn assert_honest_spans(text: &str, expect_captured: bool) {
    let spans = text
        .split_once("\"spans\":[")
        .and_then(|(_, tail)| tail.split_once("],\"tasks\""))
        .map(|(array, _)| array)
        .unwrap_or_else(|| panic!("missing spans array: {text}"));
    assert!(spans.contains("\"clock\":\"monotonic\""), "{text}");
    assert!(spans.contains("\"kind\":\"task_observed\""), "{text}");
    assert!(text.contains("\"span_row_limit\":4096"), "{text}");
    assert!(text.contains("\"span_rows_truncated\":false"), "{text}");
    if !expect_captured {
        assert!(spans.contains("\"status\":\"unavailable\""), "{text}");
        assert!(spans.contains("\"start_ns\":null"), "{text}");
        assert!(spans.contains("\"end_ns\":null"), "{text}");
        assert!(spans.contains("\"task_id\":null"), "{text}");
        assert!(spans.contains("\"parent_task_id\":null"), "{text}");
        assert!(
            spans.contains("task span requires multiple live observations"),
            "{text}"
        );
        return;
    }
    assert!(!spans.is_empty(), "empty captured span set: {text}");
    assert!(!spans.contains("\"status\":\"unavailable\""), "{text}");
    let tasks = text
        .split_once("\"tasks\":[")
        .and_then(|(_, tail)| tail.split_once("],\"toolchain\""))
        .map(|(array, _)| array)
        .unwrap_or_else(|| panic!("missing task array: {text}"));
    let wall_at = text.find("\"domain\":\"wall\"").unwrap();
    let wall = json_u64_after(&text[wall_at..], "duration_ns").unwrap();
    let mut count = 0usize;
    for span in spans.split("},{") {
        assert!(span.contains("\"status\":\"captured\""), "{span}");
        let task_id = json_u64_after(span, "task_id").unwrap();
        let task = tasks
            .split("},{")
            .find(|task| json_u64_after(task, "id") == Some(task_id))
            .unwrap_or_else(|| panic!("span task {task_id} missing: {text}"));
        let start = json_u64_after(span, "start_ns").unwrap();
        let end = json_u64_after(span, "end_ns").unwrap();
        assert!(end >= start, "span runs backward: {span}");
        assert!(end <= wall, "span ends after trace wall: {span}");
        let parent = json_u64_after(task, "parent").unwrap();
        if parent == 0 {
            assert!(span.contains("\"parent_task_id\":null"), "{span}");
        } else {
            assert_eq!(json_u64_after(span, "parent_task_id"), Some(parent), "{span}");
        }
        count += 1;
    }
    assert_eq!(count, tasks.split("},{").count(), "span/task count drift: {text}");
}
