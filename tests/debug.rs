//! D-DBG1 / D-DBG3 — `jet debug` source-level step-debugger tests.
//!
//! D-EXEC1/D-EXEC2: live stepping is native only (LLDB/DAP). The live tests
//! drive the production `jet debug <file>` session with scripted stdin and
//! assert on the protocol/output: that a breakpoint resolves to the right Jet
//! line, that frames and locals are in Jet terms (I2 — never generated Rust),
//! and that step / next / continue / finish behave as ratified. They gate only
//! on `rustc`/`lldb` being absent; once both tools exist, a stop or cleanup
//! failure is a test failure.
//!
//! Also covers codegen-only line-marker checks (no rustc/lldb needed), the
//! production DAP wire, the native recorder and its `--replay` browser
//! (D-DX-DEBUGBACK1), and the Canvas debug panel's recorded native tier.

mod common;
#[path = "tir_support/mod.rs"]
mod tir_support;

use jet_foundation::Dyn::Dyn;
use jet_foundation::JSON::{json_get, json_str, parse_json};
use std::ffi::OsStr;
use std::fmt;
use std::io::{Read, Write};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// An owned fixture keeps its package boundary and scratch tree alive for the
/// whole test, then removes both deterministically on drop.
struct Fixture {
    path: String,
    _scratch: common::Scratch,
}

impl Deref for Fixture {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.path
    }
}

impl AsRef<Path> for Fixture {
    fn as_ref(&self) -> &Path {
        Path::new(&self.path)
    }
}

impl AsRef<OsStr> for Fixture {
    fn as_ref(&self) -> &OsStr {
        OsStr::new(&self.path)
    }
}

impl fmt::Display for Fixture {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.path.fmt(formatter)
    }
}

/// Write `src` to an owned `.jet` project and return its path.
fn fixture(tag: &str, src: &str) -> Fixture {
    let scratch = common::Scratch::new(&format!("jet_debug_{tag}"));
    let dir = &scratch.path;
    tir_support::write_test_package(dir, tir_support::TIR_TEST_PACKAGE);
    let p = dir.join(format!("jet_debug_{tag}.jet"));
    let mut f = std::fs::File::create(&p).unwrap();
    f.write_all(src.as_bytes()).unwrap();
    Fixture {
        path: p.to_string_lossy().into_owned(),
        _scratch: scratch,
    }
}

fn canvas_source_id(path: &Path) -> &str {
    path.file_name()
        .and_then(|name| name.to_str())
        .expect("Canvas fixture must have a UTF-8 file name")
}

const LOOPS: &str = "\
fn run() {
    n := 3
    total := 0
    loop i in 1..n {
        total += i
    }
    print(\"total is {total}\")
}
";

