//! E2-M18 — REPL transcript tests (D-REPL20=A).
//!
//! Each transcript in `tests/repl/*.txt` is run through `jet::REPL::run_transcript`.
//! Lines starting with `> ` are REPL inputs; other non-comment lines are
//! expected output. Lines starting with `# ` are ignored.
//!
//! Test failures show the first diverging expected/actual line.

use jet::REPL::{run_transcript, run_transcript_with_flags};

fn run_repl_process(state: &std::path::Path, input: &[u8], limit: Option<&str>) -> std::process::Output {
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    let mut command = Command::new(env!("CARGO_BIN_EXE_jet"));
    command
        .arg("repl")
        .env("XDG_STATE_HOME", state)
        .env_remove("JET_REPL_HISTORY")
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(limit) = limit {
        command.env("JET_REPL_HISTORY_LIMIT", limit);
    }
    let mut child = command.spawn().expect("start repl");
    child.stdin.as_mut().unwrap().write_all(input).unwrap();
    child.wait_with_output().expect("finish repl")
}

fn spawn_repl_process(state: &std::path::Path) -> std::process::Child {
    use std::process::{Command, Stdio};
    Command::new(env!("CARGO_BIN_EXE_jet"))
        .arg("repl")
        .env("XDG_STATE_HOME", state)
        .env_remove("JET_REPL_HISTORY")
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start repl")
}

fn wait_for_history_dir(state: &std::path::Path) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !state.join("jet").is_dir() {
        assert!(std::time::Instant::now() < deadline, "history directory was not opened");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    std::thread::sleep(std::time::Duration::from_millis(50));
}

#[test]
fn repl_history_persists_only_successes_searches_clears_and_is_private() {
    let state = std::env::temp_dir().join(format!("jet_repl_history_{}", std::process::id()));
    std::fs::remove_dir_all(&state).ok();
    let first = run_repl_process(&state, b"answer :: 42\nmissing_name\n:quit\n", None);
    assert!(first.status.success(), "first session: {:?}", first.status);

    let history = state.join("jet/repl-history");
    let stored = std::fs::read_to_string(&history).expect("history file");
    assert_eq!(stored.lines().count(), 1, "failed turn persisted: {stored:?}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&history).unwrap().permissions().mode() & 0o777, 0o600);
    }

    let second = run_repl_process(
        &state,
        b":history search answer\n:history search missing\n:history clear\n:quit\n",
        None,
    );
    let out = String::from_utf8_lossy(&second.stdout);
    assert!(out.contains("answer :: 42"), "search output: {out:?}");
    assert!(out.contains("No history matches."), "failed turn was searchable: {out:?}");
    assert!(out.contains("History cleared."), "clear output: {out:?}");
    assert!(!history.exists(), "clear must erase whole file");
    std::fs::remove_dir_all(state).ok();
}

#[test]
fn repl_history_limit_and_corrupt_tail_recover_visibly() {
    let state = std::env::temp_dir().join(format!("jet_repl_history_tail_{}", std::process::id()));
    std::fs::remove_dir_all(&state).ok();
    let first = run_repl_process(&state, b"1 + 1\n2 + 2\n3 + 3\n:quit\n", Some("2"));
    assert!(first.status.success());
    let history = state.join("jet/repl-history");
    use std::io::Write as _;
    std::fs::OpenOptions::new().append(true).open(&history).unwrap().write_all(b"broken-tail").unwrap();

    let second = run_repl_process(&state, b":history search +\n:quit\n", Some("2"));
    let out = String::from_utf8_lossy(&second.stdout);
    let err = String::from_utf8_lossy(&second.stderr);
    assert!(!out.contains("1 + 1"), "retention exceeded: {out:?}");
    assert!(out.contains("2 + 2") && out.contains("3 + 3"), "retained entries missing: {out:?}");
    assert!(err.contains("corrupt history tail") && err.contains("discarded"), "warning missing: {err:?}");
    assert!(!std::fs::read_to_string(&history).unwrap().contains("broken-tail"));
    std::fs::remove_dir_all(state).ok();
}

#[test]
fn repl_history_off_is_session_only_and_visible_storage_failure_falls_back() {
    use std::io::Write as _;
    use std::process::{Command, Stdio};
    let state = std::env::temp_dir().join(format!("jet_repl_history_off_{}", std::process::id()));
    std::fs::remove_dir_all(&state).ok();
    let mut child = Command::new(env!("CARGO_BIN_EXE_jet"))
        .arg("repl").env("XDG_STATE_HOME", &state).env("JET_REPL_HISTORY", "off")
        .env("NO_COLOR", "1").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    child.stdin.as_mut().unwrap().write_all(b"8 + 9\n:history search 8\n:quit\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(String::from_utf8_lossy(&output.stdout).contains("8 + 9"));
    assert!(!state.join("jet/repl-history").exists());

    let blocked = state.join("blocked");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(&blocked, b"not a directory").unwrap();
    let fallback = run_repl_process(&blocked, b":quit\n", None);
    assert!(String::from_utf8_lossy(&fallback.stderr).contains("session-only history"));
    std::fs::remove_dir_all(state).ok();
}

#[cfg(unix)]
#[test]
fn repl_history_rejects_symlinked_state_parent_without_chmod_or_escape() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let root = std::env::temp_dir().join(format!("jet_repl_history_link_{}", std::process::id()));
    let outside = std::env::temp_dir().join(format!("jet_repl_history_outside_{}", std::process::id()));
    std::fs::remove_dir_all(&root).ok();
    std::fs::remove_dir_all(&outside).ok();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o755)).unwrap();
    symlink(&outside, root.join("linked")).unwrap();

    let output = run_repl_process(&root.join("linked/state"), b"5 + 5\n:quit\n", None);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("session-only history"), "fallback invisible: {stderr:?}");
    assert!(!outside.join("state").exists(), "history escaped through state symlink");
    assert_eq!(std::fs::metadata(&outside).unwrap().permissions().mode() & 0o777, 0o755);
    std::fs::remove_dir_all(root).ok();
    std::fs::remove_dir_all(outside).ok();
}

#[cfg(unix)]
#[test]
fn repl_history_parent_swap_stays_on_held_directory() {
    use std::io::Write as _;
    use std::os::unix::fs::symlink;
    let state = std::env::temp_dir().join(format!("jet_repl_history_swap_{}", std::process::id()));
    let outside = std::env::temp_dir().join(format!("jet_repl_history_swap_out_{}", std::process::id()));
    std::fs::remove_dir_all(&state).ok();
    std::fs::remove_dir_all(&outside).ok();
    std::fs::create_dir_all(&outside).unwrap();
    let mut child = spawn_repl_process(&state);
    wait_for_history_dir(&state);
    let held = state.join("held-jet");
    std::fs::rename(state.join("jet"), &held).unwrap();
    symlink(&outside, state.join("jet")).unwrap();
    child.stdin.as_mut().unwrap().write_all(b"4 + 4\n:quit\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(!outside.join("repl-history").exists(), "swap redirected history write");
    assert_eq!(std::fs::read_to_string(held.join("repl-history")).unwrap().lines().count(), 1);
    std::fs::remove_file(state.join("jet")).ok();
    std::fs::remove_dir_all(state).ok();
    std::fs::remove_dir_all(outside).ok();
}

#[test]
fn repl_history_two_processes_merge_and_clear_cannot_resurrect() {
    use std::io::Write as _;
    let state = std::env::temp_dir().join(format!("jet_repl_history_tx_{}", std::process::id()));
    std::fs::remove_dir_all(&state).ok();
    let mut first = spawn_repl_process(&state);
    let mut second = spawn_repl_process(&state);
    wait_for_history_dir(&state);
    let mut first_stdin = first.stdin.take().unwrap();
    let mut second_stdin = second.stdin.take().unwrap();
    let writer_a = std::thread::spawn(move || first_stdin.write_all(b"left_marker :: 1\n:quit\n"));
    let writer_b = std::thread::spawn(move || second_stdin.write_all(b"right_marker :: 2\n:quit\n"));
    writer_a.join().unwrap().unwrap();
    writer_b.join().unwrap().unwrap();
    assert!(first.wait().unwrap().success());
    assert!(second.wait().unwrap().success());
    let merged = run_repl_process(
        &state,
        b":history search left_marker\n:history search right_marker\n:quit\n",
        None,
    );
    let merged = String::from_utf8_lossy(&merged.stdout);
    assert!(merged.contains("left_marker :: 1") && merged.contains("right_marker :: 2"), "lost update: {merged:?}");

    let mut stale = spawn_repl_process(&state);
    wait_for_history_dir(&state);
    let cleared = run_repl_process(&state, b":history clear\n:quit\n", None);
    assert!(cleared.status.success());
    stale.stdin.as_mut().unwrap().write_all(b"fresh_marker :: 3\n:quit\n").unwrap();
    assert!(stale.wait().unwrap().success());
    let after = run_repl_process(
        &state,
        b":history search left_marker\n:history search right_marker\n:history search fresh_marker\n:quit\n",
        None,
    );
    let after = String::from_utf8_lossy(&after.stdout);
    assert!(after.contains("fresh_marker :: 3"), "fresh write missing: {after:?}");
    assert!(!after.contains("left_marker :: 1") && !after.contains("right_marker :: 2"), "clear resurrected history: {after:?}");
    std::fs::remove_dir_all(state).ok();
}

#[cfg(unix)]
#[test]
fn repl_raw_f3_search_recalls_and_submits_persistent_history() {
    use std::process::Command;
    let state = std::env::temp_dir().join(format!("jet_repl_history_f3_{}", std::process::id()));
    std::fs::remove_dir_all(&state).ok();
    assert!(run_repl_process(&state, b"3 + 3\n:quit\n", None).status.success());
    let shell = r#"
{
  sleep 0.2
  printf '\033OR'
  sleep 0.1
  printf '3 +\r'
  sleep 0.1
  printf '\r'
  sleep 0.2
  printf ':quit\r'
} | script -qec '"$JET_REPL_BIN" repl' /dev/null
"#;
    let output = Command::new("sh")
        .args(["-c", shell])
        .env("JET_REPL_BIN", env!("CARGO_BIN_EXE_jet"))
        .env("XDG_STATE_HOME", &state)
        .env_remove("JET_REPL_HISTORY")
        .env("NO_COLOR", "1")
        .output()
        .expect("run raw REPL under PTY");
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "PTY status: {:?}\n{out}", output.status);
    assert!(out.contains("history search>"), "F3 did not open search: {out:?}");
    assert!(out.contains("6 : Int"), "recalled submission did not execute: {out:?}");
    std::fs::remove_dir_all(state).ok();
}

#[cfg(unix)]
fn run_raw_multiline_pty(writer: &str) -> std::process::Output {
    use std::process::Command;
    let shell = format!(
        "{{ sleep 0.2; {writer}; sleep 0.2; printf ':quit\\r'; }} | \
         timeout 8s script -qec '\"$JET_REPL_BIN\" repl' /dev/null"
    );
    Command::new("sh")
        .args(["-c", &shell])
        .env("JET_REPL_BIN", env!("CARGO_BIN_EXE_jet"))
        .env("JET_REPL_HISTORY", "off")
        .env("NO_COLOR", "1")
        .output()
        .expect("run raw multiline REPL under PTY")
}

#[cfg(unix)]
#[test]
fn repl_raw_enter_uses_parser_completeness_not_bracket_balance() {
    let output = run_raw_multiline_pty(
        "printf '40 +\\r'; sleep 0.15; printf '2\\r'",
    );
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "PTY status: {:?}\n{out}", output.status);
    assert!(out.contains("· "), "continuation prompt missing: {out:?}");
    assert!(out.contains("42 : Int"), "parser-complete input did not submit: {out:?}");
}

#[cfg(unix)]
#[test]
fn repl_raw_escape_enter_forces_newline_then_blank_submits() {
    let output = run_raw_multiline_pty(
        "printf '40 + 2\\033\\r'; sleep 0.15; printf '\\r'",
    );
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "PTY status: {:?}\n{out}", output.status);
    assert!(out.contains("· "), "forced newline did not redraw continuation: {out:?}");
    assert!(out.contains("42 : Int"), "blank continuation did not submit: {out:?}");
}

#[cfg(unix)]
#[test]
fn repl_raw_blank_continuation_force_submits_incomplete_input() {
    let output = run_raw_multiline_pty(
        "printf '1 +\\r'; sleep 0.15; printf '\\r'",
    );
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "PTY status: {:?}\n{out}", output.status);
    assert!(out.contains("E0003"), "blank continuation kept waiting: {out:?}");
}

#[cfg(unix)]
#[test]
fn repl_raw_multiline_redraw_keeps_second_line_editable() {
    let output = run_raw_multiline_pty(
        "printf '10 +\\r'; sleep 0.15; printf '3\\1772\\r'",
    );
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "PTY status: {:?}\n{out}", output.status);
    assert!(out.contains("· 2"), "continuation redraw lost edited line: {out:?}");
    assert!(out.contains("12 : Int"), "edited multiline input was wrong: {out:?}");
}

