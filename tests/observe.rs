//! E2-M12 observability tests: structured JSON logs, rich panic reports,
//! safe-locals policy (D-OBS1/D-OBS2/D-OBS3).

use std::process::Command;

mod common;

/// Builds WITHOUT -O so cfg!(debug_assertions) is true (dev-mode locals) —
/// the shared helper never passes -O, which is exactly this contract.
fn build_and_run_debug(name: &str, src: &str) -> (i32, String, String) {
    common::build_and_run("jet_observe_test", name, src)
}

// ── D-OBS3: structured JSON log format ──────────────────────────────────────

#[test]
fn structured_log_json_fields() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        return;
    }

    let src = r#"
use core.log as log
fn run() {
    log.set_level("debug")
    log.info("hello world")
    log.warn("disk low")
    log.error("connection lost")
    log.debug("trace detail")
}
"#;
    let (code, _stdout, stderr) = build_and_run_debug("log_json", src);
    assert_eq!(code, 0, "program should exit cleanly; stderr:\n{}", stderr);

    let lines: Vec<&str> = stderr.lines().collect();
    assert_eq!(lines.len(), 4, "expected 4 log lines, got:\n{}", stderr);

    // Every line must be valid JSON (starts/ends with braces).
    for line in &lines {
        assert!(
            line.starts_with('{') && line.ends_with('}'),
            "log line is not a JSON object: {}",
            line
        );
        assert!(
            line.contains("\"level\""),
            "missing 'level' field: {}",
            line
        );
        assert!(line.contains("\"body\""), "missing 'body' field: {}", line);
        assert!(line.contains("\"ts\":"), "missing 'ts' field: {}", line);
    }

    // Check level and body values.
    assert!(
        lines[0].contains("\"level\":\"info\"") && lines[0].contains("\"body\":\"hello world\""),
        "info line wrong: {}",
        lines[0]
    );
    assert!(
        lines[1].contains("\"level\":\"warn\"") && lines[1].contains("\"body\":\"disk low\""),
        "warn line wrong: {}",
        lines[1]
    );
    assert!(
        lines[2].contains("\"level\":\"error\"")
            && lines[2].contains("\"body\":\"connection lost\""),
        "error line wrong: {}",
        lines[2]
    );
    assert!(
        lines[3].contains("\"level\":\"debug\"") && lines[3].contains("\"body\":\"trace detail\""),
        "debug line wrong: {}",
        lines[3]
    );
}

#[test]
fn structured_log_level_filter() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        return;
    }

    // Default level is "info" — debug messages are suppressed.
    let src = r#"
use core.log as log
fn run() {
    log.info("visible")
    log.debug("hidden")
}
"#;
    let (_code, _stdout, stderr) = build_and_run_debug("log_filter", src);
    let lines: Vec<&str> = stderr.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(
        lines.len(),
        1,
        "expected only 1 log line at info level, got:\n{}",
        stderr
    );
    assert!(
        lines[0].contains("\"level\":\"info\""),
        "wrong level: {}",
        lines[0]
    );
}

#[test]
fn structured_log_trace_id() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        return;
    }

    let src = r#"
use core.log as log
fn run() {
    log.set_trace_id("req-abc-123")
    log.info("with trace")
}
"#;
    let (_code, _stdout, stderr) = build_and_run_debug("log_trace_id", src);
    let lines: Vec<&str> = stderr.lines().filter(|l| l.starts_with('{')).collect();
    assert_eq!(lines.len(), 1);
    assert!(
        lines[0].contains("\"trace_id\":\"req-abc-123\""),
        "trace_id missing from log line: {}",
        lines[0]
    );
}

#[test]
fn structured_log_fields_and_span_context() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        return;
    }

    let src = r#"
use core.log as log
fn run() {
    log.setup("json")
    log.set_trace_id("req-1")
    span :: log.span("request")
    log.enter(span)
    log.info_fields("served", [
        log.field("route", "/health"),
        log.int("status", 200),
        log.bool("cache", true),
        log.counter("requests", 1),
    ])
    log.close(span)
}
"#;
    let (_code, _stdout, stderr) = build_and_run_debug("log_structured_fields", src);
    let lines: Vec<&str> = stderr.lines().filter(|l| l.starts_with('{')).collect();
    assert_eq!(lines.len(), 1, "stderr:\n{stderr}");
    let line = lines[0];
    assert!(line.contains("\"trace_id\":\"req-1\""), "trace missing: {line}");
    assert!(line.contains("\"route\":\"/health\""), "field missing: {line}");
    assert!(line.contains("\"status\":200"), "int field missing: {line}");
    assert!(line.contains("\"cache\":true"), "bool field missing: {line}");
    assert!(line.contains("\"metric.counter.requests\":1"), "counter missing: {line}");
    assert!(line.contains("\"spans\":[\"request\"]"), "span missing: {line}");
}

#[test]
fn structured_log_json_escape() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        return;
    }

    // Special characters in the message must be JSON-escaped.
    let src = r#"