const CALLS: &str = "\
fn double(x: Int) -> Int {
    y := x * 2
    return y
}
fn run() {
    a := 5
    b := double(a)
    print(\"{b}\")
}
";

#[test]
fn canvas_debug_session_replays_one_source_bound_session() {
    let file = fixture("canvas_live_protocol", LOOPS);
    let path = Path::new(&file);
    let source = std::fs::read_to_string(path).unwrap();
    let revision = jet::Canvas::source_revision(&source);
    let source_id = canvas_source_id(path);
    let sessions = jet::Canvas::DebugSessions::default();
    let first_request = format!(
        "{{\"schema_version\":1,\"source_id\":\"{}\",\"revision\":\"{}\",\"commands\":[\"s\"]}}",
        source_id, revision
    );
    let first =
        jet::Canvas::debug_session_json_for_file_with_sessions(path, &first_request, &sessions)
            .expect("Canvas should expose the first live stop");
    assert!(first.contains("\"state\":\"running\""), "{first}");
    assert!(
        first.contains("\"tier\":\"native-recorded\""),
        "{first}"
    );
    assert!(
        first.contains(&format!("\"revision\":\"{}\"", revision)),
        "{first}"
    );
    assert!(
        first.contains("run()"),
        "Canvas must expose the Jet entry function, not its host wrapper: {first}"
    );

    let marker = "\"id\":\"canvas-debug-";
    let start = first.find(marker).expect("live response session id") + 6;
    let end = first[start..]
        .find('"')
        .map(|offset| start + offset)
        .expect("live response session id terminator");
    let session_id = &first[start..end];
    let next_request = format!(
        "{{\"schema_version\":1,\"source_id\":\"{}\",\"revision\":\"{}\",\"session_id\":\"{}\",\"commands\":[\"s\"]}}",
        source_id, revision, session_id
    );
    let next =
        jet::Canvas::debug_session_json_for_file_with_sessions(path, &next_request, &sessions)
            .expect("Canvas should replay the same live session");
    assert!(next.contains("\"state\":\"running\""), "{next}");
    assert!(
        next.contains(&format!("\"id\":\"{}\"", session_id)),
        "{next}"
    );
    assert!(next.contains("\"debug_overlay\":\"running\""), "{next}");

    let invalid_continuation = format!(
        "{{\"schema_version\":1,\"source_id\":\"{}\",\"revision\":\"{}\",\"session_id\":\"{}\",\"commands\":[\"teleport\"]}}",
        source_id, revision, session_id
    );
    let invalid = jet::Canvas::debug_session_json_for_file_with_sessions(
        path,
        &invalid_continuation,
        &sessions,
    )
    .expect_err("Canvas must reject an unsupported continuation command");
    assert!(invalid.contains("\"kind\":\"unsupported\""), "{invalid}");
    let after_invalid =
        jet::Canvas::debug_session_json_for_file_with_sessions(path, &next_request, &sessions)
            .expect("a rejected command must leave the live session usable");
    assert!(
        after_invalid.contains(&format!("\"id\":\"{}\"", session_id)),
        "{after_invalid}"
    );
    assert!(
        after_invalid.contains("\"state\":\"running\""),
        "{after_invalid}"
    );

    let stop_request = format!(
        "{{\"schema_version\":1,\"source_id\":\"{}\",\"revision\":\"{}\",\"session_id\":\"{}\",\"stop\":true}}",
        source_id, revision, session_id
    );
    let stopped =
        jet::Canvas::debug_session_json_for_file_with_sessions(path, &stop_request, &sessions)
            .expect("Canvas should stop the live session without editing source");
    assert!(stopped.contains("\"state\":\"stopped\""), "{stopped}");
    assert!(stopped.contains("\"overlay\":null"), "{stopped}");
    assert_eq!(std::fs::read_to_string(path).unwrap(), source);

    let invalid_request = format!(
        "{{\"schema_version\":1,\"source_id\":\"{}\",\"revision\":\"{}\",\"commands\":[\"teleport\"]}}",
        source_id, revision
    );
    let invalid =
        jet::Canvas::debug_session_json_for_file_with_sessions(path, &invalid_request, &sessions)
            .expect_err("Canvas must reject commands outside the debugger vocabulary");
    assert!(invalid.contains("\"kind\":\"unsupported\""), "{invalid}");
    assert_eq!(std::fs::read_to_string(path).unwrap(), source);
}

#[test]
fn canvas_debug_stop_requires_matching_source_revision_and_tier() {
    let file = fixture("canvas_stop_binding", LOOPS);
    let path = Path::new(&file);
    let source = std::fs::read_to_string(path).unwrap();
    let revision = jet::Canvas::source_revision(&source);
    let source_id = canvas_source_id(path);
    let sessions = jet::Canvas::DebugSessions::default();
    let first_request = format!(
        "{{\"schema_version\":1,\"source_id\":\"{}\",\"revision\":\"{}\",\"tier\":\"native-recorded\",\"commands\":[\"s\"]}}",
        source_id, revision
    );
    let first =
        jet::Canvas::debug_session_json_for_file_with_sessions(path, &first_request, &sessions)
            .expect("Canvas should start a stoppable session");
    let marker = "\"id\":\"canvas-debug-";
    let start = first.find(marker).expect("live response session id") + 6;
    let end = first[start..]
        .find('"')
        .map(|offset| start + offset)
        .expect("live response session id terminator");
    let session_id = &first[start..end];

    let wrong_tier = format!(
        "{{\"schema_version\":1,\"source_id\":\"{}\",\"revision\":\"{}\",\"tier\":\"native-lldb\",\"session_id\":\"{}\",\"stop\":true}}",
        source_id, revision, session_id
    );
    let error =
        jet::Canvas::debug_session_json_for_file_with_sessions(path, &wrong_tier, &sessions)
            .expect_err("Canvas must not stop a session through another tier");
    assert!(error.contains("\"kind\":\"conflict\""), "{error}");

    let other_file = fixture("canvas_stop_other_source", LOOPS);
    let other_revision =
        jet::Canvas::source_revision(&std::fs::read_to_string(&other_file).unwrap());
    let other_source_id = canvas_source_id(Path::new(&other_file));
    let wrong_source = format!(
        "{{\"schema_version\":1,\"source_id\":\"{}\",\"revision\":\"{}\",\"tier\":\"native-recorded\",\"session_id\":\"{}\",\"stop\":true}}",
        other_source_id, other_revision, session_id
    );
    let error = jet::Canvas::debug_session_json_for_file_with_sessions(
        Path::new(&other_file),
        &wrong_source,
        &sessions,
    )
    .expect_err("Canvas must not stop a session through another source");
    assert!(error.contains("\"kind\":\"conflict\""), "{error}");

    let next_request = format!(
        "{{\"schema_version\":1,\"source_id\":\"{}\",\"revision\":\"{}\",\"tier\":\"native-recorded\",\"session_id\":\"{}\",\"commands\":[\"s\"]}}",
        source_id, revision, session_id
    );
    jet::Canvas::debug_session_json_for_file_with_sessions(path, &next_request, &sessions)
        .expect("a mismatched stop must leave the live session intact");

    let stale_stop = format!(
        "{{\"schema_version\":1,\"source_id\":\"{}\",\"revision\":\"sha256-stale\",\"tier\":\"native-recorded\",\"session_id\":\"{}\",\"stop\":true}}",
        source_id, session_id
    );
    let error =
        jet::Canvas::debug_session_json_for_file_with_sessions(path, &stale_stop, &sessions)
            .expect_err("Canvas must reject a stale stop request");
    assert!(error.contains("\"kind\":\"conflict\""), "{error}");

    let ended = format!(
        "{{\"schema_version\":1,\"source_id\":\"{}\",\"revision\":\"{}\",\"tier\":\"native-recorded\",\"session_id\":\"{}\",\"stop\":true}}",
        source_id, revision, session_id
    );
    let stopped = jet::Canvas::debug_session_json_for_file_with_sessions(path, &ended, &sessions)
        .expect("a stale stop request must leave the current session intact");
    assert!(stopped.contains("\"state\":\"stopped\""), "{stopped}");
    assert_eq!(std::fs::read_to_string(path).unwrap(), source);
}

#[test]
fn canvas_debug_rejects_unbounded_breakpoint_spans_before_execution() {
    let file = fixture("canvas_breakpoint_limit", LOOPS);
    let path = Path::new(&file);
    let source = std::fs::read_to_string(path).unwrap();
    let revision = jet::Canvas::source_revision(&source);
    let source_id = canvas_source_id(path);
    let anchors = (0..129).map(|_| "\"0:1\"").collect::<Vec<_>>().join(",");
    let request = format!(
        "{{\"schema_version\":1,\"source_id\":\"{}\",\"revision\":\"{}\",\"breakpoint_spans\":[{}],\"commands\":[\"s\"]}}",
        source_id, revision, anchors
    );
    let error = jet::Canvas::debug_session_json_for_file(path, &request)
        .expect_err("Canvas must bound breakpoint span input");
    assert!(error.contains("\"kind\":\"limit\""), "{error}");
    assert_eq!(std::fs::read_to_string(path).unwrap(), source);
}

#[test]
fn canvas_debug_live_native_tier_keeps_its_tier() {
    let file = fixture("canvas_native_protocol", LOOPS);
    let path = Path::new(&file);
    let source = std::fs::read_to_string(path).unwrap();
    let revision = jet::Canvas::source_revision(&source);
    let source_id = canvas_source_id(path);
    let sessions = jet::Canvas::DebugSessions::default();
    let request = format!(
        "{{\"schema_version\":1,\"source_id\":\"{}\",\"revision\":\"{}\",\"tier\":\"native-lldb\",\"commands\":[\"s\"]}}",
        source_id, revision
    );
    match jet::Canvas::debug_session_json_for_file_with_sessions(path, &request, &sessions) {
        Ok(body) => {
            assert!(body.contains("\"tier\":\"native-lldb\""), "{body}");
            assert!(!body.contains("\"tier\":\"native-recorded\""), "{body}");
            assert!(body.contains("\"state\":\"running\""), "{body}");
            let marker = "\"id\":\"canvas-debug-";
            let start = body.find(marker).expect("native session id") + 6;
            let end = body[start..]
                .find('"')
                .map(|offset| start + offset)
                .expect("native session id terminator");
            let session_id = &body[start..end];
            let stop = format!(
                "{{\"schema_version\":1,\"source_id\":\"{}\",\"revision\":\"{}\",\"tier\":\"native-lldb\",\"session_id\":\"{}\",\"stop\":true}}",
                source_id, revision, session_id
            );
            let stopped =
                jet::Canvas::debug_session_json_for_file_with_sessions(path, &stop, &sessions)
                    .expect("native session stop");
            assert!(stopped.contains("\"state\":\"stopped\""), "{stopped}");
        }
        Err(error) => {
            assert!(error.contains("\"kind\":\"diagnostic\""), "{error}");
            assert!(!error.contains("rustc rejected"), "{error}");
        }
    }
    assert_eq!(std::fs::read_to_string(path).unwrap(), source);
}

#[test]
fn canvas_graph_source_id_stays_project_relative_for_relative_entries() {
    let graph = jet::Canvas::graph_json_for_file(Path::new("Examples/features/basics/hello.jet"))
        .expect("relative Canvas entry should project");
    assert!(
        graph.contains("\"source_id\":\"hello.jet\""),
        "Canvas/debug source identity must match project file ids: {graph}"
    );
}

/// Drive the production `jet debug <file>` native session (D-EXEC1: LLDB is
/// the only live stepping backend) with scripted stdin. Returns the exit code
/// and stdout+stderr, or `None` when rustc/lldb are absent.
fn native_cli_session(tag: &str, src: &str, commands: &str) -> Option<(Option<i32>, String)> {
    if !have("rustc") || !have("lldb") {
        eprintln!("skipping: native jet debug needs rustc and lldb on PATH");
        return None;
    }
    let file = native_fixture(tag, src);
    let dir = Path::new(&*file).parent().unwrap().to_path_buf();
    let mut session = Command::new(env!("CARGO_BIN_EXE_jet"))
        .current_dir(&dir)
        .args(["debug", &*file])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start jet debug");
    session
        .stdin
        .take()
        .unwrap()
        .write_all(commands.as_bytes())
        .unwrap();
    let output = session.wait_with_output().unwrap();
    Some((
        output.status.code(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
    ))
}

/// The first stop names the entry function and line with a `<- here` caret;
/// locals and `print` are Jet names and values (I2), and `continue` runs the
/// program to completion.
#[test]
fn native_cli_steps_shows_locals_and_finishes() {
    let Some((code, out)) = native_cli_session("cli_step", LOOPS, "s\ns\nlocals\np total\nc\n") else {
        return;
    };
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains("breakpoint hit") && out.contains(":2  in run()"), "{out}");
    assert!(out.contains("<- here"), "{out}");
    assert!(out.contains("total = 0"), "{out}");
    assert!(out.contains("total is 6"), "{out}");
    assert!(out.contains("program finished"), "{out}");
    assert!(!out.contains("__jet_"), "generated names leaked:\n{out}");
}

#[test]
fn native_cli_break_then_continue_stops_at_breakpoint() {
    let Some((code, out)) = native_cli_session("cli_break", LOOPS, "break 7\nc\nc\n") else {
        return;
    };
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains("breakpoint set"), "{out}");
    assert!(
        out.contains("print(\"total is {total}\")        <- here"),
        "continue should stop on the line-7 breakpoint:\n{out}"
    );
}

#[test]
fn native_cli_steps_into_a_call_and_backtraces() {
    let Some((code, out)) = native_cli_session("cli_calls", CALLS, "s\ns\nbt\nc\n") else {
        return;
    };
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains("y := x * 2"), "step should descend into the callee:\n{out}");
    assert!(
        out.contains("double()") && out.contains("run()"),
        "backtrace should show both Jet frames:\n{out}"
    );
}

#[test]
fn native_cli_next_steps_over_a_call() {
    let Some((code, out)) = native_cli_session("cli_next", CALLS, "n\nn\nc\n") else {
        return;
    };
    assert_eq!(code, Some(0), "{out}");
    assert!(
        !out.contains("y := x * 2        <- here"),
        "`next` must not stop inside the callee:\n{out}"
    );
    assert!(out.contains("print(\"{b}\")        <- here"), "{out}");
}

#[test]
fn native_cli_finish_returns_to_caller() {
    let Some((code, out)) = native_cli_session("cli_finish", CALLS, "s\ns\nfinish\nc\n") else {
        return;
    };
    assert_eq!(code, Some(0), "{out}");
    assert!(
        out.contains("b := double(a)") || out.contains("print(\"{b}\")"),
        "finish should return control to the caller frame:\n{out}"
    );
    assert!(out.contains("10"), "{out}");
}