#[cfg(unix)]
#[test]
fn repl_raw_narrow_soft_wrap_keeps_cursor_delete_and_redraw_aligned() {
    use std::process::Command;
    let shell = r#"
{
  sleep 0.2
  printf '10 + 20 + 30 + 49'
  sleep 0.15
  printf '\033[D\033[3~\r'
  sleep 0.2
  printf ':quit\r'
} | timeout 8s script -qec 'stty cols 20; "$JET_REPL_BIN" repl' /dev/null
"#;
    let output = Command::new("sh")
        .args(["-c", shell])
        .env("JET_REPL_BIN", env!("CARGO_BIN_EXE_jet"))
        .env("JET_REPL_HISTORY", "off")
        .env("NO_COLOR", "1")
        .output()
        .expect("run narrow raw REPL under PTY");
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "PTY status: {:?}\n{out}", output.status);
    assert!(out.contains("\x1b[1A"), "soft-wrap redraw never moved to prior physical row: {out:?}");
    assert!(out.contains("64 : Int"), "cursor/delete changed wrapped input incorrectly: {out:?}");
}

#[test]
fn repl_cooked_keeps_bracket_balance_continuation() {
    use std::io::Write as _;
    use std::process::{Command, Stdio};
    let mut child = Command::new(env!("CARGO_BIN_EXE_jet"))
        .arg("repl")
        .env("JET_REPL_HISTORY", "off")
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"[1,\n2]\n:quit\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "cooked status: {:?}\n{out}", output.status);
    assert!(out.contains("...  "), "cooked continuation prompt missing: {out:?}");
    assert!(out.contains("[1, 2]"), "balanced cooked input failed: {out:?}");
}

/// Parse a transcript file: return `(inputs, expected_outputs)`.
/// Lines `> input` become inputs; non-comment, non-`>` lines become expected.
/// Lines starting with `# ` are comments. Blank expected-output lines are
/// represented as blank strings (the transcript may expect a blank output line).
fn parse_transcript(txt: &str) -> (Vec<String>, Vec<String>) {
    let mut inputs: Vec<String> = Vec::new();
    let mut expected: Vec<String> = Vec::new();
    for line in txt.lines() {
        if line.starts_with("# ") || line == "#" {
            // comment — skip
        } else if let Some(input) = line.strip_prefix("> ") {
            inputs.push(input.to_string());
        } else {
            // expected output line (may be blank)
            expected.push(line.to_string());
        }
    }
    (inputs, expected)
}

fn run_transcript_file(txt: &str) {
    run_transcript_file_with_trailing_policy(txt, true);
}

fn run_transcript_file_strict(txt: &str) {
    run_transcript_file_with_trailing_policy(txt, false);
}

fn run_transcript_file_with_trailing_policy(txt: &str, allow_trailing: bool) {
    let (inputs, expected_lines) = parse_transcript(txt);
    let input_refs: Vec<&str> = inputs.iter().map(String::as_str).collect();
    let actual = run_transcript(&input_refs, None);

    // Split actual output into lines (preserve blank lines).
    let actual_lines: Vec<&str> = actual.lines().collect();

    // Compare line by line; skip blank expected lines that represent
    // "no output" comments in the transcript.
    let mut ai = 0; // index into actual_lines
    for (i, exp) in expected_lines.iter().enumerate() {
        if exp.is_empty() {
            // blank expected line — skip (these are transcript spacing)
            continue;
        }
        let got = actual_lines.get(ai).copied().unwrap_or("<no output>");
        assert_eq!(
            got,
            exp.as_str(),
            "transcript line {}: expected {:?} but got {:?}\nfull actual output:\n{}",
            i + 1,
            exp,
            got,
            actual
        );
        ai += 1;
    }
    if !allow_trailing {
        assert_eq!(
            ai,
            actual_lines.len(),
            "unexpected trailing transcript output: {:?}\nfull actual output:\n{}",
            &actual_lines[ai..],
            actual
        );
    }
}

// ── test cases ────────────────────────────────────────────────────────────

#[test]
fn repl_arithmetic_echo() {
    let out = run_transcript(&["1 + 2"], None);
    assert_eq!(out.trim(), "3 : Int");
}

#[test]
fn repl_rejects_user_dunder() {
    let out = run_transcript(&["__user :: 1"], None);
    assert!(
        out.contains("E0067") && out.contains("reserved for Jet"),
        "got: {out:?}"
    );
}

#[test]
fn repl_val_binding_then_expr() {
    let inputs = &["x :: 10", "x * 2"];
    let out = run_transcript(inputs, None);
    // First input produces no output (val declaration).
    // Second produces the echo.
    assert!(out.trim().ends_with("20 : Int"), "got: {:?}", out);
}

#[test]
fn repl_print_output() {
    let out = run_transcript(&["print(\"hello, world\")"], None);
    assert!(out.contains("hello, world"), "got: {:?}", out);
}

#[test]
fn repl_string_echo() {
    let out = run_transcript(&["\"abc\""], None);
    assert!(out.contains("\"abc\" : String"), "got: {:?}", out);
}

#[test]
fn repl_bool_echo() {
    let out = run_transcript(&["2 > 1"], None);
    assert!(out.trim().ends_with("true : Bool"), "got: {:?}", out);
}

#[test]
fn repl_suppress_semicolon() {
    // A binding declaration produces no echo.
    let out = run_transcript(&["_ignored :: 42"], None);
    assert!(out.trim().is_empty(), "expected no output, got: {:?}", out);
}

#[test]
fn repl_reset_clears_bindings() {
    let inputs = &["x :: 10", ":reset", ":type x"];
    let out = run_transcript(inputs, None);
    assert!(out.contains("session reset"), "got: {:?}", out);
    assert!(out.contains("isn't defined"), "got: {:?}", out);
}

#[test]
fn repl_function_declare_and_call() {
    let inputs = &["fn double(n: Int) -> Int { return n * 2 }", "double(5)"];
    let out = run_transcript(inputs, None);
    assert!(
        out.contains("ok"),
        "no ok for fn declaration, got: {:?}",
        out
    );
    assert!(
        out.contains("10 : Int"),
        "fn call result wrong, got: {:?}",
        out
    );
}

#[test]
fn repl_hard_reject_unsafe() {
    let out = run_transcript(&["#Unsafe { }"], None);
    assert!(
        out.contains("E1802"),
        "expected E1802 hard-reject, got: {:?}",
        out
    );
}

#[test]
fn repl_hard_reject_extern_rust() {
    let out = run_transcript(&["extern rust \"mycrate\" { }"], None);
    assert!(
        out.contains("E1802"),
        "expected E1802 hard-reject, got: {:?}",
        out
    );
}

#[test]
fn repl_type_meta_command() {
    let inputs = &["y :: 42", ":type y"];
    let out = run_transcript(inputs, None);
    assert!(out.contains("y : Int"), "got: {:?}", out);
}

#[test]
fn repl_type_unknown() {
    let out = run_transcript(&[":type nosuchvar"], None);
    assert!(out.contains("isn't defined"), "got: {:?}", out);
}

#[test]
fn repl_quit_exits() {
    let out = run_transcript(&[":quit"], None);
    assert_eq!(out.trim(), "bye");
}

#[test]
fn repl_help_shows_commands() {
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    // Merge the child's stderr into stdout before launch. The discovery hint
    // intentionally uses stderr so the historical stdout banner stays stable;
    // one pipe lets this regression verify the order users actually observe.
    let mut child = Command::new("sh")
        .args(["-c", "exec \"$JET_REPL_BIN\" repl 2>&1"])
        .env("JET_REPL_BIN", env!("CARGO_BIN_EXE_jet"))
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("start jet repl");
    child
        .stdin
        .as_mut()
        .expect("piped stdin")
        .write_all(b":help\n:quit\n")
        .expect("write transcript");
    let output = child.wait_with_output().expect("finish jet repl");
    assert!(output.status.success(), "status: {:?}", output.status);
    let out = String::from_utf8(output.stdout).expect("utf-8 help");
    let banner = out.find("Jet 1.0.0 — interactive REPL").expect("banner");
    let cooked_hint = out
        .find("Try: ?name docs · :pin/:fold/:rerun <id> · interactive keys require a TTY")
        .expect("cooked discovery hint from stderr");
    assert!(banner < cooked_hint, "banner must precede hint: {out:?}");
    assert!(out.contains("REPL meta-commands"), "got: {:?}", out);
    for hint in [
        "?name",
        ":? <name>",
        "Interactive terminal only",
        "Tab",
        "^P",
        "^F",
        "^R",
        "^B",
    ] {
        assert!(out.contains(hint), "help missing {hint:?}: {out:?}");
    }
}

#[test]
fn repl_notebook_turn_controls() {
    let out = run_transcript(
        &[
            "1 + 2",
            ":turns",
            ":pin 1",
            ":fold 1",
            ":turns",
            ":unfold 1",
            ":unpin 1",
            ":rerun 1",
        ],
        None,
    );
    assert!(out.contains("#1 ok"), "got: {out:?}");
    assert!(out.contains("turn pinned"), "got: {out:?}");
    assert!(out.contains("turn folded"), "got: {out:?}");
    assert!(out.contains("#1 ok pinned folded"), "got: {out:?}");
    assert!(out.contains("rerun #1: 1 + 2"), "got: {out:?}");
    assert!(out.matches("3 : Int").count() >= 2, "got: {out:?}");
}

#[test]
fn repl_question_name_shows_type() {
    let out = run_transcript(&["answer :: 42", ":? answer"], None);
    assert!(out.contains("answer: Int :: 42"), "got: {out:?}");
}

#[test]
fn repl_strips_terminal_control_bytes() {
    let out = run_transcript(&["\x0e1 + 2"], None);
    assert_eq!(out.trim(), "3 : Int");
    assert!(
        !out.contains('\x0e'),
        "control byte leaked into transcript: {:?}",
        out
    );
}

#[test]
fn repl_load_hello() {
    // Load examples/features/basics/hello.jet and check it runs.
    let out = run_transcript(&[":load examples/features/basics/hello.jet"], None);
    assert!(
        out.contains("hello, world"),
        "load should run main, got: {:?}",
        out
    );
}

#[test]
fn repl_basics_transcript() {
    let txt = include_str!("repl/basics.txt");
    run_transcript_file(txt);
}

#[test]
fn repl_bigint_exact_transcript() {
    run_transcript_file_strict(include_str!("repl/bigint.txt"));
}

#[test]
fn repl_f32_exact_transcript() {
    run_transcript_file_strict(include_str!("repl/f32.txt"));
}

#[test]
fn repl_zoned_exact_transcript() {
    run_transcript_file_strict(include_str!("repl/zoned.txt"));
}

#[test]
fn repl_decimal_exact_transcript() {
    run_transcript_file_strict(include_str!("repl/decimal.txt"));
}

#[test]
fn repl_string_from_bytes_exact_transcript() {
    run_transcript_file_strict(include_str!("repl/string_from_bytes.txt"));
}

#[test]
fn repl_sketch_exact_transcript() {
    run_transcript_file_strict(include_str!("repl/sketch.txt"));
}

#[test]
fn repl_archive_exact_transcript() {
    run_transcript_file_strict(include_str!("repl/archive.txt"));
}

#[test]
fn repl_compress_exact_transcript() {
    run_transcript_file_strict(include_str!("repl/compress.txt"));
}

#[test]
fn repl_pool_exact_transcript() {
    run_transcript_file_strict(include_str!("repl/pool.txt"));
}

#[test]
fn repl_ui_values_exact_transcript() {
    run_transcript_file_strict(include_str!("repl/ui_values.txt"));
}

#[test]
fn repl_net_style_values_exact_transcript() {
    run_transcript_file_strict(include_str!("repl/net_style_values.txt"));
}

#[test]
fn repl_net_runtime_call_still_requires_authority() {
    let out = run_transcript(
        &["use core.net as net", "net.tcp_connect(\"127.0.0.1:1\")"],
        None,
    );
    assert!(out.contains("ok"), "core.net import should be accepted: {out}");
    assert!(out.contains("E1803"), "live socket call must require authority: {out}");
}

#[test]
fn strict_transcript_rejects_unexpected_trailing_output() {
    assert!(
        std::panic::catch_unwind(|| run_transcript_file_strict("> 1 + 1")).is_err(),
        "strict transcripts must fail when an input emits an unasserted line"
    );
}

// ── D-REPL8=A: move semantics across inputs ───────────────────────────────

#[test]
fn repl_move_across_inputs_string() {
    // `t :: s` moves s (String is non-scalar). A later reference to s must error.
    let inputs = &["s :: \"hello\"", "t :: s", "s"];
    let out = run_transcript(inputs, None);
    // After s is moved into t, using s in input 3 must produce an error.
    // The error will be E0121 ("was given away") because the binding_stubs_src
    // emits a synthetic consume of s before the check.
    assert!(
        out.contains("error")
            && (out.contains("E0121")
                || out.contains("given away")
                || out.contains("nothing named")),
        "expected use-after-move error, got: {:?}",
        out
    );
    // Must NOT silently succeed (no echo of the moved value).
    assert!(
        !out.trim_end().ends_with(": String"),
        "moved value should not be echoed, got: {:?}",
        out
    );
}

#[test]
fn repl_move_within_single_input_still_errors() {
    // Move within a single input: x is moved in the same step it's used again.
    let inputs = &["x :: \"hi\"", "y :: x; x"];
    let out = run_transcript(inputs, None);
    // Within the same input, sema catches the use after move.
    assert!(
        out.contains("error"),
        "expected within-input move error, got: {:?}",
        out
    );
}

#[test]
fn repl_move_int_not_moved() {
    // Int is a scalar — `t :: s` where s is an Int does NOT move s.
    let inputs = &["s :: 42", "t :: s", "s"];
    let out = run_transcript(inputs, None);
    // s should still be accessible since Int is copy.
    assert!(
        out.contains("42 : Int"),
        "Int should be accessible after copy, got: {:?}",
        out
    );
}

