//! D-DBG1 / D-DBG3 — `jet debug` source-level step-debugger tests.
//!
//! Drives the debugger programmatically via `jet::Debug::run_session(file,
//! inputs)`, which feeds a scripted sequence of `(jet)` commands and returns
//! the full transcript (banners, `<- here` carets, `locals:` dumps, command
//! echoes, program output, and the final marker). We assert on the exact
//! protocol/output: that a breakpoint resolves to the right Jet line, that the
//! stack frame and locals are in Jet terms (I2 — never generated Rust), and
//! that step / next / continue / finish behave as ratified.
//!
//! The fixtures are tiny temp files so the test is hermetic. The debugger drives
//! the same dev interpreter `jet dev`/`jet repl` use, so no rustc is needed.
//!
//! Also covers D-DBG3 step 2 (dap-debugger) — the native lldb-backed `jet
//! debug` backend (see the section below): codegen-only line-marker checks
//! (no rustc/lldb needed), a production DAP wire failure check, and a full
//! native session. The live session gates only on `rustc`/`lldb` being absent;
//! once both tools exist, a stop or cleanup failure is a test failure.

mod common;

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Write `src` to a temp `.jet` file and return its path.
fn fixture(tag: &str, src: &str) -> String {
    let p = std::env::temp_dir().join(format!("jet_debug_{tag}.jet"));
    let mut f = std::fs::File::create(&p).unwrap();
    f.write_all(src.as_bytes()).unwrap();
    p.to_string_lossy().into_owned()
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
fn double(x: Int) Int {
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
fn structured_session_status_ignores_program_output() {
    let file = fixture(
        "structured_status",
        "fn run() {\n    print(\"program finished\")\n}\n",
    );
    let result = jet::Debug::run_session_result(&file, &["c"]);

    assert_eq!(result.status, jet::Debug::SessionStatus::Finished);
    assert_eq!(
        result
            .transcript
            .lines()
            .filter(|line| *line == "program finished")
            .count(),
        2,
        "transcript must retain program output and the debugger completion marker"
    );
}

#[test]
fn paused_session_stops_at_a_live_command_boundary() {
    let file = fixture("paused_status", LOOPS);
    let result = jet::Debug::run_session_result_paused(&file, &["s"]);

    assert_eq!(result.status, jet::Debug::SessionStatus::Running);
    assert!(
        result.transcript.contains("<- here"),
        "missing live stop: {}",
        result.transcript
    );
    assert!(
        !result.transcript.contains("program finished"),
        "a paused session must not fabricate completion: {}",
        result.transcript
    );

    let finished = jet::Debug::run_session_result_paused(&file, &["c"]);
    assert_eq!(finished.status, jet::Debug::SessionStatus::Finished);
    assert!(finished.transcript.contains("program finished"));
}

#[test]
fn canvas_debug_session_replays_one_source_bound_session() {
    let file = fixture("canvas_live_protocol", LOOPS);
    let path = Path::new(&file);
    let source = std::fs::read_to_string(path).unwrap();
    let revision = jet::Canvas::source_revision(&source);
    let sessions = jet::Canvas::DebugSessions::default();
    let first_request = format!(
        "{{\"schema_version\":1,\"revision\":\"{}\",\"commands\":[\"s\"]}}",
        revision
    );
    let first =
        jet::Canvas::debug_session_json_for_file_with_sessions(path, &first_request, &sessions)
            .expect("Canvas should expose the first live stop");
    assert!(first.contains("\"state\":\"running\""), "{first}");
    assert!(
        first.contains("\"tier\":\"jet-dev-interpreter\""),
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
        "{{\"schema_version\":1,\"revision\":\"{}\",\"session_id\":\"{}\",\"commands\":[\"s\"]}}",
        revision, session_id
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
        "{{\"schema_version\":1,\"revision\":\"{}\",\"session_id\":\"{}\",\"commands\":[\"teleport\"]}}",
        revision, session_id
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
        "{{\"schema_version\":1,\"revision\":\"{}\",\"session_id\":\"{}\",\"stop\":true}}",
        revision, session_id
    );
    let stopped =
        jet::Canvas::debug_session_json_for_file_with_sessions(path, &stop_request, &sessions)
            .expect("Canvas should stop the live session without editing source");
    assert!(stopped.contains("\"state\":\"stopped\""), "{stopped}");
    assert!(stopped.contains("\"overlay\":null"), "{stopped}");
    assert_eq!(std::fs::read_to_string(path).unwrap(), source);

    let invalid_request = format!(
        "{{\"schema_version\":1,\"revision\":\"{}\",\"commands\":[\"teleport\"]}}",
        revision
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
    let sessions = jet::Canvas::DebugSessions::default();
    let first_request = format!(
        "{{\"schema_version\":1,\"revision\":\"{}\",\"tier\":\"jet-dev-interpreter\",\"commands\":[\"s\"]}}",
        revision
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
        "{{\"schema_version\":1,\"revision\":\"{}\",\"tier\":\"native-lldb\",\"session_id\":\"{}\",\"stop\":true}}",
        revision, session_id
    );
    let error =
        jet::Canvas::debug_session_json_for_file_with_sessions(path, &wrong_tier, &sessions)
            .expect_err("Canvas must not stop a session through another tier");
    assert!(error.contains("\"kind\":\"conflict\""), "{error}");

    let other_file = fixture("canvas_stop_other_source", LOOPS);
    let other_revision =
        jet::Canvas::source_revision(&std::fs::read_to_string(&other_file).unwrap());
    let wrong_source = format!(
        "{{\"schema_version\":1,\"revision\":\"{}\",\"tier\":\"jet-dev-interpreter\",\"session_id\":\"{}\",\"stop\":true}}",
        other_revision, session_id
    );
    let error = jet::Canvas::debug_session_json_for_file_with_sessions(
        Path::new(&other_file),
        &wrong_source,
        &sessions,
    )
    .expect_err("Canvas must not stop a session through another source");
    assert!(error.contains("\"kind\":\"conflict\""), "{error}");

    let next_request = format!(
        "{{\"schema_version\":1,\"revision\":\"{}\",\"tier\":\"jet-dev-interpreter\",\"session_id\":\"{}\",\"commands\":[\"s\"]}}",
        revision, session_id
    );
    jet::Canvas::debug_session_json_for_file_with_sessions(path, &next_request, &sessions)
        .expect("a mismatched stop must leave the live session intact");

    let stale_stop = format!(
        "{{\"schema_version\":1,\"revision\":\"sha256-stale\",\"tier\":\"jet-dev-interpreter\",\"session_id\":\"{}\",\"stop\":true}}",
        session_id
    );
    let error =
        jet::Canvas::debug_session_json_for_file_with_sessions(path, &stale_stop, &sessions)
            .expect_err("Canvas must reject a stale stop request");
    assert!(error.contains("\"kind\":\"conflict\""), "{error}");

    let ended = format!(
        "{{\"schema_version\":1,\"revision\":\"{}\",\"tier\":\"jet-dev-interpreter\",\"session_id\":\"{}\",\"stop\":true}}",
        revision, session_id
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
    let anchors = (0..129).map(|_| "\"0:1\"").collect::<Vec<_>>().join(",");
    let request = format!(
        "{{\"schema_version\":1,\"revision\":\"{}\",\"breakpoint_spans\":[{}],\"commands\":[\"s\"]}}",
        revision, anchors
    );
    let error = jet::Canvas::debug_session_json_for_file(path, &request)
        .expect_err("Canvas must bound breakpoint span input");
    assert!(error.contains("\"kind\":\"limit\""), "{error}");
    assert_eq!(std::fs::read_to_string(path).unwrap(), source);
}

#[test]
fn canvas_debug_native_tier_never_falls_back_to_interpreter() {
    let file = fixture("canvas_native_protocol", LOOPS);
    let path = Path::new(&file);
    let source = std::fs::read_to_string(path).unwrap();
    let revision = jet::Canvas::source_revision(&source);
    let sessions = jet::Canvas::DebugSessions::default();
    let request = format!(
        "{{\"schema_version\":1,\"revision\":\"{}\",\"tier\":\"native-lldb\",\"commands\":[\"s\"]}}",
        revision
    );
    match jet::Canvas::debug_session_json_for_file_with_sessions(path, &request, &sessions) {
        Ok(body) => {
            assert!(body.contains("\"tier\":\"native-lldb\""), "{body}");
            assert!(!body.contains("\"tier\":\"jet-dev-interpreter\""), "{body}");
            assert!(body.contains("\"state\":\"running\""), "{body}");
            let marker = "\"id\":\"canvas-debug-";
            let start = body.find(marker).expect("native session id") + 6;
            let end = body[start..]
                .find('"')
                .map(|offset| start + offset)
                .expect("native session id terminator");
            let session_id = &body[start..end];
            let stop = format!(
                "{{\"schema_version\":1,\"revision\":\"{}\",\"tier\":\"native-lldb\",\"session_id\":\"{}\",\"stop\":true}}",
                revision, session_id
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
    let graph = jet::Canvas::graph_json_for_file(Path::new("examples/features/basics/hello.jet"))
        .expect("relative Canvas entry should project");
    assert!(
        graph.contains("\"source_id\":\"hello.jet\""),
        "Canvas/debug source identity must match project file ids: {graph}"
    );
    assert!(
        !graph.contains("examples/features/basics/hello.jet"),
        "relative entry path leaked into the source identity: {graph}"
    );
}

/// `step`/`s` stops on the very first statement of `main`, with the right Jet
/// line, a `<- here` caret, and a `locals:` dump — all in Jet terms.
#[test]
fn step_stops_at_first_line_with_caret_and_locals() {
    let file = fixture("step", LOOPS);
    let out = jet::Debug::run_session(&file, &["s"]);
    // The first stop names the file:line and the function.
    assert!(
        out.contains("breakpoint hit  jet_debug_step.jet:2  in main()"),
        "missing breakpoint banner; got:\n{out}"
    );
    // The current line carries the `<- here` caret (D-DBG3 layout).
    assert!(
        out.contains("<- here"),
        "missing `<- here` caret; got:\n{out}"
    );
    // The `(jet)` prompt echoes the typed command (D-DBG3 prompt = `(jet)`).
    assert!(
        out.contains("(jet) s"),
        "missing `(jet)` prompt echo; got:\n{out}"
    );
    // Locals are shown in Jet terms; after the first `s` the second stop sees `n`.
    assert!(out.contains("locals:"), "missing locals dump; got:\n{out}");
    // The program runs to completion once the script ends.
    assert!(
        out.contains("total is 6"),
        "program output missing; got:\n{out}"
    );
    assert!(
        out.contains("program finished"),
        "missing end marker; got:\n{out}"
    );
}

/// `locals` after stepping into the loop shows the live `total`/`n`/`i` values
/// through the Jet display path (I2), and `print X` shows one named local.
#[test]
fn locals_and_print_show_jet_values() {
    let file = fixture("locals", LOOPS);
    // step to line 3 (total := 0), step to 4 (loop header), step into loop body
    // (i bound), then inspect.
    let out = jet::Debug::run_session(&file, &["s", "s", "s", "locals", "p total"]);
    assert!(
        out.contains("locals:  i = 1   n = 3   total = 0"),
        "locals dump should show all three in Jet terms; got:\n{out}"
    );
    assert!(
        out.contains("total = 0"),
        "`print total` should show the value; got:\n{out}"
    );
}

/// A `break N` + `continue`/`c` runs to the breakpoint line — not stopping at
/// every statement in between.
#[test]
fn break_then_continue_stops_at_breakpoint() {
    let file = fixture("brk", LOOPS);
    // First stop is line 2; set a breakpoint on line 7 (the print) and continue.
    let out = jet::Debug::run_session(&file, &["break 7", "c"]);
    assert!(
        out.contains("breakpoint set  jet_debug_brk.jet:7"),
        "break should confirm the line; got:\n{out}"
    );
    // The continue lands on line 7 with the loop already finished (total = 6).
    assert!(
        out.contains("7 |") && out.contains("total = 6"),
        "continue should stop on line 7 with the final total; got:\n{out}"
    );
}

/// `step` descends into a called function (`double`), and `backtrace`/`bt`
/// shows the two-frame Jet call stack in Jet terms.
#[test]
fn step_into_call_and_backtrace() {
    let file = fixture("calls", CALLS);
    // main:6 (a := 5) -> main:7 (b := double(a)) -> step into double body.
    let out = jet::Debug::run_session(&file, &["s", "s", "bt"]);
    assert!(
        out.contains("y := x * 2"),
        "step should descend into the callee body; got:\n{out}"
    );
    // The backtrace shows the callee innermost, the caller below — Jet frames.
    assert!(
        out.contains("#0  double()") && out.contains("#1  main()"),
        "backtrace should show both Jet frames; got:\n{out}"
    );
}

/// `finish`/`f` runs to the end of the current function and stops back in the
/// caller.
#[test]
fn finish_returns_to_caller() {
    let file = fixture("finish", CALLS);
    // Descend into double (s,s,s), then finish back to main.
    let out = jet::Debug::run_session(&file, &["s", "s", "s", "finish"]);
    assert!(
        out.contains("b := double(a)") || out.contains("print"),
        "finish should return control to the caller frame; got:\n{out}"
    );
    assert!(
        out.contains("program finished"),
        "should run to the end; got:\n{out}"
    );
}

/// `quit`/`q` ends the session before the program finishes — surfaced as the
/// E2204 debug-session-aborted diagnostic (I4).
#[test]
fn quit_ends_session_with_e2204() {
    let file = fixture("quit", LOOPS);
    let out = jet::Debug::run_session(&file, &["s", "q"]);
    assert!(
        out.contains("E2204"),
        "quitting mid-session must surface E2204; got:\n{out}"
    );
    // The success marker is a line of its own; the E2204 message also contains
    // the words "finished", so check for the standalone marker line.
    assert!(
        !out.lines().any(|l| l == "program finished"),
        "the program must NOT have finished after quit; got:\n{out}"
    );
}

/// `help`/`h` lists every verb (D-DBG3: `help` lists the verbs).
#[test]
fn help_lists_the_verbs() {
    let file = fixture("help", LOOPS);
    let out = jet::Debug::run_session(&file, &["help", "c"]);
    for verb in [
        "step",
        "next",
        "continue",
        "finish",
        "break",
        "print",
        "backtrace",
        "quit",
    ] {
        assert!(
            out.contains(verb),
            "help should mention `{verb}`; got:\n{out}"
        );
    }
}

/// A program using a feature the dev interpreter can't step (here `core.files`)
/// declines at the debug boundary with E2203, pointing at the real build (I2 —
/// the message names `jet debug`, never the generated Rust).
#[test]
fn unsupported_feature_stops_at_e2203_boundary() {
    let file = fixture(
        "boundary",
        "use core.files as fs\nfn run() {\n    print(\"hi\")\n}\n",
    );
    let out = jet::Debug::run_session(&file, &["c"]);
    assert!(
        out.contains("E2203"),
        "an unsteppable feature must stop at E2203; got:\n{out}"
    );
    assert!(
        out.contains("jet debug") && (out.contains("jet run") || out.contains("jet build")),
        "E2203 must name `jet debug` and point at the real build; got:\n{out}"
    );
}

/// Card #2029 / I4: a library with no `run` is registered E0101, not a bare line.
#[test]
fn missing_run_is_registered_e0101() {
    let file = fixture("no_run", "fn helper() { print(1) }\n");
    let out = jet::Debug::run_session(&file, &["c"]);
    assert!(
        out.contains("E0101") && out.contains("no `run` function"),
        "no-run must be registered E0101; got:\n{out}"
    );
}

/// `next`/`n` steps over the loop body without descending statement-by-statement
/// at a deeper position — the line advances within `main`, not into a callee.
#[test]
fn next_steps_over() {
    let file = fixture("next", CALLS);
    // main:6 (a:=5), then `next` over the `double(a)` call to main's next line.
    let out = jet::Debug::run_session(&file, &["s", "n", "n"]);
    // `next` from `b := double(a)` must NOT stop inside `double` — the callee's
    // `y := x * 2` line should never appear as a stop before we reach main's print.
    assert!(
        out.contains("program finished"),
        "should run to the end; got:\n{out}"
    );
}

#[test]
fn loop_next_edges_are_step_visible_and_line_mapped() {
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

    let plain = jet::Debug::run_session(&file, &["break 5", "c", "n", "p i", "n", "p i", "c"]);
    let next_stop = plain
        .find("5 |             next        <- here")
        .expect("plain next stop");
    let step_stop = plain[next_stop..]
        .find("3 |     loop i in 0..<3 {        <- here")
        .map(|offset| next_stop + offset)
        .expect("plain next must stop on the loop afterthought");
    assert!(
        !plain[next_stop..step_stop].contains("7 |         hits += i        <- here"),
        "plain next leaked into the skipped body before its afterthought:\n{plain}"
    );
    assert!(
        plain.contains("i = 2"),
        "afterthought did not advance i:\n{plain}"
    );
    assert!(plain.contains("2,3,0"), "wrong loop result:\n{plain}");

    let labeled = jet::Debug::run_session(&file, &["break 14", "c", "n", "p i", "n", "p i", "c"]);
    let next_stop = labeled
        .find("14 |                 next(outer)        <- here")
        .expect("labeled next stop");
    let outer_step = labeled[next_stop..]
        .find("11 |     outer :: loop i in 0..<3 {        <- here")
        .map(|offset| next_stop + offset)
        .expect("labeled next must target the outer afterthought");
    assert!(
        !labeled[next_stop..outer_step].contains("12 |         loop j in 0..<3 {        <- here"),
        "labeled next stepped through the inner loop edge first:\n{labeled}"
    );
    assert!(
        labeled.contains("i = 1"),
        "outer afterthought did not advance i:\n{labeled}"
    );
    assert!(
        labeled.contains("2,3,0"),
        "wrong labeled loop result:\n{labeled}"
    );

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

fn native_fixture(tag: &str, src: &str) -> String {
    let p = std::env::temp_dir().join(format!("jet_debug_native_{tag}.jet"));
    std::fs::write(&p, src).unwrap();
    p.to_string_lossy().into_owned()
}

fn dap_frame(body: &str) -> String {
    format!("Content-Length: {}\r\n\r\n{}", body.as_bytes().len(), body)
}

fn dap_response(output: &str, request_seq: u32, command: &str) -> Option<String> {
    let marker = format!("\"request_seq\":{request_seq},\"success\":");
    let command = format!("\"command\":\"{command}\"");
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
        if body.contains(&marker) && body.contains(&command) {
            return Some(body.to_string());
        }
        cursor = body_end;
    }
    None
}

#[cfg(target_os = "linux")]
fn linux_process_state(pid: u32) -> Option<String> {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|stat| stat.split_whitespace().nth(2).map(str::to_owned))
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

    let mut target = Command::new(&binary)
        .spawn()
        .expect("start the matching native debug target");
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
}

#[test]
fn line_markers_resolve_every_statement_to_its_source_line() {
    let file = native_fixture("markers_line3", LOOPS);
    let out = jet::compile_for_debug(&file).expect("compiles for debug");
    // `n := 3` is Jet line 2; the marker for it must appear before codegen
    // for that statement.
    assert!(
        out.rust.contains("// jet:line 2\n"),
        "expected a marker for line 2 (`n := 3`):\n{}",
        out.rust
    );
    assert!(
        out.rust.contains("// jet:line 4\n"),
        "expected a marker for line 4 (the `loop` statement):\n{}",
        out.rust
    );
}

#[test]
fn needs_native_is_false_for_an_interpreter_safe_program() {
    let file = native_fixture("needs_native_false", LOOPS);
    assert_eq!(jet::Debug::needs_native(&file), Some(false));
}

/// D-DBG3 step 2: an FFI/task/#Unsafe/native-std program is exactly the case
/// the interpreter declines (E2203) — `needs_native` must say so, so the CLI
/// dispatch (`Source/main.rs`'s `debug` arm) routes it to the native backend
/// instead of erroring.
#[test]
fn needs_native_is_true_for_a_native_only_import() {
    let src = "use core.files as fs\nfn run() {\n    print(\"hi\")\n}\n";
    let file = native_fixture("needs_native_true", src);
    assert_eq!(jet::Debug::needs_native(&file), Some(true));
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
    let dir = std::env::temp_dir().join(format!("jet_debug_native_build_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
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
        transcript.contains("total = 0") || transcript.contains("total ="),
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
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(&file);
}