#[test]
fn native_cli_quit_ends_session_with_e2204() {
    let Some((code, out)) = native_cli_session("cli_quit", LOOPS, "q\n") else {
        return;
    };
    assert_ne!(code, Some(0), "{out}");
    assert!(out.contains("E2204"), "{out}");
    assert!(!out.contains("total is 6"), "quit must not run the program on:\n{out}");
}

#[test]
fn native_cli_help_lists_the_verbs() {
    let Some((_, out)) = native_cli_session("cli_help", LOOPS, "help\nc\n") else {
        return;
    };
    for verb in ["step", "next", "continue", "finish", "break", "print", "backtrace", "quit"] {
        assert!(out.contains(verb), "help must list `{verb}`:\n{out}");
    }
}

/// `next` and `next(outer)` statements carry line markers so the native
/// debugger can stop on them.
#[test]
fn loop_next_edges_are_line_mapped() {
    let src = "\
fn run() {
    hits := 0
    loop i in 0..<3 {
        if i == 1 {
            next
        }
        hits += i
    }
    outer_hits := 0
    inner_hits := 0
    outer :: loop i in 0..<3 {
        loop j in 0..<3 {
            if j == 1 {
                next(outer)
            }
            inner_hits += 1
        }
        outer_hits += 1
    }
    print(\"{hits},{inner_hits},{outer_hits}\")
}
";
    let file = fixture("loop_next_edges", src);
    let generated = jet::compile_for_debug(&file).expect("loop next fixture compiles for debug");
    for line in [5, 14] {
        assert!(
            generated.rust.contains(&format!("// jet:line {line}\n")),
            "missing line marker for loop next on Jet line {line}:\n{}",
            generated.rust
        );
    }
}

// ============================================================================
// Section: native lldb-backed backend (was tests/debug_native.rs)
// ============================================================================