#[test]
fn repl_failed_multi_statement_turn_rolls_back_scope_and_moves() {
    let out = run_transcript(
        &["s :: \"hello\"", "t :: s; 1 / 0", ":type t", "s"],
        None,
    );
    assert!(out.contains("isn't defined"), "failed turn leaked `t`: {out}");
    assert!(
        out.contains("\"hello\" : String"),
        "failed turn consumed `s`: {out}"
    );
}

// ── D-REPL-FUEL=A: :run ──────────────────────────────────────────────────

#[test]
fn repl_run_empty_session() {
    // :run on an empty session is a clean no-op.
    let out = run_transcript(&[":run"], None);
    assert!(
        out.contains("empty") || out.contains("nothing"),
        "empty :run should note nothing to run, got: {:?}",
        out
    );
    // Must not produce an error code.
    assert!(
        !out.contains("error [E"),
        "empty :run must not error, got: {:?}",
        out
    );
}

#[test]
fn repl_run_produces_output() {
    // :run on a session with a print should produce that output.
    let inputs = &["print(\"hello from run\")", ":run"];
    let out = run_transcript(inputs, None);
    assert!(
        out.contains("hello from run"),
        ":run should replay print output, got: {:?}",
        out
    );
}

#[test]
fn repl_run_consistency_with_interpreter() {
    // The output of :run must match the interpreter's direct output.
    // First run with interpreter: print("ping") produces "ping".
    let direct_out = run_transcript(&["print(\"ping\")"], None);
    // Now run the same with :run.
    let run_out = run_transcript(&["print(\"ping\")", ":run"], None);
    // :run output must contain the same output as the interpreter.
    assert!(
        run_out.contains("ping"),
        ":run output must contain interpreter output; got: {:?}",
        run_out
    );
    // Direct interpreter also produced "ping".
    assert!(direct_out.contains("ping"), "got: {:?}", direct_out);
}

// Probe test — not a real test, only used during dev to see exact output.
// Kept for reference but skipped in CI.
#[test]
#[ignore]
fn repl_probe_exact_outputs() {
    use jet::REPL::run_transcript;

    // Move: String binding s moved into t, then use of s
    let out = run_transcript(&["s :: \"hello\"", "t :: s", "s"], None);
    eprintln!("MOVE_STRING: {:?}", out);

    // Move: Int binding s copied into t, then use of s
    let out = run_transcript(&["s :: 42", "t :: s", "s"], None);
    eprintln!("COPY_INT: {:?}", out);

    // :run empty
    let out = run_transcript(&[":run"], None);
    eprintln!("RUN_EMPTY: {:?}", out);

    // :run with print
    let out = run_transcript(&["print(\"hello from run\")", ":run"], None);
    eprintln!("RUN_PRINT: {:?}", out);

    // --project probe
    let fixture = std::env::temp_dir().join("jet_repl_project_probe");
    std::fs::create_dir_all(&fixture).ok();
    std::fs::write(
        fixture.join("helper.jet"),
        "fn add_three(x: Int) -> Int { return x + 3; }\n",
    )
    .ok();
    let project_dir = fixture.to_string_lossy().to_string();
    let out = run_transcript(&["add_three(10)"], Some(&project_dir));
    eprintln!("PROJECT_ADD_THREE: {:?}", out);
    std::fs::remove_dir_all(&fixture).ok();
}

// ── D-REPL10=A: --project mode ───────────────────────────────────────────

#[test]
fn repl_project_loads_items() {
    // Create a tiny fixture project with a function.
    let fixture = std::env::temp_dir().join("jet_repl_project_test");
    std::fs::create_dir_all(&fixture).ok();
    std::fs::write(
        fixture.join("helper.jet"),
        "fn add_three(x: Int) -> Int { return x + 3; }\n",
    )
    .expect("write fixture");

    let project_dir = fixture.to_string_lossy().to_string();
    // With --project, add_three() is available without `use`.
    let out = run_transcript(&["add_three(10)"], Some(&project_dir));
    // Must not report unknown function error.
    assert!(
        !out.contains("E0102") && !out.contains("error [E0107]"),
        "--project should make project functions available, got: {:?}",
        out
    );
    // Must produce the correct result.
    assert!(
        out.contains("13 : Int"),
        "--project: add_three(10) should return 13, got: {:?}",
        out
    );

    // Without --project, add_three() is unknown.
    let out_no_project = run_transcript(&["add_three(10)"], None);
    assert!(
        out_no_project.contains("error"),
        "without --project, unknown fn should error, got: {:?}",
        out_no_project
    );

    // Clean up.
    std::fs::remove_dir_all(&fixture).ok();
}

// ── S16/D-REPL10: `use` imports carried across REPL inputs ────────────────

#[test]
fn repl_use_import_accepted() {
    // A bare `use core.math as math` import is accepted (not "expected a
    // statement, found `use`"), and reports `ok`.
    let out = run_transcript(&["use core.math as math"], None);
    assert!(
        out.contains("ok"),
        "import should be accepted, got: {:?}",
        out
    );
    assert!(
        !out.contains("E0003"),
        "import must not be a statement error, got: {:?}",
        out
    );
}

#[test]
fn repl_use_import_resolves_alias_in_later_input() {
    // The import typed in input 1 must make `math` resolve in input 2 — the
    // alias is carried across inputs (this is the c55 cross-input delta). Before
    // the fix this produced E0107 ("nothing named `math`").
    let out = run_transcript(&["use core.math as math", "r :: math.sqrt(16.0)"], None);
    assert!(
        !out.contains("E0107") && !out.contains("nothing named `math`"),
        "carried import should resolve the alias, got: {:?}",
        out
    );
}

#[test]
fn repl_use_import_persists_across_unrelated_input() {
    // An intervening unrelated input must not drop the import: the alias still
    // resolves two inputs later.
    let out = run_transcript(
        &["use core.math as math", "x :: 1", "r :: math.sqrt(9.0)"],
        None,
    );
    assert!(
        !out.contains("E0107") && !out.contains("nothing named `math`"),
        "import should persist across inputs, got: {:?}",
        out
    );
}

#[test]
fn repl_use_unknown_core_module_rejected() {
    // An unknown core module reports E1001 and is not retained.
    let out = run_transcript(&["use core.bogus as b"], None);
    assert!(
        out.contains("E1001"),
        "unknown core module should error, got: {:?}",
        out
    );
    assert!(
        !out.contains("ok"),
        "bad import must not be accepted, got: {:?}",
        out
    );
}

#[test]
fn repl_use_repl_incompatible_module_hard_rejected() {
    // Native-only modules (HTTP, DB, …) hard-reject with E1802 before import.
    let out = run_transcript(&["use core.http.client as http"], None);
    assert!(
        out.contains("E1802"),
        "core.http.client should hard-reject, got: {:?}",
        out
    );
}

#[test]
fn repl_infinite_loop_hits_e1801_fuel_cap() {
    let out = run_transcript(&["loop { }"], None);
    assert!(out.contains("Error [E1801]:"), "missing REPL fuel diagnostic:\n{out}");
    assert!(out.contains("Why:"), "missing E1801 reason:\n{out}");
    assert!(out.contains("Fix:"), "missing E1801 fix:\n{out}");
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/cli/repl_e1801.txt");
    if std::env::var_os("UPDATE_EXPECT").is_some() {
        std::fs::write(&path, &out).unwrap();
    }
    assert_eq!(out, std::fs::read_to_string(path).unwrap());
}

#[test]
fn repl_use_core_fs_import_accepted() {
    let out = run_transcript(&["use core.files as fs"], None);
    assert!(
        out.contains("ok"),
        "core.files import should work, got: {:?}",
        out
    );
    assert!(!out.contains("E1802"), "got: {:?}", out);
}

#[test]
fn repl_reset_clears_imports() {
    // After :reset, a carried import is gone — the alias no longer resolves.
    let out = run_transcript(
        &["use core.math as math", ":reset", "r :: math.sqrt(4.0)"],
        None,
    );
    assert!(out.contains("session reset"), "got: {:?}", out);
    assert!(
        out.contains("error"),
        "after reset the alias must not resolve, got: {:?}",
        out
    );
}

// ── D-CTCORE1: inline pure Core execution in the REPL ────────────────────

#[test]
fn repl_core_math_sqrt_inline() {
    // c133/D-CTCORE1: after importing core.math, math.sqrt() must execute
    // inline in the interpreter (not raise E0956).
    let inputs = &["use core.math as math", "math.sqrt(16.0)"];
    let out = run_transcript(inputs, None);
    assert!(
        out.contains("ok") && out.contains("4") && !out.contains("E0956"),
        "math.sqrt(16.0) should produce 4 inline, got: {:?}",
        out
    );
}

#[test]
fn repl_core_math_multiple_calls() {
    // Confirm several whitelisted math calls work sequentially in one session.
    let inputs = &[
        "use core.math as math",
        "math.floor(3.7)",
        "math.ceil(2.1)",
        "math.abs(-5)",
    ];
    let out = run_transcript(inputs, None);
    assert!(out.contains("3"), "floor(3.7) should be 3, got: {:?}", out);
    assert!(out.contains("3"), "ceil(2.1) should be 3, got: {:?}", out);
    assert!(out.contains("5"), "abs(-5) should be 5, got: {:?}", out);
    assert!(
        !out.contains("E0956"),
        "no E0956 for whitelisted calls, got: {:?}",
        out
    );
}

#[test]
fn repl_core_math_pow_inline() {
    // Another whitelisted math function: math.pow(2.0, 10.0) = 1024.
    let inputs = &["use core.math as math", "math.pow(2.0, 10.0)"];
    let out = run_transcript(inputs, None);
    assert!(
        out.contains("1024") && !out.contains("E0956"),
        "math.pow(2.0, 10.0) should produce 1024 inline, got: {:?}",
        out
    );
}

#[test]
fn repl_core_result_stored_in_binding() {
    // The result of a whitelisted core call can be stored and used.
    let inputs = &["use core.math as math", "r :: math.sqrt(9.0)", "r"];
    let out = run_transcript(inputs, None);
    assert!(
        out.contains("3") && !out.contains("E0956"),
        "binding from math.sqrt should work, got: {:?}",
        out
    );
}

#[test]
fn repl_core_loadable_constructors_and_methods_exact() {
    let out = run_transcript(
        &[
            "use core.reactive.loadable as loadable",
            "loadable.idle()",
            "loadable.loading()",
            "loadable.loaded(7)",
            "loadable.failed(\"offline\")",
            "loadable.loading().loaded()",
            "loadable.idle().is_idle()",
            "loadable.loading().is_loading()",
            "ready :: loadable.loaded(7)",
            "ready.is_loaded()",
            "offline :: loadable.failed(\"offline\")",
            "offline.is_failed()",
            "ready.loaded() ?? 0",
            "ready.or_else(0)",
        ],
        None,
    );
    assert_eq!(
        out.trim(),
        "ok\nIdle : Loadable\nLoading : Loadable\nLoaded(7) : Loadable\nFailed(offline) : Loadable\nnull : Option\ntrue : Bool\ntrue : Bool\ntrue : Bool\ntrue : Bool\n7 : Int\n7 : Int"
    );
}

#[test]
fn repl_complex_bindings_keep_exact_typed_ast_across_turns() {
    let out = run_transcript(
        &[
            "struct Point { x: Int y: Int }",
            "p :: Point.{x: 3, y: 4}",
            "p.x + p.y",
            "inc :: (x: Int) => x + 1",
            "inc(4)",
            "words :: [\"jet\", \"repl\"]",
            "words[1]",
        ],
        None,
    );
    assert!(!out.contains("error ["), "complex state regressed: {out}");
    assert!(out.contains("7 : Int"), "struct value unavailable: {out}");
    assert!(out.contains("5 : Int"), "closure value unavailable: {out}");
    assert!(out.contains("\"repl\" : String"), "list element type collapsed: {out}");
}

#[test]
fn repl_all_complex_binding_shapes_survive_across_turns() {
    let out = run_transcript(
        &[
            "enum State { Ready(Int) }",
            "fn state_value(s: State) -> Int { if s == { .Ready(value) -> { return value } } return 0 }",
            "items: [String] :: [\"jet\", \"repl\"]",
            "items[0]",
            "counts: [String: Int] :: [\"jet\": 2]",
            "counts[\"jet\"]",
            "maybe: Int? :: Val(7)",
            "maybe ?? 0",
            "result: Int ? String :: Ok(9)",
            "result ?? 0",
            "state :: State.Ready(11)",
            "state_value(state)",
        ],
        None,
    );
    assert!(!out.contains("error ["), "typed state regressed: {out}");
    for expected in ["\"jet\" : String", "2 : Int", "7 : Int", "9 : Int", "11 : Int"] {
        assert!(out.contains(expected), "missing {expected:?}: {out}");
    }
}

#[test]
fn repl_declared_types_survive_empty_and_absent_values() {
    let out = run_transcript(
        &[
            "names: [String] :: []",
            "missing: String? :: None",
            ":type names",
            ":type missing",
            "names.len()",
            "missing ?? \"fallback\"",
        ],
        None,
    );
    assert!(!out.contains("error ["), "declared type state regressed: {out}");
    assert!(out.contains("names : [String]"), "empty list type collapsed: {out}");
    assert!(out.contains("missing : String?"), "None type collapsed: {out}");
    assert!(out.contains("0 : Int") && out.contains("\"fallback\" : String"), "values unusable: {out}");
}