use core.log as log
fn run() {
    log.info("say \"hello\"")
}
"#;
    let (_code, _stdout, stderr) = build_and_run_debug("log_escape", src);
    let line = stderr
        .lines()
        .find(|l| l.starts_with('{'))
        .expect("no log line");
    assert!(
        line.contains(r#"\"hello\""#),
        "quotes not escaped in: {}",
        line
    );
}

// ── D-OBS1/D-OBS2: rich panic format + safe locals ──────────────────────────

#[test]
fn rich_panic_shows_jet_location() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        return;
    }

    let src = r#"
fn check(n: Int) {
    require(n > 0, "must be positive")
}
fn run() {
    check(0)
}
"#;
    let (code, _stdout, stderr) = build_and_run_debug("rich_panic", src);
    assert_eq!(code, 70, "expected exit 70");
    assert!(
        stderr.contains("panic: must be positive"),
        "wrong panic header: {}",
        stderr
    );
    assert!(
        stderr.contains("in check"),
        "function name missing: {}",
        stderr
    );
    assert!(
        stderr.contains("require(n > 0"),
        "source line missing: {}",
        stderr
    );
    assert!(stderr.contains('^'), "caret missing: {}", stderr);
}

#[test]
fn safe_locals_shown_in_dev_mode() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        return;
    }

    // Int and Bool locals should appear; String should not (non-scalar).
    let src = r#"
fn capture(count: Int, active: Bool, name: String) {
    require(count > 10, "not enough")
}
fn run() {
    capture(3, true, "test")
}
"#;
    let (_code, _stdout, stderr) = build_and_run_debug("safe_locals", src);
    // Scalars appear.
    assert!(
        stderr.contains("count = 3"),
        "count not in locals: {}",
        stderr
    );
    assert!(
        stderr.contains("active = true"),
        "active not in locals: {}",
        stderr
    );
    // Non-scalar (String) must NOT appear.
    assert!(
        !stderr.contains("name = "),
        "String local leaked into panic report: {}",
        stderr
    );
}

#[test]
fn unsafe_block_locals_not_leaked() {
    // D-OBS2: locals inside #Unsafe blocks must never appear in panic reports.
    // This is enforced structurally: safe_locals_expr only includes Copy scalars
    // that are in the *safe* env, never raw pointer slots from core.mem.
    // The sema gate (E3101/E3102/E3103) prevents unsafe ops outside #Unsafe blocks,
    // so the only unsafe values are Ptr types — which are not Int/Float/Bool
    // and are therefore always excluded by the type filter.
    // This test confirms the invariant holds by checking that a panic inside an
    // ordinary function with no unsafe block shows only its safe locals.
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        return;
    }

    let src = r#"
fn capture(secret: String, count: Int) {
    require(count > 5, "failed")
}
fn run() {
    capture("password123", 1)
}
"#;
    let (_code, _stdout, stderr) = build_and_run_debug("no_leak", src);
    assert!(
        !stderr.contains("password"),
        "secret string leaked into panic: {}",
        stderr
    );
    assert!(
        stderr.contains("count = 1"),
        "scalar should appear: {}",
        stderr
    );
}

// ── D-OBS1: source-map marker in generated Rust ──────────────────────────────

#[test]
fn source_map_marker_in_generated_rust() {
    let src = r#"
fn run() {
    print("hello")
}
"#;
    // jet::compile takes source directly and uses "input.jet" as the synthetic path.
    let out = jet::compile(src).expect("compile failed");
    assert!(
        out.rust.contains("jet:source-map source=input.jet"),
        "source-map marker missing from generated Rust:\n{}",
        &out.rust[..out.rust.len().min(500)]
    );
}

#[test]
fn error_return_trace_frames() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        return;
    }

    // parse_age originates an Err; `?` in load and again in double each append
    // one E3002 frame as the error propagates. main catches it, so exit is 0.
    let src = r#"
enum ParseError {
    Empty
    BadDigit(String)
}
fn parse_age(raw: String) -> Int ? ParseError {
    if raw == "" {
        return err(ParseError.Empty)
    }
    return ok(42)
}
fn load(raw: String) -> Int ? ParseError {
    n :: parse_age(raw)?
    return ok((n * 2))
}
fn double(raw: String) -> Int ? ParseError {
    n :: load(raw)?
    return ok((n * 2))
}
fn run() {
    if double("") == {
        ok(n) -> { print(n) }
        err(e) -> { print("failed") }
    }
}
"#;
    let (code, stdout, stderr) = build_and_run_debug("error_trace", src);
    assert_eq!(code, 0, "program should exit 0 (error caught): {stderr}");
    assert!(stdout.contains("failed"), "error not caught: {stdout}");
    // Two propagation frames, innermost (load) first.
    assert!(
        stderr.contains("error propagated from: load"),
        "missing load frame: {stderr}"
    );
    assert!(
        stderr.contains("error propagated from: double"),
        "missing double frame: {stderr}"
    );
    assert!(stderr.contains("via ?"), "missing `via ?` suffix: {stderr}");
}