fn have(tool: &str) -> bool {
    std::process::Command::new(tool)
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn native_fixture(tag: &str, src: &str) -> Fixture {
    let scratch = common::Scratch::new(&format!("jet_debug_native_{tag}"));
    let dir = &scratch.path;
    tir_support::write_test_package(dir, tir_support::TIR_TEST_PACKAGE);
    let p = dir.join(format!("jet_debug_native_{tag}.jet"));
    std::fs::write(&p, src).unwrap();
    Fixture {
        path: p.to_string_lossy().into_owned(),
        _scratch: scratch,
    }
}

fn native_binary(tag: &str, rust: &str) -> (common::Scratch, PathBuf) {
    let scratch = common::Scratch::new(&format!("jet_debug_native_map_{tag}"));
    let dir = &scratch.path;
    let rust_file = dir.join("prog.rs");
    let binary = dir.join("prog");
    std::fs::write(&rust_file, rust).unwrap();
    let rustc = Command::new("rustc")
        .args([
            "--edition",
            "2021",
            "-C",
            "debuginfo=2",
            rust_file.to_str().unwrap(),
            "-o",
            binary.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        rustc.status.success(),
        "rustc rejected the {tag} debug fixture:\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    (scratch, binary)
}

/// D-DX-DEBUGBACK1=A: `jet debug --record` records one complete native LLDB
/// run, and `--replay` browses that history without executing the program.
#[test]
fn native_record_then_replay_browses_without_executing() {
    if !have("rustc") || !have("lldb") {
        eprintln!("skipping: native record needs rustc and lldb on PATH");
        return;
    }
    let file = native_fixture("record_replay", LOOPS);
    let dir = Path::new(&*file).parent().unwrap().to_path_buf();
    let recorded = Command::new(env!("CARGO_BIN_EXE_jet"))
        .current_dir(&dir)
        .args(["debug", &*file, "--record=loops"])
        .stdin(Stdio::null())
        .output()
        .expect("run jet debug --record");
    assert_eq!(
        recorded.status.code(),
        Some(0),
        "native record failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&recorded.stdout),
        String::from_utf8_lossy(&recorded.stderr)
    );
    assert!(
        dir.join(".jet/replays/loops.jetproof-replay").is_file(),
        "native record wrote no replay receipt"
    );
    let mut replay = Command::new(env!("CARGO_BIN_EXE_jet"))
        .current_dir(&dir)
        .args(["debug", &*file, "--replay=loops"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start jet debug --replay");
    replay
        .stdin
        .take()
        .unwrap()
        .write_all(b"when total\nc\n")
        .unwrap();
    let output = replay.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        output.status.code(),
        Some(0),
        "replay failed:\n{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("last change:"), "{stdout}");
    assert!(
        !stdout.contains("total is"),
        "replay re-executed the recorded program: {stdout}"
    );
}

fn dap_frame(body: &str) -> String {
    format!("Content-Length: {}\r\n\r\n{}", body.as_bytes().len(), body)
}

fn dap_response(output: &str, request_seq: u32, command: &str) -> Option<String> {
    let bytes = output.as_bytes();
    let mut cursor = 0;
    while let Some(relative_start) = output[cursor..].find("Content-Length: ") {
        let start = cursor + relative_start;
        let header_end = output[start..].find("\r\n\r\n")?;
        let header = &output[start..start + header_end];
        let length = header
            .strip_prefix("Content-Length: ")?
            .parse::<usize>()
            .ok()?;
        let body_start = start + header_end + 4;
        let body_end = body_start.checked_add(length)?;
        let body = std::str::from_utf8(bytes.get(body_start..body_end)?).ok()?;
        let Ok(value) = parse_json(body) else {
            cursor = body_end;
            continue;
        };
        let type_name = json_get(&value, "type").and_then(json_str);
        let actual_request_seq = match json_get(&value, "request_seq") {
            Some(Dyn::Int(value)) => value.to_i64(),
            _ => None,
        };
        let success = json_get(&value, "success");
        let actual_command = json_get(&value, "command").and_then(json_str);
        if type_name == Some("response")
            && actual_request_seq == Some(i64::from(request_seq))
            && matches!(success, Some(Dyn::Bool(_)))
            && actual_command == Some(command)
        {
            return Some(body.to_string());
        }
        cursor = body_end;
    }
    None
}

fn write_dap_request(input: &mut impl Write, body: &str) {
    input.write_all(body.as_bytes()).expect("send DAP request");
    input.flush().expect("flush DAP request");
}

fn read_dap_until(
    output_rx: &std::sync::mpsc::Receiver<Vec<u8>>,
    stdout: &mut String,
    ready: impl Fn(&str) -> bool,
    what: &str,
) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if ready(stdout) {
            return;
        }
        match output_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(chunk) => stdout.push_str(&String::from_utf8_lossy(&chunk)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    panic!("timed out waiting for {what}:\n{stdout}");
}

fn read_dap_response(
    output_rx: &std::sync::mpsc::Receiver<Vec<u8>>,
    stdout: &mut String,
    request_seq: u32,
    command: &str,
) -> String {
    read_dap_until(
        output_rx,
        stdout,
        |output| dap_response(output, request_seq, command).is_some(),
        &format!("DAP {command} response {request_seq}"),
    );
    dap_response(stdout, request_seq, command).expect("DAP response after wait")
}

fn dap_u32_after(body: &str, marker: &str) -> u32 {
    let start = body
        .find(marker)
        .unwrap_or_else(|| panic!("missing `{marker}` in DAP body: {body}"));
    body[start + marker.len()..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or_else(|_| panic!("missing integer after `{marker}` in DAP body: {body}"))
}

fn dap_u32_before(body: &str, marker: &str) -> u32 {
    let end = body
        .find(marker)
        .unwrap_or_else(|| panic!("missing `{marker}` in DAP body: {body}"));
    let prefix = &body[..end];
    let start = prefix
        .rfind("\"id\":")
        .unwrap_or_else(|| panic!("missing frame id before `{marker}` in DAP body: {body}"));
    prefix[start + 5..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or_else(|_| panic!("missing frame id before `{marker}` in DAP body: {body}"))
}

#[cfg(target_os = "linux")]
fn linux_process_state(pid: u32) -> Option<String> {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|stat| stat.split_whitespace().nth(2).map(str::to_owned))
}

#[cfg(target_os = "linux")]
fn spawn_attachable_native_target(binary: &Path, tag: &str) -> std::process::Child {
    let root = common::unique_tmp(&format!("jet_debug_attach_authorizer_{tag}"));
    std::fs::create_dir_all(&root).unwrap();
    let source = root.join("authorizer.rs");
    let helper = root.join("authorizer");
    std::fs::write(
        &source,
        r#"
use std::os::unix::process::CommandExt;
use std::process::Command;

unsafe extern "C" {
    fn prctl(option: i32, arg2: usize, arg3: usize, arg4: usize, arg5: usize) -> i32;
}

fn main() {
    let target = std::env::args_os().nth(1).expect("target binary");
    let result = unsafe { prctl(0x5961_6d61, usize::MAX, 0, 0, 0) };
    if result != 0 {
        std::process::exit(125);
    }
    let error = Command::new(target).exec();
    eprintln!("{error}");
    std::process::exit(126);
}
"#,
    )
    .expect("write attach authorizer");
    let compile = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&source)
        .args(["-o"])
        .arg(&helper)
        .output()
        .expect("compile attach authorizer");
    assert!(
        compile.status.success(),
        "rustc rejected attach authorizer:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let target = Command::new(&helper)
        .arg(binary)
        .spawn()
        .expect("start attachable native debug target");
    let _ = std::fs::remove_file(source);
    let _ = std::fs::remove_file(helper);
    target
}

#[cfg(target_os = "linux")]
fn wait_for_target_binary(pid: u32, binary: &Path) {
    let expected = std::fs::canonicalize(binary).expect("canonicalize attach target");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if std::fs::canonicalize(format!("/proc/{pid}/exe")).ok().as_deref()
            == Some(expected.as_path())
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "attach target did not exec the matching native debug binary"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn dap_cli_exercises_the_production_wire_and_attach_failure() {
    let tag = format!("dap_cli_failure_{}", std::process::id());
    let file = native_fixture(&tag, LOOPS);
    let input = format!(
        "{}{}{}",
        dap_frame(
            r#"{"seq":1,"type":"request","command":"initialize","arguments":{"adapterID":"jet","pathFormat":"path","linesStartAt1":true,"columnsStartAt1":true}}"#
        ),
        dap_frame(&format!(
            r#"{{"seq":2,"type":"request","command":"setBreakpoints","arguments":{{"source":{{"path":"{}"}},"breakpoints":[{{"line":7}}]}}}}"#,
            file
        )),
        dap_frame(r#"{"seq":3,"type":"request","command":"attach","arguments":{"processId":0}}"#),
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["debug", "--dap", &file])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start the production Jet DAP command");
    child
        .stdin
        .take()
        .expect("DAP stdin")
        .write_all(input.as_bytes())
        .expect("send framed DAP requests");
    let output = child
        .wait_with_output()
        .expect("wait for the production Jet DAP command");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "DAP command failed:\n{stderr}");
    let initialize = dap_response(&stdout, 1, "initialize").expect("initialize response");
    assert!(initialize.contains("\"success\":true"), "{initialize}");
    let breakpoints = dap_response(&stdout, 2, "setBreakpoints").expect("breakpoint response");
    assert!(breakpoints.contains("\"verified\":true"), "{breakpoints}");
    assert!(
        breakpoints.contains("\"id\":1,\"verified\":true,\"line\":7"),
        "{breakpoints}"
    );
    let attach = dap_response(&stdout, 3, "attach").expect("attach response");
    assert!(attach.contains("\"success\":false"), "{attach}");
    assert!(attach.contains("\"id\":22032"), "{attach}");
    assert!(attach.contains("\"code\":\"E2236\""), "{attach}");
    assert!(
        !stdout.contains("\"event\":\"stopped\""),
        "invalid attach must not start a target: {stdout}"
    );
    let _ = std::fs::remove_file(&file);
}

#[cfg(target_os = "linux")]
#[test]
fn dap_cli_rejects_a_cross_executable_attach_through_the_production_wire() {
    if !have("rustc") {
        return;
    }
    let tag = format!(
        "dap_cli_attach_denied_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos()
    );
    let file = native_fixture(&tag, "fn run() {\n    loop {}\n}\n");
    let stem = Path::new(&file)
        .file_stem()
        .expect("attach fixture has a file stem")
        .to_string_lossy();
    let binary = Path::new("build").join(format!("{stem}_dbg"));
    let map = binary.with_extension("jetmap");
    let mut adapter = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["debug", "--dap", &file])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start the production Jet DAP command");
    let mut input = adapter.stdin.take().expect("DAP stdin");
    input
        .write_all(
            dap_frame(
                r#"{"seq":1,"type":"request","command":"initialize","arguments":{"adapterID":"jet","pathFormat":"path"}}"#,
            )
            .as_bytes(),
        )
        .expect("send initialize request");
    input.flush().expect("flush initialize request");

    let deadline = Instant::now() + Duration::from_secs(30);
    while !(binary.is_file() && map.is_file()) {
        if adapter
            .try_wait()
            .expect("check production DAP process")
            .is_some()
        {
            panic!("production DAP process exited before writing its debug map");
        }
        assert!(
            Instant::now() < deadline,
            "production DAP process did not publish its debug map"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    let mut target = Command::new("sleep")
        .arg("30")
        .spawn()
        .expect("start mismatched native attach target");
    let attach = dap_frame(&format!(
        r#"{{"seq":2,"type":"request","command":"attach","arguments":{{"processId":{},"program":"{}","map":"{}"}}}}"#,
        target.id(),
        binary.display(),
        map.display()
    ));
    input
        .write_all(attach.as_bytes())
        .expect("send attach request");
    input.flush().expect("flush attach request");
    drop(input);
    let output = adapter
        .wait_with_output()
        .expect("wait for the production Jet DAP command");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let target_alive = target.try_wait().expect("check denied target").is_none();
    let _ = target.kill();
    let _ = target.wait();
    let _ = std::fs::remove_file(&map);
    let _ = std::fs::remove_file(&binary);
    let _ = std::fs::remove_file(Path::new("build").join(format!("{stem}.rs")));
    let _ = std::fs::remove_file(&file);

    assert!(output.status.success(), "DAP command failed:\n{stderr}");
    assert!(stdout.contains("\"command\":\"attach\""), "{stdout}");
    assert!(
        stdout.contains("\"request_seq\":2,\"success\":false,\"command\":\"attach\""),
        "{stdout}"
    );
    assert!(stdout.contains("\"id\":22037"), "{stdout}");
    assert!(stdout.contains("\"code\":\"E2231\""), "{stdout}");
    assert!(!stdout.contains("\"event\":\"stopped\""), "{stdout}");
    assert!(target_alive, "denied attach must leave the target alive");
}

#[cfg(target_os = "linux")]
#[test]
fn dap_cli_attaches_and_disconnects_through_the_production_wire() {
    if !have("rustc") || !have("lldb") {
        return;
    }
    let tag = format!(
        "dap_cli_attach_success_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos()
    );
    let file = native_fixture(&tag, "fn run() {\n    loop {}\n}\n");
    let stem = Path::new(&file)
        .file_stem()
        .expect("attach fixture has a file stem")
        .to_string_lossy();
    let binary = Path::new("build").join(format!("{stem}_dbg"));
    let map = binary.with_extension("jetmap");
    let mut adapter = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["debug", "--dap", &file])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start the production Jet DAP command");
    let mut input = adapter.stdin.take().expect("DAP stdin");
    input
        .write_all(
            dap_frame(
                r#"{"seq":1,"type":"request","command":"initialize","arguments":{"adapterID":"jet","pathFormat":"path"}}"#,
            )
            .as_bytes(),
        )
        .expect("send initialize request");
    input.flush().expect("flush initialize request");

    let deadline = Instant::now() + Duration::from_secs(30);
    while !(binary.is_file() && map.is_file()) {
        if adapter
            .try_wait()
            .expect("check production DAP process")
            .is_some()
        {
            panic!("production DAP process exited before writing its debug map");
        }
        assert!(
            Instant::now() < deadline,
            "production DAP process did not publish its debug map"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    let mut target = spawn_attachable_native_target(&binary, &tag);
    wait_for_target_binary(target.id(), &binary);
    let attach = dap_frame(&format!(
        r#"{{"seq":2,"type":"request","command":"attach","arguments":{{"processId":{},"program":"{}","map":"{}"}}}}"#,
        target.id(),
        binary.display(),
        map.display()
    ));
    input
        .write_all(attach.as_bytes())
        .expect("send attach request");
    input.flush().expect("flush attach request");
    let attach_deadline = Instant::now() + Duration::from_secs(30);
    let mut target_stopped = false;
    while Instant::now() < attach_deadline {
        if adapter
            .try_wait()
            .expect("check production DAP process during attach")
            .is_some()
        {
            break;
        }
        if matches!(linux_process_state(target.id()).as_deref(), Some("T" | "t")) {
            target_stopped = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let disconnect =
        dap_frame(r#"{"seq":3,"type":"request","command":"disconnect","arguments":{}}"#);
    let _ = input.write_all(disconnect.as_bytes());
    let _ = input.flush();
    drop(input);
    let output = adapter
        .wait_with_output()
        .expect("wait for the production Jet DAP command");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let target_alive = target.try_wait().expect("check detached target").is_none();
    let target_running = if target_alive {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match linux_process_state(target.id()).as_deref() {
                Some("T" | "t") if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Some("T" | "t") => break false,
                Some(_) => break true,
                None => break false,
            }
        }
    } else {
        false
    };
    if target_alive {
        let _ = target.kill();
    }
    let _ = target.wait();
    let _ = std::fs::remove_file(&map);
    let _ = std::fs::remove_file(&binary);
    let _ = std::fs::remove_file(Path::new("build").join(format!("{stem}.rs")));
    let _ = std::fs::remove_file(&file);

    assert!(output.status.success(), "DAP command failed:\n{stderr}");
    assert!(stdout.contains("\"command\":\"attach\""), "{stdout}");
    assert!(
        stdout.contains("\"request_seq\":2,\"success\":true,\"command\":\"attach\""),
        "{stdout}"
    );
    assert!(stdout.contains("\"command\":\"disconnect\""), "{stdout}");
    assert!(
        stdout.contains("\"request_seq\":3,\"success\":true,\"command\":\"disconnect\""),
        "{stdout}"
    );
    assert!(!stdout.contains("\"success\":false"), "{stdout}");
    assert!(
        target_alive,
        "production disconnect must leave an attached target alive"
    );
    assert!(
        target_running,
        "production disconnect must resume the attached target"
    );
    assert!(
        target_stopped,
        "production attach must stop the real target before disconnect"
    );
}

#[test]
fn dap_cli_launches_and_projects_a_live_target_in_jet_terms() {
    if !have("rustc") || !have("lldb") {
        return;
    }
    let tag = format!("dap_cli_success_{}", std::process::id());
    let file = native_fixture(&tag, LOOPS);
    let input = format!(
        "{}{}{}{}{}{}{}{}{}",
        dap_frame(
            r#"{"seq":1,"type":"request","command":"initialize","arguments":{"adapterID":"jet","pathFormat":"path","linesStartAt1":true,"columnsStartAt1":true}}"#
        ),
        dap_frame(&format!(
            r#"{{"seq":2,"type":"request","command":"setBreakpoints","arguments":{{"source":{{"path":"{}"}},"breakpoints":[{{"line":7}}]}}}}"#,
            file
        )),
        dap_frame(&format!(
            r#"{{"seq":3,"type":"request","command":"launch","arguments":{{"program":"{}","stopOnEntry":true,"args":[],"env":{{}}}}}}"#,
            file
        )),
        dap_frame(r#"{"seq":4,"type":"request","command":"configurationDone","arguments":{}}"#),
        dap_frame(r#"{"seq":5,"type":"request","command":"threads","arguments":{}}"#),
        dap_frame(
            r#"{"seq":6,"type":"request","command":"stackTrace","arguments":{"threadId":1}}"#
        ),
        dap_frame(r#"{"seq":7,"type":"request","command":"continue","arguments":{"threadId":1}}"#),
        dap_frame(
            r#"{"seq":8,"type":"request","command":"stackTrace","arguments":{"threadId":1}}"#
        ),
        dap_frame(r#"{"seq":9,"type":"request","command":"continue","arguments":{"threadId":1}}"#),
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["debug", "--dap", &file])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start the production Jet DAP command");
    child
        .stdin
        .take()
        .expect("DAP stdin")
        .write_all(input.as_bytes())
        .expect("send framed DAP requests");
    let output = child
        .wait_with_output()
        .expect("wait for the production Jet DAP command");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "DAP command failed:\n{stderr}");
    let breakpoints = dap_response(&stdout, 2, "setBreakpoints").expect("breakpoint response");
    assert!(breakpoints.contains("\"verified\":true"), "{breakpoints}");
    assert!(
        breakpoints.contains("\"id\":1,\"verified\":true,\"line\":7"),
        "{breakpoints}"
    );
    assert!(
        stdout.contains("\"request_seq\":1,\"success\":true,\"command\":\"initialize\""),
        "{stdout}"
    );
    assert!(stdout.contains("\"event\":\"stopped\""), "{stdout}");
    assert!(stdout.contains("\"reason\":\"breakpoint\""), "{stdout}");
    assert!(stdout.contains("\"hitBreakpointIds\":[1]"), "{stdout}");
    assert!(stdout.contains("\"command\":\"threads\""), "{stdout}");
    assert!(stdout.contains("\"command\":\"stackTrace\""), "{stdout}");
    assert!(stdout.contains("\"name\":\"run\""), "{stdout}");
    assert!(stdout.contains("\"line\":2"), "{stdout}");
    let breakpoint_stack = dap_response(&stdout, 8, "stackTrace").expect("breakpoint stack trace");
    assert!(
        breakpoint_stack.contains("\"line\":7"),
        "{breakpoint_stack}"
    );
    assert!(
        stdout.contains(&format!("\"path\":\"{}\"", file)),
        "{stdout}"
    );
    assert!(stdout.contains("\"event\":\"output\""), "{stdout}");
    assert!(stdout.contains("total is "), "{stdout}");
    assert!(
        !stdout.contains("__jet_"),
        "generated Rust leaked into Jet view: {stdout}"
    );
    assert!(stdout.contains("\"event\":\"exited\""), "{stdout}");
    assert!(stdout.contains("\"event\":\"terminated\""), "{stdout}");
    let _ = std::fs::remove_file(&file);
}

#[test]
fn dap_cli_pauses_a_running_target_through_production_wire() {
    if !have("rustc") || !have("lldb") {
        return;
    }
    let tag = format!(
        "dap_cli_pause_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos()
    );
    let file = native_fixture(
        &tag,
        "fn run() {\n    total := 0\n    loop i in 1..1000000000 {\n        total += i\n    }\n    print(\"{total}\")\n}\n",
    );
    let input = format!(
        "{}{}{}{}",
        dap_frame(
            r#"{"seq":1,"type":"request","command":"initialize","arguments":{"adapterID":"jet","pathFormat":"path"}}"#
        ),
        dap_frame(&format!(
            r#"{{"seq":2,"type":"request","command":"launch","arguments":{{"program":"{}","stopOnEntry":false,"args":[],"env":{{}}}}}}"#,
            file
        )),
        dap_frame(r#"{"seq":3,"type":"request","command":"configurationDone","arguments":{}}"#),
        dap_frame(r#"{"seq":4,"type":"request","command":"pause","arguments":{}}"#),
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["debug", "--dap", &file])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the production Jet DAP command");
    let mut input_pipe = child.stdin.take().expect("DAP stdin");
    input_pipe
        .write_all(input.as_bytes())
        .expect("send initialize, launch, configuration, and pause");
    input_pipe.flush().expect("flush DAP requests");

    let mut stdout = child.stdout.take().expect("DAP stdout");
    let (output_tx, output_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let output_reader = std::thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        loop {
            match stdout.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(size) => {
                    if output_tx.send(chunk[..size].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut transcript = String::new();
    let mut paused = false;
    while Instant::now() < deadline && !paused {
        match output_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(chunk) => {
                transcript.push_str(&String::from_utf8_lossy(&chunk));
                paused = transcript
                    .contains("\"request_seq\":4,\"success\":true,\"command\":\"pause\"")
                    && transcript.contains("\"event\":\"stopped\"")
                    && transcript.contains("\"reason\":\"pause\"");
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let mut threads_ok = false;
    if paused {
        input_pipe
            .write_all(
                dap_frame(r#"{"seq":5,"type":"request","command":"threads","arguments":{}}"#)
                    .as_bytes(),
            )
            .expect("send threads after pause");
        input_pipe.flush().expect("flush threads request");
        let threads_deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < threads_deadline && !threads_ok {
            match output_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(chunk) => {
                    transcript.push_str(&String::from_utf8_lossy(&chunk));
                    threads_ok = transcript
                        .contains("\"request_seq\":5,\"success\":true,\"command\":\"threads\"");
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        input_pipe
            .write_all(
                dap_frame(r#"{"seq":6,"type":"request","command":"disconnect","arguments":{}}"#)
                    .as_bytes(),
            )
            .expect("send disconnect after pause");
        input_pipe.flush().expect("flush disconnect");
    }
    drop(input_pipe);

    let exit_deadline = Instant::now() + Duration::from_secs(10);
    let mut status = None;
    while Instant::now() < exit_deadline {
        if let Some(result) = child.try_wait().expect("check production DAP process") {
            status = Some(result);
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if status.is_none() {
        let _ = child.kill();
        status = child.wait().ok();
    }
    let _ = output_reader.join();
    while let Ok(chunk) = output_rx.try_recv() {
        transcript.push_str(&String::from_utf8_lossy(&chunk));
    }
    let _ = std::fs::remove_file(&file);
    assert!(paused, "production DAP pause did not settle:\n{transcript}");
    assert!(
        threads_ok,
        "production DAP stopped state was not usable:\n{transcript}"
    );
    assert!(
        status.is_some_and(|status| status.success()),
        "production DAP pause session failed:\n{transcript}"
    );
}

#[test]
fn dap_cli_projects_raw_frames_only_when_requested_through_production_wire() {
    if !have("rustc") || !have("lldb") {
        return;
    }
    let tag = format!(
        "dap_cli_raw_frames_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos()
    );
    let file = native_fixture(&tag, LOOPS);
    let stem = Path::new(&file)
        .file_stem()
        .expect("raw-frame fixture has a file stem")
        .to_string_lossy();
    let rust_file = format!("{stem}.rs");
    let setup = format!(
        "{}{}{}",
        dap_frame(
            r#"{"seq":1,"type":"request","command":"initialize","arguments":{"adapterID":"jet","pathFormat":"path"}}"#
        ),
        dap_frame(&format!(
            r#"{{"seq":2,"type":"request","command":"launch","arguments":{{"program":"{}","stopOnEntry":true,"args":[],"env":{{}}}}}}"#,
            file
        )),
        dap_frame(r#"{"seq":3,"type":"request","command":"configurationDone","arguments":{}}"#),
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["debug", "--dap", &file])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the production Jet DAP command");
    let mut input_pipe = child.stdin.take().expect("DAP stdin");
    write_dap_request(&mut input_pipe, &setup);

    let mut stdout_pipe = child.stdout.take().expect("DAP stdout");
    let (output_tx, output_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let output_reader = std::thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        loop {
            match stdout_pipe.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(size) => {
                    if output_tx.send(chunk[..size].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut stdout = String::new();
    let mut stopped = false;
    while Instant::now() < deadline && !stopped {
        match output_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(chunk) => {
                stdout.push_str(&String::from_utf8_lossy(&chunk));
                stopped = stdout.contains("\"event\":\"stopped\"");
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    assert!(stopped, "production DAP did not stop at entry:\n{stdout}");
    if stopped {
        write_dap_request(
            &mut input_pipe,
            &dap_frame(
                r#"{"seq":4,"type":"request","command":"stackTrace","arguments":{"threadId":1,"showRawFrames":"yes"}}"#,
            ),
        );
    }
    let rejected = read_dap_response(&output_rx, &mut stdout, 4, "stackTrace");
    assert!(rejected.contains("\"success\":false"), "{rejected}");
    assert!(rejected.contains("\"id\":22032"), "{rejected}");
    assert!(rejected.contains("\"code\":\"E2236\""), "{rejected}");

    write_dap_request(
        &mut input_pipe,
        &dap_frame(
            r#"{"seq":5,"type":"request","command":"stackTrace","arguments":{"threadId":1}}"#,
        ),
    );
    let safe = read_dap_response(&output_rx, &mut stdout, 5, "stackTrace");
    assert!(safe.contains("\"success\":true"), "{safe}");
    assert!(
        !safe.contains("__jet_"),
        "generated Rust leaked into Jet view: {safe}"
    );
    assert!(
        !safe.contains(&rust_file),
        "generated Rust source leaked into Jet view: {safe}"
    );
    let safe_frame = dap_u32_after(&safe, "\"stackFrames\":[{\"id\":");

    write_dap_request(
        &mut input_pipe,
        &dap_frame(&format!(
            r#"{{"seq":6,"type":"request","command":"scopes","arguments":{{"frameId":{safe_frame}}}}}"#
        )),
    );
    let safe_scopes = read_dap_response(&output_rx, &mut stdout, 6, "scopes");
    assert!(safe_scopes.contains("\"success\":true"), "{safe_scopes}");
    assert!(safe_scopes.contains("\"name\":\"Locals\""), "{safe_scopes}");
    assert!(
        !safe_scopes.contains("[raw]"),
        "raw scope leaked into Jet view: {safe_scopes}"
    );
    let safe_scope = dap_u32_after(&safe_scopes, "\"name\":\"Locals\",\"variablesReference\":");

    write_dap_request(
        &mut input_pipe,
        &dap_frame(&format!(
            r#"{{"seq":7,"type":"request","command":"variables","arguments":{{"variablesReference":{safe_scope}}}}}"#
        )),
    );
    let safe_variables = read_dap_response(&output_rx, &mut stdout, 7, "variables");
    assert!(
        safe_variables.contains("\"success\":true"),
        "{safe_variables}"
    );
    assert!(
        !safe_variables.contains("__jet_"),
        "generated Rust leaked into Jet locals: {safe_variables}"
    );
    assert!(
        !safe_variables.contains(&rust_file),
        "generated Rust source leaked into Jet locals: {safe_variables}"
    );

    write_dap_request(
        &mut input_pipe,
        &dap_frame(
            r#"{"seq":8,"type":"request","command":"stackTrace","arguments":{"threadId":1,"showRawFrames":true}}"#,
        ),
    );
    let raw = read_dap_response(&output_rx, &mut stdout, 8, "stackTrace");
    assert!(raw.contains("\"success\":true"), "{raw}");
    assert!(raw.contains("\"name\":\"[raw]"), "{raw}");
    assert!(
        raw.contains(&format!("\"path\":\"{}\"", rust_file)),
        "raw stack must expose the generated frame source only in expert mode: {raw}"
    );
    let raw_frame = dap_u32_before(&raw, "\"name\":\"[raw]");

    write_dap_request(
        &mut input_pipe,
        &dap_frame(&format!(
            r#"{{"seq":9,"type":"request","command":"scopes","arguments":{{"frameId":{raw_frame}}}}}"#
        )),
    );
    let raw_scopes = read_dap_response(&output_rx, &mut stdout, 9, "scopes");
    assert!(raw_scopes.contains("\"success\":true"), "{raw_scopes}");
    assert!(
        raw_scopes.contains("\"name\":\"[raw] Locals\""),
        "{raw_scopes}"
    );
    let raw_scope = dap_u32_after(
        &raw_scopes,
        "\"name\":\"[raw] Locals\",\"variablesReference\":",
    );

    write_dap_request(
        &mut input_pipe,
        &dap_frame(&format!(
            r#"{{"seq":10,"type":"request","command":"variables","arguments":{{"variablesReference":{raw_scope}}}}}"#
        )),
    );
    let raw_variables = read_dap_response(&output_rx, &mut stdout, 10, "variables");
    assert!(
        raw_variables.contains("\"success\":true"),
        "{raw_variables}"
    );

    let stopped_count = stdout.matches("\"event\":\"stopped\"").count();
    write_dap_request(
        &mut input_pipe,
        &dap_frame(r#"{"seq":11,"type":"request","command":"next","arguments":{"threadId":1}}"#),
    );
    let next = read_dap_response(&output_rx, &mut stdout, 11, "next");
    assert!(next.contains("\"success\":true"), "{next}");
    read_dap_until(
        &output_rx,
        &mut stdout,
        |output| output.matches("\"event\":\"stopped\"").count() > stopped_count,
        "next stopped event",
    );

    write_dap_request(
        &mut input_pipe,
        &dap_frame(&format!(
            r#"{{"seq":12,"type":"request","command":"variables","arguments":{{"variablesReference":{safe_scope}}}}}"#
        )),
    );
    let stale = read_dap_response(&output_rx, &mut stdout, 12, "variables");
    assert!(stale.contains("\"success\":false"), "{stale}");
    assert!(stale.contains("\"id\":22036"), "{stale}");
    assert!(stale.contains("\"code\":\"E2238\""), "{stale}");

    write_dap_request(
        &mut input_pipe,
        &dap_frame(
            r#"{"seq":13,"type":"request","command":"disconnect","arguments":{"terminateDebuggee":true}}"#,
        ),
    );
    let disconnected = read_dap_response(&output_rx, &mut stdout, 13, "disconnect");
    assert!(disconnected.contains("\"success\":true"), "{disconnected}");
    drop(input_pipe);
    let status = child
        .wait()
        .expect("wait for the production Jet DAP command");
    output_reader.join().expect("join DAP output reader");
    while let Ok(chunk) = output_rx.try_recv() {
        stdout.push_str(&String::from_utf8_lossy(&chunk));
    }
    let _ = std::fs::remove_file(&file);
    assert!(status.success(), "DAP command failed:\nstdout:\n{stdout}");
}

/// #4881: exercise the production root and the debug C stop channel, not a
/// scheduler/interrupt flag injected into generated code. The Jet input is the
/// exact fixed-width witness already used by the remainder parity test.
#[test]
fn ordinary_root_and_debug_dispatch_stop_after_lifo_cleanup() {
    assert!(have("rustc"), "root/debug stop proof requires rustc");
    let source = r#"
fn remainder(value: I8, divisor: I8) -> I8 {
    return value % divisor
}

fn run() {
    print(remainder(I8{7}, I8{0}))
}
"#;
    assert!(include_str!("dev_parts/routine.rs").contains(source));
    let file = native_fixture("root_stop_cleanup", source);
    let compiled = jet::compile_for_debug(&file).expect("fixed-width witness compiles for debug");
    let callable = compiled
        .rust
        .lines()
        .find(|line| {
            line.starts_with("// jet-native-callable:")
                && line.split_whitespace().any(|field| field == "jet=remainder")
        })
        .and_then(|line| {
            line.split_whitespace()
                .find_map(|field| field.strip_prefix("name="))
        })
        .expect("the trap witness must use actual native debug dispatch");
    let probes = r#"
#[cfg(test)]
mod root_stop_regression {
    use super::*;

    #[test]
    #[ignore]
    fn ordinary_root() {
        jet_runtime_boundary(|| {
            let _first = JetDeferredClose::new(|| println!("close-first"));
            let _second = JetDeferredClose::new(|| println!("close-second"));
            println!("body");
            jet_panic("<root-stop>", 1, "root-stop");
        });
        panic!("a stopped root returned");
    }

    #[test]
    #[ignore]
    fn rendered_diagnostic_root() {
        jet_runtime_boundary(|| {
            let _first = JetDeferredClose::new(|| println!("close-first"));
            let _second = JetDeferredClose::new(|| println!("close-second"));
            println!("body");
            let report = jet_runtime_stop_report(
                "E3010", "<root-stop>", 1, "", "", 1, 1, "diagnostic-stop", "",
            );
            jet_runtime_diagnostic(report.rendered);
        });
        panic!("a diagnostic root returned");
    }

    #[test]
    #[ignore]
    fn debug_dispatch() {
        jet_runtime_boundary(|| {
            let _first = JetDeferredClose::new(|| println!("close-first"));
            let _second = JetDeferredClose::new(|| println!("close-second"));
            println!("body");
            let value = @REMAINDER@(7i8, 0i8);
            println!("unexpected-value {value}");
        });
        panic!("a stopped debug call returned");
    }

    #[test]
    #[ignore]
    fn callback_registration_timing() {
        jet_runtime_boundary(|| {
            let _body = JetDeferredClose::new(|| println!("close-body"));
            println!("body");
            jet_runtime_atexit::register(|| {
                let _callback = JetDeferredClose::new(|| println!("close-callback"));
                println!("atexit-first");
                jet_runtime_atexit::register(|| println!("late-before-stop"));
                jet_panic("<callback-stop>", 1, "callback-stop");
            });
            jet_runtime_atexit::register(|| {
                println!("atexit-second");
                let capture = JetDeferredClose::new(|| println!("drop-after-stop"));
                jet_runtime_atexit::register(move || {
                    let _borrowed = &capture;
                    println!("unexpected-after-stop-body");
                });
            });
        });
        panic!("a stopped callback returned");
    }

    #[test]
    #[ignore]
    fn lexical_registration_after_stop() {
        jet_runtime_boundary(|| {
            let _body = JetDeferredClose::new(|| {
                println!("close-body");
                let capture = JetDeferredClose::new(|| println!("drop-after-stop"));
                jet_runtime_atexit::register(move || {
                    let _borrowed = &capture;
                    println!("unexpected-after-stop-body");
                });
            });
            println!("body");
            jet_panic("<lexical-stop>", 1, "lexical-stop");
        });
        panic!("a stopped root returned");
    }

    #[test]
    #[ignore]
    fn stopped_callback_double_panic() {
        jet_runtime_boundary(|| {
            jet_runtime_atexit::register(|| jet_panic("<second-stop>", 1, "second-stop"));
            jet_panic("<first-stop>", 1, "first-stop");
        });
        panic!("stopped cleanup returned");
    }

    #[test]
    fn carrier_roundtrip() {
        assert!(!jet_runtime_process_should_unwind());
        let mut stop = None;
        let value = jet_native_dispatch_boundary(&mut stop, || {
            assert!(jet_runtime_process_should_unwind());
            assert!(jet_runtime_fault_should_unwind());
            assert!(!jet_scheduler_panic_should_unwind(), "an ordinary Root is not a cancellation catch frame");
            let nested = JetRuntimeProcessBoundary::enter();
            drop(nested);
            assert!(jet_runtime_process_should_unwind());
            7u8
        });
        assert!(!jet_runtime_process_should_unwind());
        assert!(stop.is_none());
        assert_eq!(jet_native_dispatch_finish(stop, value), 7);

        let mut stop = None;
        let value: u8 = jet_native_dispatch_boundary(&mut stop, || {
            jet_panic("<debug-stop>", 1, "channel-stop")
        });
        assert!(!jet_runtime_process_should_unwind());
        assert!(stop.is_some());
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| jet_native_dispatch_finish(stop, value)))
            .expect_err("the transport scalar must not escape as a result");
        let report = caught.downcast::<JetRenderedRuntimeStop>().expect("typed report");
        assert_eq!(report.exit_code, 70);
        assert!(report.rendered.contains("channel-stop"));

        let mut stop = None;
        let value: u8 = jet_native_dispatch_boundary(&mut stop, || {
            std::panic::resume_unwind(Box::new(123usize))
        });
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| jet_native_dispatch_finish(stop, value)))
            .expect_err("an opaque host payload must not become a scalar");
        assert_eq!(*caught.downcast::<usize>().expect("original host payload"), 123);
        assert!(!jet_runtime_process_should_unwind());

        let mut stop = None;
        let value: u8 = jet_native_dispatch_boundary(&mut stop, || jet_runtime_explicit_exit(0));
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| jet_native_dispatch_finish(stop, value)))
            .expect_err("explicit exit zero is still a stop, not successful return");
        assert_eq!(caught.downcast::<JetExplicitExit>().expect("explicit exit").code, 0);
    }
}
"#
    .replace("@REMAINDER@", callable);
    let scratch = common::Scratch::new("jet_root_debug_stop_probe");
    let rust_file = scratch.path.join("probe.rs");
    let binary = scratch.path.join("probe");
    std::fs::write(&rust_file, format!("{}\n{probes}", compiled.rust)).unwrap();
    let built = Command::new("rustc")
        .args(["--edition", "2021", "--test"])
        .arg(&rust_file)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "rustc rejected production root/debug probes:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let carrier = Command::new(&binary)
        .args(["--exact", "root_stop_regression::carrier_roundtrip", "--nocapture"])
        .output()
        .unwrap();
    assert!(
        carrier.status.success(),
        "carrier regression failed: {carrier:?}"
    );
    for (probe, code, cleanup) in [
        ("ordinary_root", "E3001", "body\nclose-second\nclose-first\n"),
        ("rendered_diagnostic_root", "E3010", "body\nclose-second\nclose-first\n"),
        ("debug_dispatch", "E3010", "body\nclose-second\nclose-first\n"),
        (
            "callback_registration_timing",
            "E3001",
            "body\nclose-body\natexit-first\nclose-callback\natexit-second\ndrop-after-stop\nlate-before-stop\n",
        ),
        (
            "lexical_registration_after_stop",
            "E3001",
            "body\nclose-body\ndrop-after-stop\n",
        ),
    ] {
        // Share the same file description for both streams so the assertion
        // observes report-vs-cleanup order, not two independently captured logs.
        let path = scratch.path.join(format!("{probe}.log"));
        let log = std::fs::File::create(&path).unwrap();
        let status = Command::new(&binary)
            .args([
                "--exact",
                &format!("root_stop_regression::{probe}"),
                "--ignored",
                "--nocapture",
            ])
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .status()
            .unwrap();
        let transcript = std::fs::read_to_string(path).unwrap();
        assert_eq!(status.code(), Some(70), "{probe}: {transcript}");
        assert!(transcript.contains(cleanup), "{probe}: {transcript}");
        let report = format!("Stop [{code}]");
        assert_eq!(transcript.matches(&report).count(), 1, "{probe}: {transcript}");
        let last_cleanup = cleanup.trim_end().rsplit('\n').next().unwrap();
        assert!(
            transcript.find(last_cleanup).unwrap() < transcript.find(&report).unwrap(),
            "{probe}: report preceded owed cleanup: {transcript}"
        );
        assert!(!transcript.contains("unexpected-value"), "{probe}: {transcript}");
        assert!(!transcript.contains("unexpected-after-stop-body"), "{probe}: {transcript}");
    }
    let path = scratch.path.join("stopped_callback_double_panic.log");
    let log = std::fs::File::create(&path).unwrap();
    let status = Command::new(&binary)
        .args([
            "--exact",
            "root_stop_regression::stopped_callback_double_panic",
            "--ignored",
            "--nocapture",
        ])
        .stdout(Stdio::from(log.try_clone().unwrap()))
        .stderr(Stdio::from(log))
        .status()
        .unwrap();
    let transcript = std::fs::read_to_string(path).unwrap();
    assert!(!status.success(), "{transcript}");
    assert_ne!(status.code(), Some(70), "{transcript}");
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(status.signal(), Some(6), "{transcript}");
    }
    assert!(transcript.contains("panic in a destructor during cleanup\n"), "{transcript}");
    assert!(!transcript.contains("Stop [E3001]"), "{transcript}");
}

#[test]
fn debug_build_carries_line_markers_a_normal_build_does_not() {
    let file = native_fixture("markers", LOOPS);
    let debug_out = jet::compile_for_debug(&file).expect("compiles for debug");
    assert!(
        debug_out.rust.contains("// jet:line "),
        "debug_linemap build should carry `// jet:line N` markers:\n{}",
        debug_out.rust
    );

    let normal_out = jet::compile_with_path(&std::fs::read_to_string(&file).unwrap(), &file)
        .expect("compiles normally");
    assert!(
        !normal_out.rust.contains("// jet:line "),
        "a normal build must stay byte-identical to today's output — no markers leak in \
         when debug_linemap is off (JIT tier + golden tests depend on this)"
    );

    if !have("rustc") || !have("lldb") {
        return;
    }
    let jet_src = std::fs::read_to_string(&file).unwrap();
    let (debug_dir, debug_bin) = native_binary("markers_debug", &debug_out.rust);
    let debug_transcript = jet::Debug::run_native_scripted(
        &debug_bin,
        "prog.rs",
        &debug_out.rust,
        &file,
        &jet_src,
        false,
        &["next"],
    );
    assert!(
        debug_transcript.contains("2 |     n := 3")
            && debug_transcript.contains("3 |     total := 0"),
        "debug source map did not step through mapped statements:\n{debug_transcript}"
    );

    let (normal_dir, normal_bin) = native_binary("markers_normal", &normal_out.rust);
    let normal_transcript = jet::Debug::run_native_scripted(
        &normal_bin,
        "prog.rs",
        &normal_out.rust,
        &file,
        &jet_src,
        false,
        &[],
    );
    assert!(
        normal_transcript.contains("source-mapped entry statement"),
        "normal build unexpectedly supplied a debugger source map:\n{normal_transcript}"
    );
    let _ = std::fs::remove_dir_all(debug_dir);
    let _ = std::fs::remove_dir_all(normal_dir);
}

#[test]
fn line_markers_resolve_every_statement_to_its_source_line() {
    let file = native_fixture("markers_line3", LOOPS);
    let out = jet::compile_for_debug(&file).expect("compiles for debug");
    if !have("rustc") || !have("lldb") {
        return;
    }
    let jet_src = std::fs::read_to_string(&file).unwrap();
    let (dir, binary) = native_binary("markers_line3", &out.rust);
    let transcript = jet::Debug::run_native_scripted(
        &binary,
        "prog.rs",
        &out.rust,
        &file,
        &jet_src,
        false,
        &["next", "next"],
    );
    assert!(
        transcript.contains("2 |     n := 3")
            && transcript.contains("4 |     loop i in 1..n {"),
        "debugger did not resolve mapped statements to Jet source lines:\n{transcript}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// Full end-to-end native session: build a debug binary, launch it under
/// lldb, and drive the SAME `(jet)` vocabulary the interpreter backend uses.
/// Gated on rustc AND lldb; skips (not fails) when either is absent.
#[test]
fn native_session_steps_and_shows_locals() {
    if !have("rustc") || !have("lldb") {
        return;
    }
    let file = native_fixture("native_session", LOOPS);
    let out = jet::compile_for_debug(&file).expect("compiles for debug");
    let scratch = common::Scratch::new("jet_debug_native_build");
    let dir = &scratch.path;
    let rs = dir.join("prog.rs");
    let bin = dir.join("prog");
    std::fs::write(&rs, &out.rust).unwrap();
    let rustc = std::process::Command::new("rustc")
        .args([
            "--edition",
            "2021",
            "-C",
            "debuginfo=2",
            rs.to_str().unwrap(),
            "-o",
            bin.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        rustc.status.success(),
        "rustc rejected the debug-linemap build (I2):\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let jet_src = std::fs::read_to_string(&file).unwrap();
    let transcript = jet::Debug::run_native_scripted(
        &bin,
        "prog.rs",
        &out.rust,
        &file,
        &jet_src,
        false,
        &[
            "locals",
            "next",
            "next",
            "print total",
            "backtrace",
            "continue",
        ],
    );
    assert!(
        transcript.contains("breakpoint hit"),
        "expected an initial stop banner:\n{}",
        transcript
    );
    assert!(
        transcript.contains("locals:"),
        "expected a locals dump:\n{}",
        transcript
    );
    assert!(
        transcript.contains("total = 0"),
        "expected `print total` to show the local:\n{}",
        transcript
    );
    assert!(
        transcript.contains("program finished"),
        "continuing to completion should print the same completion marker \
         the interpreter backend uses:\n{}",
        transcript
    );
    assert!(
        !transcript.contains("__jet_") && !transcript.contains("prog.rs:"),
        "default native projection leaked generated Rust details:\n{}",
        transcript
    );

    let raw = jet::Debug::run_native_scripted(
        &bin,
        "prog.rs",
        &out.rust,
        &file,
        &jet_src,
        true,
        &["locals", "backtrace", "continue"],
    );
    assert!(
        raw.contains("breakpoint hit"),
        "raw native session did not stop at a Jet line:\n{}",
        raw
    );
    assert!(
        raw.contains("[raw]"),
        "raw expert mode must mark generated frames and scopes:\n{}",
        raw
    );

}

#[test]
fn dap_cli_keeps_running_state_transitions_on_the_production_wire() {
    if !have("rustc") || !have("lldb") {
        return;
    }
    let tag = format!(
        "dap_cli_running_state_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos()
    );
    let file = native_fixture(
        &tag,
        "fn run() {\n    total := 0\n    loop i in 1..1000000000 {\n        total += i\n    }\n}\n",
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["debug", "--dap", &file])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the production Jet DAP command");
    let mut input = child.stdin.take().expect("DAP stdin");
    let mut stdout_pipe = child.stdout.take().expect("DAP stdout");
    let (output_tx, output_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let output_reader = std::thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        loop {
            match stdout_pipe.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(size) => {
                    if output_tx.send(chunk[..size].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let mut transcript = String::new();
    write_dap_request(
        &mut input,
        &format!(
            "{}{}",
            dap_frame(
                r#"{"seq":1,"type":"request","command":"initialize","arguments":{"adapterID":"jet","pathFormat":"path"}}"#
            ),
            dap_frame(&format!(
                r#"{{"seq":2,"type":"request","command":"launch","arguments":{{"program":"{}","stopOnEntry":false,"args":[],"env":{{}}}}}}"#,
                file
            )),
        ),
    );
    let launch = read_dap_response(&output_rx, &mut transcript, 2, "launch");
    assert!(launch.contains("\"success\":true"), "{launch}");

    // A restart in Configuring must not execute user code before the gate.
    write_dap_request(
        &mut input,
        &dap_frame(r#"{"seq":3,"type":"request","command":"restart","arguments":{}}"#),
    );
    let configuring_restart = read_dap_response(&output_rx, &mut transcript, 3, "restart");
    assert!(
        configuring_restart.contains("\"success\":true"),
        "{configuring_restart}"
    );
    write_dap_request(
        &mut input,
        &dap_frame(r#"{"seq":4,"type":"request","command":"configurationDone","arguments":{}}"#),
    );
    let configured = read_dap_response(&output_rx, &mut transcript, 4, "configurationDone");
    assert!(configured.contains("\"success\":true"), "{configured}");

    // Both restart and breakpoint replacement must preempt an in-flight native
    // resume, then leave the target in the state the editor requested.
    write_dap_request(
        &mut input,
        &dap_frame(r#"{"seq":5,"type":"request","command":"restart","arguments":{}}"#),
    );
    let running_restart = read_dap_response(&output_rx, &mut transcript, 5, "restart");
    assert!(
        running_restart.contains("\"success\":true"),
        "{running_restart}"
    );

    write_dap_request(
        &mut input,
        &dap_frame(&format!(
            r#"{{"seq":6,"type":"request","command":"setBreakpoints","arguments":{{"source":{{"path":"{}"}},"breakpoints":[{{"line":4}}]}}}}"#,
            file
        )),
    );
    let breakpoints = read_dap_response(&output_rx, &mut transcript, 6, "setBreakpoints");
    assert!(breakpoints.contains("\"success\":true"), "{breakpoints}");
    assert!(breakpoints.contains("\"verified\":true"), "{breakpoints}");
    read_dap_until(
        &output_rx,
        &mut transcript,
        |output| output.contains("\"hitBreakpointIds\":[1]"),
        "replacement breakpoint stop",
    );

    write_dap_request(
        &mut input,
        &dap_frame(r#"{"seq":7,"type":"request","command":"disconnect","arguments":{}}"#),
    );
    let disconnected = read_dap_response(&output_rx, &mut transcript, 7, "disconnect");
    assert!(disconnected.contains("\"success\":true"), "{disconnected}");
    drop(input);
    let status = child
        .wait()
        .expect("wait for the production Jet DAP command");
    output_reader.join().expect("join DAP output reader");
    while let Ok(chunk) = output_rx.try_recv() {
        transcript.push_str(&String::from_utf8_lossy(&chunk));
    }
    let _ = std::fs::remove_file(&file);
    assert!(status.success(), "DAP command failed:\n{transcript}");
}