#[test]
fn repl_core_io_eprint_inline() {
    let inputs = &["use core.io as io", "io.eprint(\"repl-err\")"];
    let out = run_transcript_with_flags(inputs, None, &[], &["io"]);
    assert!(
        out.contains("ok") && out.contains("repl-err"),
        "io.eprint should run inline, got: {:?}",
        out
    );
    assert!(
        !out.contains("E3410") && !out.contains("E0956"),
        "got: {:?}",
        out
    );
}

#[test]
fn repl_deny_rand_blocks_draw_and_mutating_shuffle() {
    let inputs = &[
        "use core.random as random",
        "xs := [1, 2, 3]",
        "#Grant(Rand) { caps -> random.int(1, 10) }",
        "#Grant(Rand) { caps -> random.shuffle(&xs) }",
        "xs",
    ];
    let out = run_transcript_with_flags(inputs, None, &["rand"], &["rand"]);
    assert!(
        out.matches("E1803").count() >= 2 && out.contains("Rand.Draw"),
        "ambient random calls escaped deny policy: {out}"
    );
    assert!(
        out.contains("[1, 2, 3]"),
        "denied shuffle mutated its binding: {out}"
    );
}

#[test]
fn repl_core_json_parse_inline() {
    let inputs = &[
        "use core.encoding.json as json",
        "json.to_string(json.parse(\"[42]\") ?? panic(\"bad\"))",
    ];
    let out = run_transcript(inputs, None);
    assert!(
        out.contains("42") && !out.contains("E0956"),
        "json.parse/to_string should work inline, got: {:?}",
        out
    );
}

#[test]
fn repl_core_path_join_inline() {
    let inputs = &["use core.path as path", "path.join(\"a\", \"b\")"];
    let out = run_transcript(inputs, None);
    assert!(
        out.contains("a/b") || out.contains("a\\b"),
        "path.join should work, got: {:?}",
        out
    );
}

#[test]
fn repl_core_regex_is_match_inline() {
    let inputs = &[
        "use core.regex as re",
        "re.is_match(\"\\\\d+\", \"order 42\") ?? panic(\"bad\")",
    ];
    let out = run_transcript(inputs, None);
    assert!(
        out.contains("true") && !out.contains("E0956"),
        "regex.is_match should work inline, got: {:?}",
        out
    );
}

#[test]
fn repl_core_fs_read_inline() {
    let root = std::env::temp_dir().join(format!(
        "jet_repl_fs_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).expect("create fixture root");
    std::fs::write(root.join("payload.txt"), "repl-fs-payload").expect("write fixture");
    let read_expr = "#Grant(Fs, Io) { caps -> io.eprint(fs.read(\"payload.txt\") ?? panic(\"read failed\")) }";
    let inputs = &["use core.files as fs", "use core.io as io", read_expr];
    let out = run_transcript_with_flags(inputs, root.to_str(), &["fs", "io"], &[]);
    std::fs::remove_dir_all(&root).ok();
    assert!(
        out.contains("repl-fs-payload") && !out.contains("E1802"),
        "fs.read should work in REPL, got: {:?}",
        out
    );
}

#[test]
fn repl_core_process_run_is_authorized_and_captured() {
    let inputs = &[
        "use core.process as process",
        "use core.io as io",
        "#Grant(Exec, Io) { caps -> io.eprint((process.run([\"sh\", \"-c\", \"read value || printf repl-process-ok\"]) ?? panic(\"run failed\")).output) }",
    ];
    let out = run_transcript_with_flags(inputs, None, &["exec"], &["io"]);
    assert!(
        out.contains("repl-process-ok") && !out.contains("E1803"),
        "authorized process.run should capture output, got: {out}"
    );
}

#[test]
fn repl_interrupt_signal_paths_cover_apple_and_bsd_unix() {
    let terminal = include_str!("../crates/jet-repl/src/Term.rs");
    let evaluation_path = terminal
        .split("pub struct EvaluationInterruptGuard")
        .nth(1)
        .and_then(|source| source.split("/// One decoded input event.").next())
        .expect("evaluation interrupt implementation");
    assert!(
        evaluation_path.contains("#[cfg(unix)]"),
        "raw evaluation interrupt must compile on POSIX Unix"
    );
    assert!(
        !evaluation_path.contains("target_os = \"linux\""),
        "raw evaluation interrupt must not exclude Apple or BSD"
    );

    let methods = include_str!("../crates/jet-comptime/src/Comptime/Methods/repl_process.rs");
    let forwarding_path = methods
        .split("static REPL_CHILD_GROUP")
        .nth(1)
        .and_then(|source| source.split("// ---------------------------------------------------------------------------").next())
        .expect("runtime child interrupt forwarding implementation");
    assert!(
        forwarding_path.contains("#[cfg(unix)]"),
        "runtime child forwarding must compile on POSIX Unix"
    );
    assert!(
        !forwarding_path.contains("target_os = \"linux\""),
        "runtime child forwarding must not exclude Apple or BSD"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn repl_tty_ctrl_c_reaches_child_group_and_restores_input() {
    use std::process::Command;

    let root = std::env::temp_dir().join(format!("jet_repl_interrupt_{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let shell = r#"
{
  sleep 0.2
  printf 'use core.process as process\r'
  sleep 0.2
  printf '#Grant(Exec) { caps -> process.run(["sh", "-c", "trap '\''printf done > child-exited.txt; exit 130'\'' INT; printf $$ > child.pid; while :; do :; done"]) ?? panic("run failed") }\r'
  sleep 0.8
  printf '\003'
  sleep 0.5
  printf '40 + 2\r'
  sleep 0.3
  printf ':quit\r'
} | timeout 8s script -qec '"$JET_REPL_BIN" repl --project "$JET_REPL_ROOT" --allow-exec' /dev/null
"#;
    let output = Command::new("sh")
        .args(["-c", shell])
        .env("JET_REPL_BIN", env!("CARGO_BIN_EXE_jet"))
        .env("JET_REPL_ROOT", &root)
        .output()
        .unwrap();
    let transcript = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "PTY interrupt session failed: {transcript}");
    assert!(transcript.contains("42 : Int"), "REPL did not resume after Ctrl-C: {transcript}");
    assert_eq!(std::fs::read_to_string(root.join("child-exited.txt")).unwrap(), "done");
    let pid = std::fs::read_to_string(root.join("child.pid")).unwrap();
    let alive = Command::new("kill")
        .args(["-0", pid.trim()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success();
    assert!(!alive, "interrupted REPL child {pid} leaked");
    std::fs::remove_dir_all(root).ok();
}

#[cfg(target_os = "linux")]
#[test]
fn repl_tty_ctrl_c_cancels_compute_atomically_and_records_turn() {
    use std::process::Command;

    let shell = r#"
{
  sleep 0.2
  printf 'kept :: 42\r'
  sleep 0.15
  printf 'partial :: 1\033\rloop { }\033\r\r'
  sleep 0.3
  printf '\003'
  sleep 0.15
  printf 'kept\rpartial\r:turns\r:quit\r'
} | timeout 6s script -qec '"$JET_REPL_BIN" repl' /dev/null
"#;
    let started = std::time::Instant::now();
    let output = Command::new("sh")
        .args(["-c", shell])
        .env("JET_REPL_BIN", env!("CARGO_BIN_EXE_jet"))
        .env("JET_REPL_HISTORY", "off")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let transcript = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "PTY compute interrupt failed: {transcript}");
    assert!(started.elapsed() < std::time::Duration::from_secs(3), "interrupt missed 100ms recovery path");
    assert!(transcript.contains("Interrupted. External effects already performed were not rolled back."), "interrupt notice missing: {transcript}");
    assert!(transcript.contains("42 : Int"), "prior binding lost: {transcript}");
    assert!(transcript.contains("E0107") && transcript.contains("partial"), "partial binding committed: {transcript}");
    assert!(transcript.contains("#2 interrupted"), "interrupted turn not addressable: {transcript}");
}

#[cfg(target_os = "linux")]
#[test]
fn repl_tty_compute_interrupt_returns_within_100ms() {
    use std::io::{Read as _, Write as _};
    use std::process::{Command, Stdio};
    use std::sync::mpsc;

    let mut child = Command::new("script")
        .args(["-qec", "\"$JET_REPL_BIN\" repl", "/dev/null"])
        .env("JET_REPL_BIN", env!("CARGO_BIN_EXE_jet"))
        .env("JET_REPL_HISTORY", "off")
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn timed raw REPL");
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut bytes = [0; 4096];
        while let Ok(count) = stdout.read(&mut bytes) {
            if count == 0 {
                break;
            }
            if tx.send(bytes[..count].to_vec()).is_err() {
                break;
            }
        }
    });

    let mut transcript = String::new();
    let startup_deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !transcript.contains("1 user>") && std::time::Instant::now() < startup_deadline {
        if let Ok(chunk) = rx.recv_timeout(std::time::Duration::from_millis(50)) {
            transcript.push_str(&String::from_utf8_lossy(&chunk));
        }
    }
    stdin.write_all(b"loop { }\r").unwrap();
    stdin.flush().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    let interrupted_at = std::time::Instant::now();
    stdin.write_all(b"\x03").unwrap();
    stdin.flush().unwrap();
    let mut latency = None;
    while interrupted_at.elapsed() <= std::time::Duration::from_millis(100) {
        if let Ok(chunk) = rx.recv_timeout(std::time::Duration::from_millis(5)) {
            transcript.push_str(&String::from_utf8_lossy(&chunk));
            if transcript.contains("Interrupted. External effects already performed were not rolled back.") {
                latency = Some(interrupted_at.elapsed());
                break;
            }
        }
    }
    if latency.is_some() {
        let _ = stdin.write_all(b":quit\r");
        let _ = child.wait();
    } else {
        let _ = child.kill();
        let _ = child.wait();
    }
    drop(rx);
    reader.join().ok();
    assert!(latency.is_some(), "prompt missed 100ms interrupt bound: {transcript}");
}

#[cfg(target_os = "linux")]
#[test]
fn repl_tty_ctrl_c_warns_while_blocking_child_stops() {
    use std::process::Command;

    let root = std::env::temp_dir().join(format!("jet_repl_interrupt_warning_{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let shell = r#"
{
  sleep 0.2
  printf 'use core.process as process\r'
  sleep 0.15
  printf '#Grant(Exec) { caps -> process.run(["sh", "-c", "trap '\''sleep 0.4; exit 130'\'' INT; while :; do :; done"]) ?? panic("run failed") }\r'
  sleep 0.6
  printf '\003'
  sleep 0.8
  printf ':quit\r'
} | timeout 6s script -qec '"$JET_REPL_BIN" repl --project "$JET_REPL_ROOT" --allow-exec' /dev/null
"#;
    let output = Command::new("sh")
        .args(["-c", shell])
        .env("JET_REPL_BIN", env!("CARGO_BIN_EXE_jet"))
        .env("JET_REPL_ROOT", &root)
        .env("JET_REPL_HISTORY", "off")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let transcript = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "PTY blocking interrupt failed: {transcript}");
    assert!(transcript.contains("waiting for active external I/O to stop"), "blocking warning missing: {transcript}");
    std::fs::remove_dir_all(root).ok();
}

#[cfg(target_os = "linux")]
#[test]
fn repl_tty_second_ctrl_c_during_turn_exits_session() {
    use std::process::Command;

    let shell = r#"
{
  sleep 0.2
  printf 'loop { }\r'
  sleep 0.3
  printf '\003'
  sleep 0.03
  printf '\003'
  sleep 0.3
  printf '40 + 2\r'
} | timeout 4s script -qec '"$JET_REPL_BIN" repl' /dev/null
"#;
    let output = Command::new("sh")
        .args(["-c", shell])
        .env("JET_REPL_BIN", env!("CARGO_BIN_EXE_jet"))
        .env("JET_REPL_HISTORY", "off")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let transcript = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "second Ctrl-C did not exit: {transcript}");
    assert!(!transcript.contains("42 : Int"), "REPL accepted input after double interrupt: {transcript}");
}

#[cfg(target_os = "linux")]
#[test]
fn repl_raw_completion_menu_selects_shared_symbol() {
    use std::process::Command;

    let shell = r#"
{
  sleep 0.2
  printf 'alpha :: 1\r'
  sleep 0.1
  printf 'alpine :: 2\r'
  sleep 0.1
  printf 'al\t\033[B\r\r'
  sleep 0.2
  printf ':quit\r'
} | timeout 5s script -qec '"$JET_REPL_BIN" repl' /dev/null
"#;
    let output = Command::new("sh")
        .args(["-c", shell])
        .env("JET_REPL_BIN", env!("CARGO_BIN_EXE_jet"))
        .env("JET_REPL_HISTORY", "off")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let transcript = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "completion PTY failed: {transcript}");
    assert!(transcript.contains("completion —"), "selection menu missing: {transcript}");
    assert!(transcript.contains("> alpha") && transcript.contains("> alpine"), "selection did not move: {transcript}");
    assert!(transcript.contains("2 : Int"), "selected symbol was not inserted: {transcript}");
    assert!(!transcript.contains("\x1b[7m"), "NO_COLOR completion used color: {transcript:?}");
}

#[test]
fn repl_non_tty_denies_ungranted_files_before_execution() {
    let root = std::env::temp_dir().join(format!("jet_repl_deny_{}", std::process::id()));
    std::fs::create_dir_all(&root).expect("create root");
    let target = root.join("must-not-exist.txt");
    let input = "#Grant(Fs) { caps -> fs.write(\"must-not-exist.txt\", \"bad\") ?? panic(\"write failed\") }";
    let out = run_transcript(&["use core.files as fs", input], root.to_str());
    assert!(out.contains("E1803") && out.contains("Fs.Write"), "missing deterministic deny: {out}");
    assert!(!target.exists(), "denied effect executed");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn repl_allow_and_deny_flags_work_in_transcript_mode() {
    let root = std::env::temp_dir().join(format!("jet_repl_flags_{}", std::process::id()));
    std::fs::create_dir_all(&root).expect("create root");
    let input = "#Grant(Fs) { caps -> fs.write(\"flag.txt\", \"allowed\") ?? panic(\"write failed\") }";
    let allowed = run_transcript_with_flags(
        &["use core.files as fs", input],
        root.to_str(),
        &["fs"],
        &[],
    );
    assert!(!allowed.contains("E1803"), "--allow-fs rejected: {allowed}");
    assert_eq!(std::fs::read_to_string(root.join("flag.txt")).unwrap(), "allowed");
    std::fs::remove_file(root.join("flag.txt")).unwrap();

    let denied = run_transcript_with_flags(
        &["use core.files as fs", input],
        root.to_str(),
        &["fs"],
        &["fs"],
    );
    assert!(denied.contains("E1803"), "--deny-fs must override allow: {denied}");
    assert!(!root.join("flag.txt").exists(), "explicitly denied effect executed");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn repl_cli_allow_and_deny_flags_control_non_tty_execution() {
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    let root = std::env::temp_dir().join(format!("jet_repl_cli_flags_{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let input = b"use core.files as fs\n#Grant(Fs) { caps -> fs.write(\"cli.txt\", \"yes\") ?? panic(\"write failed\") }\n:quit\n";
    let run = |extra: &[&str]| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_jet"));
        cmd.arg("repl").arg("--project").arg(&root).args(extra)
            .env("NO_COLOR", "1")
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn().unwrap();
        child.stdin.as_mut().unwrap().write_all(input).unwrap();
        child.wait_with_output().unwrap()
    };

    let allowed = run(&["--allow-fs"]);
    assert!(allowed.status.success(), "allow status: {:?}", allowed.status);
    assert_eq!(std::fs::read_to_string(root.join("cli.txt")).unwrap(), "yes");
    std::fs::remove_file(root.join("cli.txt")).unwrap();

    let denied = run(&["--allow-fs", "--deny-fs"]);
    let stderr = String::from_utf8_lossy(&denied.stderr);
    assert!(stderr.contains("E1803"), "missing deny diagnostic: {stderr}");
    assert!(!root.join("cli.txt").exists(), "CLI-denied write executed");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn repl_effect_needs_lexical_grant_even_with_allow_flag() {
    let root = std::env::temp_dir().join(format!("jet_repl_missing_grant_{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let out = run_transcript_with_flags(
        &["use core.files as fs", "fs.write(\"no.txt\", \"bad\") ?? panic(\"write failed\")"],
        root.to_str(),
        &["fs"],
        &[],
    );
    assert!(out.contains("E1803") && out.contains("no REPL runtime authority"), "missing lexical denial: {out}");
    assert!(!root.join("no.txt").exists(), "operation without #Grant executed");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn repl_allow_fs_still_rejects_paths_outside_project_root() {
    let root = std::env::temp_dir().join(format!("jet_repl_confined_{}", std::process::id()));
    let outside = std::env::temp_dir().join(format!("jet_repl_escape_{}.txt", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::remove_file(&outside).ok();
    let escaped = outside.to_string_lossy().replace('\\', "\\\\");
    let input = format!(
        "#Grant(Fs) {{ caps -> fs.write(\"{escaped}\", \"bad\") ?? panic(\"write failed\") }}"
    );
    let out = run_transcript_with_flags(
        &["use core.files as fs", &input],
        root.to_str(),
        &["fs"],
        &[],
    );
    assert!(
        out.contains("E1803") && out.contains("Fs.Write") && out.contains(&escaped),
        "absolute escape was not rejected before execution: {out}"
    );
    assert!(!outside.exists(), "confined REPL wrote outside project root");
    std::fs::remove_dir_all(root).ok();
}

#[cfg(unix)]
#[test]
fn repl_allow_fs_rejects_symlink_components_before_open() {
    use std::os::unix::fs::symlink;

    let root = std::env::temp_dir().join(format!("jet_repl_symlink_root_{}", std::process::id()));
    let outside = std::env::temp_dir().join(format!("jet_repl_symlink_out_{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    symlink(&outside, root.join("escape")).unwrap();
    let input = "#Grant(Fs) { caps -> fs.write(\"escape/must-not-exist.txt\", \"bad\") ?? panic(\"write failed\") }";
    let out = run_transcript_with_flags(
        &["use core.files as fs", input],
        root.to_str(),
        &["fs"],
        &[],
    );
    assert!(out.contains("E1803"), "symlink traversal was not denied: {out}");
    assert!(!outside.join("must-not-exist.txt").exists(), "symlink escape executed");
    std::fs::remove_file(root.join("escape")).ok();
    std::fs::remove_dir_all(root).ok();
    std::fs::remove_dir_all(outside).ok();
}

#[test]
fn repl_tty_prompts_and_reuses_exact_session_tuple() {
    use std::process::Command;

    let root = std::env::temp_dir().join(format!("jet_repl_tty_auth_{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("value.txt"), "ok").unwrap();
    let shell = r#"
{
  sleep 0.2
  printf 'use core.files as fs\r'
  sleep 0.2
  printf '#Grant(Fs) { caps -> fs.read("value.txt") ?? panic("read failed") }\r'
  sleep 0.2
  printf 's'
  sleep 0.2
  printf '#Grant(Fs) { caps -> fs.read("value.txt") ?? panic("read failed") }\r'
  sleep 0.2
  printf 'c'
  sleep 0.2
  printf ':quit\r'
} | script -qec '"$JET_REPL_BIN" repl --project "$JET_REPL_ROOT"' /dev/null
"#;
    let output = Command::new("sh")
        .args(["-c", shell])
        .env("JET_REPL_BIN", env!("CARGO_BIN_EXE_jet"))
        .env("JET_REPL_ROOT", &root)
        .env("NO_COLOR", "1")
        .output()
        .expect("run REPL under PTY");
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "PTY status: {:?}\n{out}", output.status);
    assert!(out.contains("Core effect Fs requests runtime authority"), "missing prompt: {out}");
    assert!(out.contains("Using session Fs.Read authority for `value.txt`"), "missing exact tuple reuse: {out}");
    assert!(!out.contains("E1803"), "approved tuple denied: {out}");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn repl_http_client_import_hard_rejected() {
    let out = run_transcript(&["use core.http.client as client"], None);
    assert!(
        out.contains("E1802"),
        "HTTP client import should hard-reject, got: {:?}",
        out
    );
}

#[test]
fn repl_mut_binding_allows_reassign() {
    // D-BIND4: `:=` bindings must stay mutable across REPL inputs for sema.
    let out = run_transcript(&["hi := 2", "hi = hi * 2", "hi"], None);
    assert!(
        !out.contains("E0111") && !out.contains("made with"),
        "mutable REPL binding should allow `=`, got: {:?}",
        out
    );
    assert!(out.contains("4"), "hi * 2 should be 4, got: {:?}", out);
}

#[test]
fn repl_immut_binding_rejects_reassign() {
    let out = run_transcript(&["hi :: 2", "hi = hi * 2"], None);
    assert!(
        out.contains("E0111"),
        "immutable `::` binding should reject `=`, got: {:?}",
        out
    );
}

// ── D-FE-REPL1=D: hybrid REPL — turn gutter, fold, pin rail, docs, rerun ───
//
// The raw-mode TTY event loop (`crates/jet-repl/src/Interactive.rs`) isn't
// exercised here (no real pty in the test harness — see
// `crates/jet-repl/src/Term.rs` for the raw-mode guard/key-decoder unit tests
// instead). What IS tested here, against the same `pub` API the interactive
// loop calls, is every decision that shapes the interactive UX: the turn
// gutter, the auto-fold threshold, the pin rail, `?name` docs, and the
// rerun replay plan (D-FE-REPL-RERUN1=A) — plus the bare `?name` transcript
// path (D-FE-REPL-DOCS1=B), which runs in every mode including this one.

use jet::REPL::{Docs, Render, RerunPlan, ReplTurn, ReplTurnStatus, Session};

#[test]
fn banner_wording_matches_hybrid_ratification() {
    // D-FE-REPL1=D ratified banner text (tests/cli/no_args_repl_banner.txt
    // pins the same wording end-to-end through the real `jet` binary).
    let banner = Render::render_banner("1.0.0", false);
    assert_eq!(banner, "Jet 1.0.0 — interactive REPL  (:quit, :help, ^B bindings)");
}

#[test]
fn interactive_prompt_carries_a_turn_gutter() {
    // `1 user> `, `2 user> `, … — the dim one-character-cost gutter that
    // distinguishes the interactive TTY prompt from the plain `user> ` the
    // non-TTY floor keeps (see `no_args_repl_banner_golden` in tests/cli.rs).
    assert_eq!(Render::render_prompt(1, false), "1 user> ");
    assert_eq!(Render::render_prompt(2, false), "2 user> ");
    assert_ne!(Render::render_prompt(1, false), "user> ");
}

#[test]
fn long_list_folds_past_threshold_short_list_does_not() {
    let short = jet::AST::CtValue::List((0..5).map(jet::AST::CtValue::Int).collect());
    assert!(
        Render::fold_decision_for_value(&short).is_none(),
        "a short list must print plainly, not fold"
    );

    let long = jet::AST::CtValue::List((0..42).map(jet::AST::CtValue::Int).collect());
    let (count, elem_ty) = Render::fold_decision_for_value(&long).expect("long list should fold");
    assert_eq!(count, 42);
    let marker = Render::render_fold_marker(count, &elem_ty, false);
    assert_eq!(marker, "⋯ 42 rows folded · [Int] · unfold ⏎");
}

#[test]
fn pin_rail_renders_binding_name_type_value_and_unpin_hint() {
    let rail = Render::render_pin_rail("total: Int :: 15", 3, 62, false);
    let head = rail.lines().next().unwrap();
    assert!(head.contains("total: Int :: 15"), "got: {head:?}");
    assert!(head.contains("turn 3"), "got: {head:?}");
    assert!(head.contains("unpin"), "got: {head:?}");
    assert!(head.contains("^P"), "got: {head:?}");
}

#[test]
fn docs_lookup_builtin_list_filter_matches_ratified_mock() {
    // D-FE-REPL-DOCS1=B ratified example: `?List.filter`.
    let session = Session::new();
    let doc = Docs::lookup(&session, "List.filter").expect("List.filter has builtin docs");
    assert!(
        doc.starts_with("List.filter(f: fn(T) -> Bool) -> List<T>"),
        "got: {doc:?}"
    );
    assert!(doc.contains("Keeps items where f(item) is true."), "got: {doc:?}");
    assert!(doc.contains("Source:"), "got: {doc:?}");
    assert!(doc.contains("Example:"), "shared symbol example missing: {doc:?}");
}

#[test]
fn shared_semantic_symbol_has_complete_identity_and_docs() {
    let symbol = jet::SemanticSymbols::lookup("List.filter").expect("shared List.filter symbol");
    assert_eq!(symbol.module, "core.collections");
    assert_eq!(symbol.owner, Some("List"));
    assert_eq!(symbol.member, "filter");
    assert!(symbol.signature.contains("fn(T) -> Bool"));
    assert!(!symbol.summary.is_empty());
    assert!(!symbol.example.is_empty());
    assert_eq!(symbol.provenance, "builtin");
}

#[test]
fn shared_semantic_symbols_catalog_numeric_conversions_and_parse() {
    let parse = jet::SemanticSymbols::lookup("Int.parse").expect("Int.parse symbol");
    assert_eq!(parse.signature, "Int.parse(text: String) -> Int ? ParseError");
    let narrow = jet::SemanticSymbols::lookup("F32.from_float")
        .expect("F32.from_float symbol");
    assert_eq!(narrow.module, "core.numeric");
    assert!(narrow.signature.ends_with("-> F32 ? String"));
    assert!(jet::SemanticSymbols::lookup("I64.from_f32").is_some());
    assert!(jet::SemanticSymbols::lookup("F64.from_u32").is_some());
}

#[cfg(target_os = "linux")]
#[test]
fn repl_raw_member_completion_menu_is_selectable() {
    let output = run_raw_multiline_pty(
        "printf 'items :: [1, 2, 3]\\r'; sleep 0.12; printf 'items.f\\t'; sleep 0.15; printf '\\033[B\\033[B\\r'; sleep 0.15; printf '\\003'",
    );
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "selectable completion PTY failed: {out}");
    assert!(out.contains("filter") && out.contains("filter_map"), "shared candidates missing: {out:?}");
    assert!(out.contains("items.find"), "two Down keys did not select the third completion: {out:?}");
}

#[test]
fn docs_lookup_local_binding_shows_live_value() {
    let mut session = Session::new();
    session.scope.insert("answer".to_string(), jet::AST::CtValue::Int(42));
    let doc = Docs::lookup(&session, "answer").expect("bound name should resolve");
    assert!(doc.starts_with("answer: Int :: 42\n"));
    assert!(doc.contains("Source: this session"));
    session.mutable_names.insert("answer".to_string());
    let doc = Docs::lookup(&session, "answer").expect("bound name should resolve");
    assert!(doc.starts_with("answer: Int := 42\n"));
}

#[test]
fn bare_question_name_is_the_primary_docs_spelling() {
    // D-FE-REPL-DOCS1=B: bare `?name` (no colon) is the primary spelling;
    // `:? name` stays an accepted alias to the same lookup.
    let bare = run_transcript(&["answer :: 42", "?answer"], None);
    let colon = run_transcript(&["answer :: 42", ":? answer"], None);
    assert!(bare.contains("answer: Int :: 42"), "got: {bare:?}");
    assert!(colon.contains("answer: Int :: 42"), "got: {colon:?}");
}

#[test]
fn live_binding_shadows_same_name_session_item_in_docs_and_completion() {
    let out = run_transcript(
        &["fn answer() -> Int { return 1 }", "answer :: 42", "?answer"],
        None,
    );
    assert!(out.contains("answer: Int :: 42"), "got: {out:?}");
}

#[test]
fn bare_question_mark_alone_shows_help_like_colon_form() {
    let out = run_transcript(&["?"], None);
    assert!(out.contains("REPL meta-commands"), "got: {out:?}");
}

fn fixture_turn(id: usize, input: &str, had_effect: bool, bound_name: Option<&str>) -> ReplTurn {
    ReplTurn {
        id,
        input: input.to_string(),
        summary: String::new(),
        status: ReplTurnStatus::Ok,
        folded: false,
        pinned: false,
        stale: false,
        had_effect,
        bound_name: bound_name.map(str::to_string),
        pending_unfold: None,
    }
}

#[test]
fn rerun_plan_gates_effectful_turns_and_lets_pure_turns_auto_replay() {
    // D-FE-REPL-RERUN1=A ratified shape: editing turn 1 replays turn 1 and
    // every turn after it; pure/binding turns replay automatically, a turn
    // that had an effect the first time needs a `y`/`N` confirmation.
    let turns = vec![
        fixture_turn(1, "rate :: 0.07", false, Some("rate")),
        fixture_turn(
            2,
            "invoice_total :: subtotal * (1.0 + rate)",
            false,
            Some("invoice_total"),
        ),
        fixture_turn(3, "write_file(\"out.txt\", invoice_total)", true, None),
    ];

    let plan = RerunPlan::build_replay_plan(&turns, 1, Some("rate :: 0.08")).expect("plan");
    assert_eq!(plan.steps.len(), 3);
    assert_eq!(plan.steps[0].input, "rate :: 0.08", "edited turn's text is substituted");
    assert!(RerunPlan::plan_needs_confirmation(&plan), "the write_file step needs confirmation");

    let rendered = RerunPlan::render_replay_plan(&plan, false);
    assert!(rendered.starts_with("Replay plan:\n"), "got: {rendered:?}");
    assert!(rendered.contains("auto"), "got: {rendered:?}");
    assert!(rendered.contains("confirm effect"), "got: {rendered:?}");
    assert!(rendered.trim_end().ends_with("Apply? [y/N]"), "got: {rendered:?}");

    // A plan with no effectful steps needs no confirmation at all.
    let pure_only = RerunPlan::build_replay_plan(&turns, 1, None).map(|mut p| {
        p.steps.truncate(2);
        p
    });
    assert!(!RerunPlan::plan_needs_confirmation(&pure_only.unwrap()));
}

#[test]
fn rerun_plan_keeps_interrupted_turn_addressable() {
    let mut interrupted = fixture_turn(1, "loop { }", false, None);
    interrupted.status = ReplTurnStatus::Interrupted;
    let plan = RerunPlan::build_replay_plan(&[interrupted], 1, None).expect("interrupted turn reruns");
    assert_eq!(plan.steps.len(), 1);
    assert_eq!(plan.steps[0].input, "loop { }");
}

#[test]
fn rerun_plan_rejects_unknown_turn_id() {
    let turns = vec![fixture_turn(1, "x :: 1", false, Some("x"))];
    assert!(RerunPlan::build_replay_plan(&turns, 99, None).is_err());
}

#[test]
fn textual_rerun_fallback_still_previews_a_turn() {
    // `run_transcript` is the non-TTY floor — it keeps the pre-redesign
    // `:rerun <id>` preview shape byte-for-byte (same assertion
    // `repl_notebook_turn_controls` already pins for `:turns`/`:pin`/`:fold`).
    // The plan-based replay (`Replay plan: … Apply? [y/N]`) is the cooked/
    // interactive-loop behavior (`crates/jet-repl/src/lib.rs::handle_meta`, exercised
    // by `RerunPlan`'s own unit/integration tests above) — `run_transcript`
    // doesn't route through it, by design (I6/floor: no pty in this harness).
    let out = run_transcript(&["1 + 2", ":rerun 1"], None);
    assert!(out.contains("rerun #1: 1 + 2"), "got: {out:?}");
    assert!(out.matches("3 : Int").count() >= 2, "got: {out:?}");
}

#[cfg(unix)]
#[test]
fn repl_raw_rerun_selects_edits_and_replays_downstream_state() {
    let output = run_raw_multiline_pty(
        "printf 'rate := 7\\r'; sleep 0.12; printf 'total :: rate * 2\\r'; sleep 0.12; printf '\\022'; sleep 0.12; printf '1\\r'; sleep 0.12; printf '\\1778\\r'; sleep 0.2; printf 'total\\r'",
    );
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "PTY status: {:?}\n{out}", output.status);
    assert!(out.contains("select turn to edit"), "arbitrary-turn selector missing: {out:?}");
    assert!(out.contains("rerun #2: total :: rate * 2"), "downstream turn not replayed: {out:?}");
    assert!(out.contains("16 : Int"), "edited state did not reach downstream binding: {out:?}");
}

#[cfg(unix)]
#[test]
fn repl_raw_rerun_effect_skip_marks_turn_and_downstream_stale_without_execution() {
    let output = run_raw_multiline_pty(
        "printf 'x := 1\\r'; sleep 0.12; printf 'print(\"EFFECT_ONCE\")\\r'; sleep 0.12; printf 'y :: x + 1\\r'; sleep 0.12; printf '\\022'; sleep 0.12; printf '1\\r'; sleep 0.12; printf '\\r'; sleep 0.12; printf 's'; sleep 0.15; printf ':turns\\r'",
    );
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "PTY status: {:?}\n{out}", output.status);
    let normalized = out.replace('\r', "");
    assert_eq!(normalized.lines().filter(|line| *line == "EFFECT_ONCE").count(), 1, "skipped effect replayed: {out:?}");
    assert!(out.contains("skip and mark stale"), "per-effect skip prompt missing: {out:?}");
    assert!(out.contains("stale"), "skipped/downstream turns not stale-marked: {out:?}");
}

#[test]
fn repl_cooked_rerun_edits_and_applies_downstream_state() {
    let state = std::env::temp_dir().join(format!("jet_repl_rerun_cooked_{}", std::process::id()));
    std::fs::remove_dir_all(&state).ok();
    let output = run_repl_process(
        &state,
        b"x := 1\ny :: x + 1\n:rerun 1\nx := 8\ny\n:quit\n",
        None,
    );
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "cooked rerun failed: {out}");
    assert!(out.contains("edit turn 1 [x := 1]"), "cooked edit prompt missing: {out}");
    assert!(out.contains("rerun #2: y :: x + 1"), "cooked downstream replay missing: {out}");
    assert!(out.contains("9 : Int"), "cooked edited state not live: {out}");
    std::fs::remove_dir_all(state).ok();
}

#[test]
fn repl_cooked_rerun_prompts_each_effect_and_skip_stales_downstream() {
    let state = std::env::temp_dir().join(format!("jet_repl_rerun_cooked_effect_{}", std::process::id()));
    std::fs::remove_dir_all(&state).ok();
    let output = run_repl_process(
        &state,
        b"x := 1\nprint(\"COOKED_EFFECT\")\ny :: x + 1\n:rerun 1\n\ns\n:turns\n:quit\n",
        None,
    );
    let out = String::from_utf8_lossy(&output.stdout).replace('\r', "");
    assert!(output.status.success(), "cooked effect rerun failed: {out}");
    assert_eq!(out.lines().filter(|line| *line == "COOKED_EFFECT").count(), 1, "skipped cooked effect replayed: {out}");
    assert!(out.contains("replay effect turn 2?"), "per-effect cooked prompt missing: {out}");
    assert!(out.contains("#2 ok stale") && out.contains("#3 ok stale"), "cooked stale marking missing: {out}");
    std::fs::remove_dir_all(state).ok();
}

#[cfg(unix)]
#[test]
fn repl_raw_project_baseline_survives_downstream_replay() {
    use std::process::Command;
    let root = std::env::temp_dir().join(format!("jet_repl_rerun_project_{}", std::process::id()));
    std::fs::remove_dir_all(&root).ok();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("helper.jet"), "fn add_three(x: Int) -> Int { return x + 3; }\n").unwrap();
    let shell = r#"
{
  sleep 0.2
  printf 'x := add_three(1)\r'
  sleep 0.15
  printf 'y :: add_three(x)\r'
  sleep 0.15
  printf '\022'
  sleep 0.12
  printf '1\r'
  sleep 0.12
  printf '\033[D\1772\r'
  sleep 0.2
  printf 'y\r'
  sleep 0.15
  printf ':quit\r'
} | timeout 8s script -qec '"$JET_REPL_BIN" repl --project "$JET_REPL_ROOT"' /dev/null
"#;
    let output = Command::new("sh")
        .args(["-c", shell])
        .env("JET_REPL_BIN", env!("CARGO_BIN_EXE_jet"))
        .env("JET_REPL_ROOT", &root)
        .env("JET_REPL_HISTORY", "off")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "project replay PTY failed: {out}");
    assert!(out.contains("rerun #2: y :: add_three(x)"), "project downstream replay missing: {out}");
    assert!(out.contains("8 : Int"), "project function baseline was lost: {out}");
    assert!(!out.contains("E0102"), "project function became unknown: {out}");
    std::fs::remove_dir_all(root).ok();
}

#[cfg(unix)]
#[test]
fn repl_raw_pin_and_fold_select_arbitrary_turns() {
    let output = run_raw_multiline_pty(
        "printf 'first :: 1\\r'; sleep 0.12; printf 'second :: 2\\r'; sleep 0.12; printf '\\020'; sleep 0.12; printf '1\\r'; sleep 0.12; printf '\\006'; sleep 0.12; printf '1\\r'; sleep 0.12; printf ':turns\\r'",
    );
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "PTY status: {:?}\n{out}", output.status);
    assert!(out.contains("select turn to pin"), "^P arbitrary selector missing: {out:?}");
    assert!(out.contains("select turn to fold"), "^F arbitrary selector missing: {out:?}");
    assert!(out.contains("#1 ok pinned folded"), "selected turn state missing: {out:?}");
    assert!(!out.contains("#2 ok pinned") && !out.contains("#2 ok folded"), "latest turn was changed instead: {out:?}");
}

#[test]
fn repl_cooked_prompt_keeps_truthful_turn_digits() {
    let state = std::env::temp_dir().join(format!("jet_repl_cooked_ids_{}", std::process::id()));
    std::fs::remove_dir_all(&state).ok();
    let output = run_repl_process(&state, b"1 + 1\n2 + 2\n:quit\n", None);
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "cooked REPL failed: {out}");
    assert!(out.contains("1 user> ") && out.contains("2 user> ") && out.contains("3 user> "), "cooked turn ids missing: {out:?}");
    std::fs::remove_dir_all(state).ok();
}

#[cfg(unix)]
#[test]
fn repl_raw_long_list_unfold_opens_indexed_pager() {
    let values = (0..25).map(|n| n.to_string()).collect::<Vec<_>>().join(", ");
    let script = format!(
        "printf '[{}]\\r'; sleep 0.2; printf '\\r'; sleep 0.15; printf 'j'; sleep 0.12; printf 'q'; sleep 0.12; printf ':quit\\r'",
        values
    );
    let output = run_raw_multiline_pty(&script);
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "PTY status: {:?}\n{out}", output.status);
    assert!(out.contains("25 rows folded"), "long collection did not fold: {out:?}");
    assert!(out.contains("idx") && out.contains("value"), "indexed table missing: {out:?}");
    assert!(out.contains("j/k") && out.contains("q"), "pager controls missing: {out:?}");
    assert!(out.contains(" 10 │ 10"), "pager did not advance to second page: {out:?}");
}

#[cfg(unix)]
#[test]
fn repl_raw_bindings_are_live_in_side_pane() {
    let output = run_raw_multiline_pty(
        "printf '\\002'; sleep 0.12; printf 'score := 7\\r'; sleep 0.2; printf ':quit\\r'",
    );
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "PTY status: {:?}\n{out}", output.status);
    assert!(out.contains("session") && out.contains("bindings"), "workspace headers missing: {out:?}");
    assert!(out.contains("│ score: Int := 7"), "binding not in side column: {out:?}");
    assert!(out.contains("new this step"), "live changed marker missing: {out:?}");
}

#[cfg(unix)]
#[test]
fn repl_raw_no_color_never_renders_history_ghost_as_real_text() {
    let output = run_raw_multiline_pty(
        "printf 'alphabet := 1\\r'; sleep 0.12; printf 'a'; sleep 0.15; printf '\\003'",
    );
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "PTY status: {:?}\n{out}", output.status);
    let second = out.rsplit("2 user> ").next().unwrap_or(&out);
    assert!(!second.contains("alphabet := 1"), "NO_COLOR ghost became fake typed text: {second:?}");
}

#[cfg(unix)]
#[test]
fn repl_raw_color_highlights_live_input() {
    use std::process::Command;
    let shell = r#"
{
  sleep 0.2
  printf 'value :: "hi"'
  sleep 0.15
  printf '\003'
} | timeout 8s script -qec '"$JET_REPL_BIN" repl' /dev/null
"#;
    let output = Command::new("sh")
        .args(["-c", shell])
        .env("JET_REPL_BIN", env!("CARGO_BIN_EXE_jet"))
        .env("JET_REPL_HISTORY", "off")
        .env("FORCE_COLOR", "1")
        .env_remove("NO_COLOR")
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "PTY status: {:?}\n{out}", output.status);
    assert!(out.contains("\u{1b}["), "live editor emitted no ANSI highlighting: {out:?}");
    assert!(out.contains("\u{1b}[1;96m") || out.contains("\u{1b}[32m"), "input tokens were not colorized: {out:?}");
}

#[cfg(target_os = "linux")]
#[test]
fn repl_terminal_matrix_raw_width_color_eof_and_ctrl_d() {
    use std::process::Command;
    for cols in [20, 120] {
        for (name, flag, no_color, force_color, expect_color) in [
            ("auto", "", false, false, true),
            ("always", "--color=always", true, false, true),
            ("never", "--color=never", false, true, false),
            ("empty-NO_COLOR", "", true, true, false),
            ("FORCE_COLOR", "", false, true, true),
        ] {
            let shell = format!(
                "{{ sleep 0.2; printf '1 + 1\\r'; sleep 0.12; printf '\\004'; }} | timeout 8s script -qec 'stty cols {cols}; \"$JET_REPL_BIN\" repl {flag}' /dev/null"
            );
            let mut command = Command::new("sh");
            command
                .args(["-c", &shell])
                .env("JET_REPL_BIN", env!("CARGO_BIN_EXE_jet"))
                .env("JET_REPL_HISTORY", "off")
                .env_remove("NO_COLOR")
                .env_remove("FORCE_COLOR");
            if no_color {
                command.env("NO_COLOR", "");
            }
            if force_color {
                command.env("FORCE_COLOR", "1");
            }
            let output = command.output().unwrap();
            let out = String::from_utf8_lossy(&output.stdout);
            assert!(
                output.status.success(),
                "raw {name}/{cols}-column Ctrl-D row failed: {out}"
            );
            assert!(
                out.contains("Tab")
                    && out.contains("F3")
                    && !out.contains("interactive keys require a TTY"),
                "raw mode not reached for {name} at width {cols}: {out:?}"
            );
            assert!(
                out.contains("2 : Int"),
                "raw evaluation failed for {name} at width {cols}: {out:?}"
            );
            assert_eq!(
                out.contains("\u{1b}[1;96m"),
                expect_color,
                "wrong color policy for {name} at width {cols}: {out:?}"
            );
        }
    }

    let eof = Command::new("sh")
        .args(["-c", "timeout 8s script -qec '\"$JET_REPL_BIN\" repl < /dev/null' /dev/null"])
        .env("JET_REPL_BIN", env!("CARGO_BIN_EXE_jet"))
        .env("NO_COLOR", "1")
        .env("JET_REPL_HISTORY", "off")
        .output()
        .unwrap();
    assert!(eof.status.success(), "PTY EOF row hung or failed: {}", String::from_utf8_lossy(&eof.stdout));
}

#[cfg(target_os = "linux")]
#[test]
fn repl_terminal_matrix_cooked_stdio_and_missing_stty_are_truthful() {
    use std::process::Command;
    let cases = [
        ("stdin-pipe/stdout-pty", "printf \"1 + 1\\n:quit\\n\" | \"$JET_REPL_BIN\" repl"),
        ("missing-stty", "PATH=/definitely-missing \"$JET_REPL_BIN\" repl"),
    ];
    for (name, inner) in cases {
        let shell = if name == "missing-stty" {
            format!("{{ sleep 0.2; printf '1 + 1\\r:quit\\r'; }} | timeout 8s script -qec '{inner}' /dev/null")
        } else {
            format!("timeout 8s script -qec '{inner}' /dev/null")
        };
        let output = Command::new("sh")
            .args(["-c", &shell])
            .env("JET_REPL_BIN", env!("CARGO_BIN_EXE_jet"))
            .env("NO_COLOR", "1")
            .env("JET_REPL_HISTORY", "off")
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "{name} row failed: {out}");
        assert!(out.contains("interactive keys require a TTY"), "{name} falsely claimed raw mode: {out:?}");
        assert!(!out.contains("Tab complete"), "{name} advertised unavailable keys: {out:?}");
        assert!(out.contains("1 user> ") && out.contains("2 : Int"), "{name} cooked behavior missing: {out:?}");
    }

    let root = std::env::temp_dir().join(format!("jet_repl_stdout_mode_{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let captured = root.join("stdout.txt");
    let shell = format!(
        "{{ sleep 0.2; printf '3 + 4\\r:quit\\r'; }} | timeout 8s script -qec '\"$JET_REPL_BIN\" repl > \"{}\"' /dev/null",
        captured.display()
    );
    let output = Command::new("sh").args(["-c", &shell]).env("JET_REPL_BIN", env!("CARGO_BIN_EXE_jet")).env("NO_COLOR", "1").env("JET_REPL_HISTORY", "off").output().unwrap();
    let stderr_side = String::from_utf8_lossy(&output.stdout);
    let stdout_side = std::fs::read_to_string(&captured).unwrap();
    assert!(output.status.success(), "stdin-pty/stdout-file row failed: {stderr_side}");
    assert!(stderr_side.contains("interactive keys require a TTY"), "redirected stdout falsely entered raw mode: {stderr_side:?}");
    assert!(stdout_side.contains("1 user> ") && stdout_side.contains("7 : Int"), "redirected cooked stdout missing: {stdout_side:?}");
    std::fs::remove_dir_all(root).ok();
}

#[cfg(target_os = "linux")]
#[test]
fn repl_terminal_matrix_keyboard_and_text_fallbacks_are_reachable() {
    let raw = run_raw_multiline_pty(
        "printf 'alpha := 1\\r'; sleep 0.12; printf '\\002'; sleep 0.12; printf '\\0201\\r'; sleep 0.12; printf '\\0061\\r'; sleep 0.12; printf '\\0221\\r\\r'; sleep 0.18; printf 'alp\\t\\r'; sleep 0.12; printf '\\033ORa\\r\\r'; sleep 0.15; printf ':quit\\r'",
    );
    let raw_out = String::from_utf8_lossy(&raw.stdout);
    assert!(raw.status.success(), "raw key matrix failed: {raw_out}");
    for marker in ["bindings", "select turn to pin", "select turn to fold", "select turn to edit", "history search>"] {
        assert!(raw_out.contains(marker), "raw key `{marker}` unreachable: {raw_out:?}");
    }
    assert!(raw_out.contains("1 : Int"), "Tab completion did not resolve live binding: {raw_out:?}");

    let state = std::env::temp_dir().join(format!("jet_repl_text_matrix_{}", std::process::id()));
    std::fs::remove_dir_all(&state).ok();
    let cooked = run_repl_process(
        &state,
        b"alpha := 1\n?alpha\n:pin 1\n:fold 1\n:unfold 1\n:rerun 1\n\n:history search alpha\n:turns\n:quit\n",
        None,
    );
    let cooked_out = String::from_utf8_lossy(&cooked.stdout);
    assert!(cooked.status.success(), "text fallback matrix failed: {cooked_out}");
    for marker in ["alpha: Int := 1", "turn pinned", "turn folded", "turn unfolded", "rerun #1", "#1 ok"] {
        assert!(cooked_out.contains(marker), "text fallback `{marker}` unreachable: {cooked_out:?}");
    }
    std::fs::remove_dir_all(state).ok();
}

// ── D-BIGINT1: comptime/REPL tier-0 BigInt (card #392) ─────────────────────
// Same expression shapes and expected decimal strings as the AOT golden
// example `examples/features/text/bigint.jet` /
// `examples/features/expected/text/bigint.out` — proves R12 parity, not just
// "doesn't crash".

#[test]
fn repl_bigint_construct_and_show() {
    let out = run_transcript(&["BigInt(100)"], None);
    assert!(out.trim().ends_with("100 : BigInt"), "got: {out:?}");
    assert!(!out.contains("E0956"), "got: {out:?}");
}

#[test]
fn repl_bigint_add_beyond_i64_max() {
    // max_i64 + 1 overflows a fixed Int; BigInt must not (that's the whole
    // point of D-BIGINT1 — no silent promotion, no overflow trap either).
    let inputs = &[
        "max_i64 :: BigInt(9223372036854775807)",
        "one :: BigInt(1)",
        "sum :: max_i64 + one",
        "sum.to_string()",
    ];
    let out = run_transcript(inputs, None);
    assert!(
        out.contains("\"9223372036854775808\" : String"),
        "got: {out:?}"
    );
}

#[test]
fn repl_bigint_from_string_add() {
    let inputs = &[
        "huge :: BigInt(\"999999999999999999999999999999\")",
        "doubled :: huge + huge",
        "doubled.to_string()",
    ];
    let out = run_transcript(inputs, None);
    assert!(
        out.contains("\"1999999999999999999999999999998\" : String"),
        "got: {out:?}"
    );
}

#[test]
fn repl_bigint_mul_massive() {
    let inputs = &[
        "massive :: BigInt(\"239238749287396287369263958629386592683596293856293659263596235962935869236592863592635962395862935629368592635926359623956293659236592635986239562936592635962395629365926359863892659826398568639269856\")",
        "massdoub :: massive * massive",
        "massdoub.to_string()",
    ];
    let out = run_transcript(inputs, None);
    assert!(
        out.contains(
            "\"57235179160597657597531047020200956544362763118771376009314169310643907927975523987347975666104048258728304782749129351109033383492052353630602575371754435856735360169485983219218233794801194422942999175714144363822988057872537938863053251946320370964445676871073768260912400343608038289822197795736070764739253602318976703277723078277315814109217255310422643607649043148897685808219536598364790260736\" : String"
        ),
        "got: {out:?}"
    );
}

#[test]
fn repl_bigint_sub_and_neg() {
    let inputs = &[
        "a :: BigInt(5)",
        "b :: BigInt(3)",
        "a.sub(b).to_string()",
        "b.neg().to_string()",
    ];
    let out = run_transcript(inputs, None);
    assert!(out.contains("\"2\" : String"), "got: {out:?}");
    assert!(out.contains("\"-3\" : String"), "got: {out:?}");
}

#[test]
fn repl_bigint_equality_is_numeric_not_identity() {
    let inputs = &[
        "left :: BigInt(\"-999999999999999999999999999999\")",
        "same :: BigInt(\"-999999999999999999999999999999\")",
        "other :: BigInt(\"999999999999999999999999999999\")",
        "left == same",
        "left != same",
        "left != other",
    ];
    let out = run_transcript(inputs, None);
    let bools = out
        .lines()
        .filter(|line| line.ends_with(" : Bool"))
        .collect::<Vec<_>>();
    assert_eq!(
        bools,
        ["true : Bool", "false : Bool", "true : Bool"],
        "got: {out:?}"
    );
    assert!(!out.contains("E0956"), "got: {out:?}");
}

// ── card #392: `core.random` widened ambient draws now dispatch at comptime ──
// (`bool`/`float_range`/`normal`/`exponential`/`bytes`/`pick`/`weighted_pick`/
// `sample`/`shuffle`/`split` — same SplitMix64 stream as AOT's `jet_std_random_*`
// (Process.rs), so a seeded transcript reproduces AOT's numbers exactly, R12).

#[test]
fn repl_core_random_widened_draws_dispatch() {
    let calls = [
        "#Grant(Rand) { caps -> random.seed(1) }",
        "#Grant(Rand) { caps -> random.bool(0.5) }",
        "#Grant(Rand) { caps -> random.float_range(1.0, 2.0) }",
        "#Grant(Rand) { caps -> random.normal(0.0, 1.0) }",
        "#Grant(Rand) { caps -> random.exponential(1.0) }",
        "#Grant(Rand) { caps -> random.bytes(3) }",
        "#Grant(Rand) { caps -> random.pick([1, 2, 3, 4, 5]) ?? 0 }",
        "#Grant(Rand) { caps -> random.sample([1, 2, 3, 4, 5], 2) }",
        "#Grant(Rand) { caps -> random.weighted_pick([1, 2, 3, 4, 5], [1.0, 1.0, 1.0, 1.0, 1.0]) ?? 0 }",
        "#Grant(Rand) { caps -> random.split(7) }",
    ];
    for call in calls {
        let out = run_transcript_with_flags(
            &["use core.random as random", call],
            None,
            &["rand"],
            &[],
        );
        assert!(!out.contains("error ["), "authorized call failed: {call}: {out}");
    }
    let shuffle = run_transcript_with_flags(
        &[
            "use core.random as random",
            "ys := [1, 2, 3, 4, 5]",
            "#Grant(Rand) { caps -> random.shuffle(&ys) }",
        ],
        None,
        &["rand"],
        &[],
    );
    assert!(!shuffle.contains("error ["), "authorized shuffle failed: {shuffle}");
}

// ── card #392: `core.fmt` (pure text formatting) now dispatches at comptime,
// byte-identical to AOT's `jet_fmt_*` (DataFmt.rs).

#[test]
fn repl_core_fmt_dispatch() {
    let inputs = &[
        "use core.fmt as fmt",
        "fmt.number(1234567)",
        "fmt.decimal(3.14159, 2)",
        "fmt.percent(0.4567, 1)",
        "fmt.bytes(1500000)",
        "fmt.duration(93784000)",
        "fmt.ordinal(23)",
        "fmt.plural(1, \"item\", \"items\")",
        "fmt.plural(3, \"item\", \"items\")",
        "fmt.pad_left(\"7\", 3, \"0\")",
        "fmt.pad_right(\"ab\", 5, \".\")",
        "fmt.pad_center(\"hi\", 6, \"*\")",
    ];
    let out = run_transcript(inputs, None);
    assert!(!out.contains("E0956"), "core.fmt should dispatch at comptime, got: {out}");
    assert!(out.contains("\"1,234,567\" : String"), "got: {out}");
    assert!(out.contains("\"3.14\" : String"), "got: {out}");
    assert!(out.contains("\"45.7%\" : String"), "got: {out}");
    assert!(out.contains("\"1.5 MB\" : String"), "got: {out}");
    assert!(out.contains("\"1d 2h 3m\" : String"), "got: {out}");
    assert!(out.contains("\"23rd\" : String"), "got: {out}");
    assert!(out.contains("\"1 item\" : String"), "got: {out}");
    assert!(out.contains("\"3 items\" : String"), "got: {out}");
    assert!(out.contains("\"007\" : String"), "got: {out}");
    assert!(out.contains("\"ab...\" : String"), "got: {out}");
    assert!(out.contains("\"**hi**\" : String"), "got: {out}");
}

// ── card #392: `core.encoding.base32` and base64's URL-safe variant now
// dispatch at comptime, byte-identical to AOT's `jet_std_base32_*` /
// `jet_std_b64url_*` (EncodingCodecs.rs).

#[test]
fn repl_core_encoding_base32_and_base64url_dispatch() {
    let inputs = &[
        "use core.encoding.base32 as base32",
        "use core.encoding.base64 as base64",
        "base32.encode([104, 101, 108, 108, 111])",
        "base32.decode(\"NBSWY3DP\")",
        "base64.encode_url([251, 239, 190])",
    ];
    let out = run_transcript(inputs, None);
    assert!(
        !out.contains("E0956"),
        "core.encoding.base32/base64 url variant should dispatch at comptime, got: {out}"
    );
    assert!(out.contains("\"NBSWY3DP\" : String"), "got: {out}");
    assert!(
        out.contains("[104, 101, 108, 108, 111] : Result"),
        "got: {out}"
    );
    assert!(out.contains("\"----\" : String"), "got: {out}");
}

// ── card #392 pass 3: `core.url` (D-URL1=A) now dispatches at comptime,
// ported verbatim from AOT's `JetUrl`/`jet_url_*` (`UrlMime.rs` +
// `MathRandomTime.rs`, see `UrlLite.rs`). `Url` instance methods
// (`.scheme()`/`.host()`/`.join()`/...) are a separate, pre-existing gap
// (`net_text_time.rs`'s sema dispatch, not `fixed_sigs.rs`'s `"core.url"`
// module table) — out of this pass's scope, so this transcript only
// exercises the module-level free functions and reads their result via
// plain string transforms (`percent_encode`/`percent_decode`/`query`) that
// don't require an instance method to observe.
#[test]
fn repl_core_url_dispatch() {
    let inputs = &[
        "use core.url as url",
        "url.percent_encode(\"a b/c\")",
        "url.percent_decode(\"a%20b\")",
        "url.query([[\"a\", \"1\"], [\"b\", \"2 c\"]])",
        "url.parse(\"https://ex.com/a/../b?x=1#f\")",
        "url.parse(\"not a url\")",
        "url.from_parts(\"https\", \"ex.com\", \"/p\", [[\"k\", \"v\"]], \"\")",
        "url.file(\"/tmp/x.txt\")",
    ];
    let out = run_transcript(inputs, None);
    assert!(
        !out.contains("E0956"),
        "core.url should dispatch at comptime, got: {out}"
    );
    assert!(out.contains("\"a%20b%2Fc\" : String"), "got: {out}");
    assert!(out.contains("a b : Result"), "got: {out}");
    assert!(out.contains("\"a=1&b=2%20c\" : String"), "got: {out}");
    assert!(out.contains("Url(scheme: https"), "got: {out}");
    // dot-segment normalization: `/a/../b` -> `/b`
    assert!(out.contains("path: /b,"), "got: {out}");
    // invalid scheme-less input takes the `ResErr` branch (not E0956/panic) —
    // `jet_show`'s generic `Result` display collapses any `ResErr` payload to
    // the literal `err` (`AST/comptime.rs`), so the specific message isn't
    // observable through plain auto-print; this only confirms the parser
    // rejected it rather than silently accepting garbage.
    assert!(out.contains("err : Result"), "got: {out}");
    assert!(out.contains("Url(scheme: file"), "got: {out}");
}

// ── card #392 pass 3: `core.data`'s fixed-signature stats/plot surface now
// dispatches at comptime, ported verbatim from AOT's `jet_data_*`
// (`EncodingTraits.rs` + `DataFmt.rs`, see `DataLite.rs`). `describe`/
// `status`/`bar_text`/`bar_svg` touch builtin struct values, so they're
// covered here (repl transcript) rather than `comptime_diff` — see that
// file's note on why struct `Display` can't be compared byte-for-byte yet.
// `bar_text`/`bar_svg` take `[DataGroup]`, but `DataGroup` isn't a
// user-constructible type name at comptime/REPL (E0119 — it's only ever
// produced by `group_count`/`group_sum`/`group_mean`, the generic
// call-site-typed pipeline functions that are still an open gap). So
// there's no Jet-source way to exercise `bar_text`/`bar_svg` standalone yet
// — they're verified instead by `DataLite.rs`'s own `#[cfg(test)]` module
// against AOT's exact expected output, and will get a real transcript once
// `group_*` closes that gap.
#[test]
fn repl_core_data_dispatch() {
    let inputs = &[
        "use core.data as data",
        "data.describe([1.0, 2.0, 3.0, 4.0])",
        "data.status()",
    ];
    let out = run_transcript(inputs, None);
    assert!(
        !out.contains("E0956"),
        "core.data stats surface should dispatch at comptime, got: {out}"
    );
    assert!(out.contains("DataSummary(count: 4"), "got: {out}");
    assert!(out.contains("mean: 2.5"), "got: {out}");
    assert!(out.contains("DataStatus(step: core.data.csv"), "got: {out}");
}

#[test]
fn repl_zstd_compress_is_resident() {
    let out = run_transcript(
        &[
            "use core.compress.zstd as zstd",
            "frame :: zstd.compress([72, 101, 108, 108, 111])",
            "frame.len() > 9",
            "frame[0] == (U8.from_int(40) ?? 0) && frame[1] == (U8.from_int(181) ?? 0) && frame[2] == (U8.from_int(47) ?? 0) && frame[3] == (U8.from_int(253) ?? 0)",
        ],
        None,
    );
    assert!(
        !out.contains("E0956"),
        "zstd compressor hit comptime fallback: {out}"
    );
    assert_eq!(out.matches("true : Bool").count(), 2, "got: {out}");
}

#[test]
fn repl_zstd_decompress_is_resident_and_typed() {
    let out = run_transcript(
        &[
            "use core.compress.zstd as zstd",
            "zstd.decompress([40, 181, 47, 253, 0, 88, 41, 0, 0, 104, 101, 108, 108, 111]) ?? []",
            "zstd.decompress([40, 181, 47]) ?? [255]",
        ],
        None,
    );
    assert!(!out.contains("E0956"), "resident decoder hit fallback: {out}");
    assert!(out.contains("[104, 101, 108, 108, 111]"), "got: {out}");
    assert!(out.contains("[255]"), "malformed frame was not a typed Err: {out}");
}

#[test]
fn repl_core_data_lazy_plans_and_typed_joins() {
    let inputs = &[
        "use core.data as data",
        "table :: data.table([3, 1, 2])",
        "lazy :: data.lazy(table)",
        "deferred :: data.lazy_filter(lazy, (x) => x > 10)",
        "data.plan(deferred)",
        "planned :: data.lazy_sort_by(data.lazy_filter(lazy, (x) => x > 1), (x) => \"{x}\")",
        "data.rows(data.collect(planned))",
        "data.inner_join([1, 2, 1], [1, 1], (x) => \"{x}\", (x) => \"{x}\")",
        "data.left_join([1, 2], [1], (x) => \"{x}\", (x) => \"{x}\")",
    ];
    let out = run_transcript(inputs, None);
    assert!(out.contains("[table, filter]"), "deferred plan missing: {out}");
    assert!(out.contains("[2, 3]"), "materialized filter/sort result missing: {out}");
    assert!(
        out.lines()
            .any(|line| line.matches("DataJoin(left: 1, right: 1)").count() == 4),
        "inner join multiplicity lost: {out}"
    );
    assert!(
        out.contains("DataJoin(left: 2, right: null)"),
        "left join unmatched row missing: {out}"
    );
}

#[test]
fn repl_core_data_schema_empty_table_and_series_law() {
    let inputs = &[
        "use core.data as data",
        "struct Ticket { team: String minutes: Float }",
        "empty_rows: [Ticket] := []",
        "empty_table :: data.table(empty_rows)",
        "data.schema(empty_table)",
        "data.schema(data.series([1.0, 2.0]))",
        "t :: Ticket.{team: \"Core\", minutes: 4.0}",
        "data.schema(data.series([t]))",
        "empty_tickets: [Ticket] := []",
        "data.schema(data.series(empty_tickets))",
        "struct Empty {}",
        "empty_units: [Empty] := []",
        "data.schema(data.table(empty_units))",
        "struct Box<T> { value: T }",
        "boxed: [Box<Int>] := []",
        "data.schema(data.table(boxed))",
    ];
    let out = run_transcript(inputs, None);
    assert!(
        !out.contains("E0956"),
        "core.data schema should dispatch at comptime, got: {out}"
    );
    assert!(
        out.contains("DataColumn(name: team, type_name: String)")
            && out.contains("DataColumn(name: minutes, type_name: Float)"),
        "empty Table<Ticket> must keep static columns: {out}"
    );
    assert!(
        out.contains("DataColumn(name: value, type_name: Float)"),
        "Series<Float> must be one value column: {out}"
    );
    let ticket_value_hits = out.matches("DataColumn(name: value, type_name: Ticket)").count();
    assert!(
        ticket_value_hits >= 2,
        "non-empty and empty Series<Ticket> both need value:Ticket (not expanded fields): {out}"
    );
    assert!(out.contains("[] : List"), "zero-field row must have zero columns: {out}");
    assert!(
        out.contains("DataColumn(name: value, type_name: Int)"),
        "generic row fields must substitute concrete type arguments: {out}"
    );
}

#[test]
fn repl_core_data_table_echo_hides_elem_type() {
    let inputs = &[
        "use core.data as data",
        "struct Ticket { team: String minutes: Float }",
        "empty_rows: [Ticket] := []",
        "data.table(empty_rows)",
        "data.series(empty_rows)",
        "data.lazy(data.table(empty_rows))",
    ];
    let out = run_transcript(inputs, None);
    assert!(
        !out.contains("E0956"),
        "core.data containers should dispatch at comptime, got: {out}"
    );
    assert!(
        out.contains("Table(rows:") && out.contains("Series(values:") && out.contains("LazyFrame(rows:"),
        "expected Table/Series/LazyFrame echoes: {out}"
    );
    assert!(
        !out.contains("elem_type:"),
        "elem_type is comptime-only metadata; must not leak into REPL echo: {out}"
    );
}

#[test]
fn repl_core_data_json_ingest_and_select() {
    let fixture = std::env::temp_dir().join(format!(
        "jet_repl_data_json_{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&fixture).unwrap();
    std::fs::write(
        fixture.join("model.jet"),
        "#Codable\nstruct Ticket { team: String minutes: Float }\n",
    )
    .unwrap();
    let project_dir = fixture.to_string_lossy().to_string();
    let inputs = &[
        "use core.data as data",
        r#"raw :: "[{{\"team\":\"Core\",\"minutes\":4.0}},{{\"team\":\"Tools\",\"minutes\":5.0}},{{\"team\":\"Core\",\"minutes\":8.0}}]""#,
        r#"rows :: data.json<Ticket>(raw) ?? panic("bad json")"#,
        "table :: data.table(rows)",
        "data.schema(table)",
        "selected :: data.filter(data.rows(table), (t) => t.minutes >= 5.0)",
        "data.count(selected)",
        "data.status()[6]",
    ];
    let out = run_transcript(inputs, Some(&project_dir));
    std::fs::remove_dir_all(fixture).ok();
    assert!(
        !out.contains("E0956"),
        "core.data json should dispatch at comptime, got: {out}"
    );
    assert!(
        out.contains("DataColumn(name: team, type_name: String)")
            && out.contains("DataColumn(name: minutes, type_name: Float)"),
        "json table schema missing: {out}"
    );
    assert!(
        out.contains("2 : Int"),
        "selected count missing: {out}"
    );
    assert!(
        out.contains("DataStatus(step: core.data.json") && out.contains("path: native"),
        "json status row missing: {out}"
    );
}
