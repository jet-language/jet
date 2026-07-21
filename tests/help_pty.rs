//! Real-PTY acceptance for the `jet ?` raw-mode app (D-FE-HELP1=D).

#![cfg(unix)]

use std::io::Write;
use std::process::{Command, Stdio};

fn run_pty(keys: &[u8], color: &str, no_color: bool) -> String {
    run_pty_sized(keys, color, no_color, 24, 120)
}

fn run_pty_sized(keys: &[u8], color: &str, no_color: bool, rows: usize, cols: usize) -> String {
    run_pty_sized_steps(&[keys], color, no_color, rows, cols)
}

fn run_pty_sized_steps(steps: &[&[u8]], color: &str, no_color: bool, rows: usize, cols: usize) -> String {
    run_pty_steps(&format!("stty rows {rows} cols {cols};"), steps, color, no_color)
}

fn run_pty_steps(setup: &str, steps: &[&[u8]], color: &str, no_color: bool) -> String {
    let jet = env!("CARGO_BIN_EXE_jet");
    let shell_line = format!(
        "{setup} exec '{}' '?' '--color={}'",
        jet.replace('\'', "'\\''"),
        color
    );
    let mut command = Command::new("script");
    command.args(["-qfec", &shell_line, "/dev/null"])
        .env_remove("NO_COLOR").env_remove("FORCE_COLOR")
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if no_color { command.env("NO_COLOR", ""); }
    let mut child = command.spawn()
        .expect("util-linux script must allocate a real PTY");
    let mut stdin = child.stdin.take().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(150));
    for keys in steps {
        stdin.write_all(keys).unwrap();
        stdin.flush().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "PTY child failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn visible_cols(line: &str) -> usize {
    let mut escaped = false;
    line.chars()
        .filter(|&ch| {
            if escaped {
                if ch == 'm' { escaped = false; }
                false
            } else if ch == '\x1b' {
                escaped = true;
                false
            } else {
                true
            }
        })
        .count()
}

fn final_frame_before_cleanup(transcript: &str) -> String {
    let normalized = transcript.replace('\r', "");
    let parts: Vec<&str> = normalized.split("\x1b[J").collect();
    assert!(parts.len() >= 3, "missing redraw/cleanup sentinels:\n{transcript}");
    let mut frame = parts[parts.len() - 2].to_string();
    if let Some(alt_leave) = frame.find("\x1b[?1049l") {
        frame.truncate(alt_leave);
    }
    if let Some(cursor) = frame.rfind("\x1b[") {
        frame.truncate(cursor);
    }
    frame
}

fn cursor_up_counts(transcript: &str) -> Vec<usize> {
    let bytes = transcript.as_bytes();
    let mut counts = Vec::new();
    let mut i = 0;
    while i + 3 < bytes.len() {
        if bytes[i] == b'\x1b' && bytes[i + 1] == b'[' && bytes[i + 2].is_ascii_digit() {
            let mut end = i + 2;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            if bytes.get(end) == Some(&b'A') {
                counts.push(std::str::from_utf8(&bytes[i + 2..end]).unwrap().parse().unwrap());
                i = end;
            }
        }
        i += 1;
    }
    counts
}

fn transcript_selected_command(transcript: &str, command: &str) -> bool {
    transcript.replace('\r', "").lines().any(|line| {
        line.contains("│> ")
            && line.replace('[', "").replace(']', "").contains(&format!("jet {command}"))
    })
}

#[test]
fn enter_expands_then_emits_prefill_without_running_it() {
    let transcript = run_pty(b"\r\r", "never", false).replace('\r', "");
    assert!(
        transcript.contains("jet run"),
        "Enter should prefill command:\n{transcript}"
    );
    assert!(
        !transcript.contains("examples/features/basics/hello.jet"),
        "Enter prefills example, not command:\n{transcript}"
    );
    assert!(!transcript.contains("Hello, Jet!"), "Enter executed selected command:\n{transcript}");
}

#[test]
fn alt_enter_prefills_example_without_running_it() {
    // Expand category, then Alt-Enter (ESC then Enter) for the example line.
    let transcript = run_pty(b"\r\x1b\r", "never", false).replace('\r', "");
    assert!(
        transcript.contains("jet run examples/features/basics/hello.jet"),
        "Alt-Enter should prefill example:\n{transcript}"
    );
    assert!(!transcript.contains("Hello, Jet!"), "Alt-Enter executed selected command:\n{transcript}");
}

#[test]
fn shell_prefill_mode_keeps_palette_on_tty_while_stdout_is_captured() {
    let jet = env!("CARGO_BIN_EXE_jet");
    let picked = std::path::Path::new("/tmp").join(format!("jet-help-picked-{}", std::process::id()));
    let shell_line = format!(
        "JET_HELP_SHELL_PREFILL=1 '{}' '?' --color=never > '{}'; printf '\\nJET_HELP_CAPTURED\\n'",
        jet.replace('\'', "'\\''"),
        picked.display(),
    );
    let mut command = Command::new("script");
    command.args(["-qfec", &shell_line, "/dev/null"])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().expect("util-linux script must allocate a real PTY");
    let mut stdin = child.stdin.take().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(150));
    if let Err(error) = stdin.write_all(b"\r") {
        drop(stdin);
        let out = child.wait_with_output().unwrap();
        panic!(
            "PTY closed before input ({error}):\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
    }
    stdin.flush().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(150));
    stdin.write_all(b"\r").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(150));
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    let selected = std::fs::read_to_string(&picked).unwrap();
    let _ = std::fs::remove_file(&picked);
    assert!(out.status.success(), "PTY child failed: {}", String::from_utf8_lossy(&out.stderr));
    let transcript = String::from_utf8_lossy(&out.stdout);
    assert!(transcript.contains("command palette"), "captured stdout disabled palette:\n{transcript}");
    assert!(transcript.contains("JET_HELP_CAPTURED"), "shell did not regain control:\n{transcript}");
    assert_eq!(selected, "jet run\n");
    assert!(!transcript.contains("ready to copy"), "palette duplicated the shell prefill:\n{transcript}");
    assert!(!transcript.contains("Hello, Jet!"), "captured selection executed:\n{transcript}");
}

#[test]
fn explicit_color_law_reaches_interactive_renderer() {
    let colored = run_pty(b"q", "always", false);
    assert!(
        colored.contains("\x1b[1;96m") || colored.contains("\x1b[48;5;24"),
        "color renderer not active:\n{colored:?}"
    );
    let plain = run_pty(b"q", "never", false);
    assert!(!plain.contains("\x1b[1;96m"), "--color=never leaked styles:\n{plain:?}");
    assert!(!plain.contains("\x1b[48;5;24"), "--color=never leaked selection bg:\n{plain:?}");
    let no_color = run_pty(b"q", "auto", true);
    assert!(!no_color.contains("\x1b[1;96m"), "NO_COLOR leaked styles:\n{no_color:?}");
    assert!(!no_color.contains("\x1b[48;5;24"), "NO_COLOR leaked selection bg:\n{no_color:?}");
}

#[test]
fn f1_uses_alt_screen_and_escape_restores_palette() {
    let transcript = run_pty(b"\x1bOP\x1b\x1b", "never", false);
    assert!(transcript.contains("\x1b[?1049h"), "F1 did not enter reference screen");
    assert!(transcript.contains("\x1b[?1049l"), "Escape did not restore screen");
    assert!(transcript.contains("Usage   jet run"), "reference detail pane did not render:\n{transcript}");
    assert!(transcript.contains("command palette"), "overlay was not preserved:\n{transcript}");
}

#[test]
fn f1_search_opens_verbatim_error_code_page() {
    let transcript = run_pty(b"\x1bOPE0102\x1b\x1b", "never", false);
    let ex = jet::Explain::lookup("E0102").unwrap();
    let canonical = jet::Explain::render(&ex, false);
    let normalized = transcript.replace('\r', "");
    assert!(normalized.contains(&canonical), "F1 page diverged from canonical Explain bytes:\n{transcript}");
}

#[test]
fn normal_search_and_detail_open_verbatim_error_code_page() {
    let transcript = run_pty_sized_steps(&[b"E0102", b"\t", b"\x1b"], "never", false, 24, 120);
    let ex = jet::Explain::lookup("E0102").unwrap();
    let canonical = jet::Explain::render(&ex, false);
    let normalized = transcript.replace('\r', "");
    assert!(
        normalized.matches(&canonical).count() >= 2,
        "normal and Tab-detail E-code pages must both preserve canonical Explain bytes:\n{transcript}"
    );
}

#[test]
fn fuzzy_results_scroll_selected_row_within_terminal_height() {
    let index = jet::Help::build_index();
    let hits = jet::Help::search(&index, "E");
    let move_count = hits.len().saturating_sub(1).min(20);
    assert!(move_count > 18, "query must exercise a longer-than-viewport result set");
    let command = |index: usize| match &hits[index] {
        jet::Help::Hit::Command { entry, .. } => entry.symbol.name.as_str(),
        jet::Help::Hit::Code(_) => panic!("fuzzy query unexpectedly returned exact code"),
    };

    let mut keys = b"E".to_vec();
    keys.extend(std::iter::repeat_n(b"\x1b[B".as_slice(), move_count).flatten());
    let transcript = run_pty_sized_steps(&[&keys, b"\x1b"], "never", false, 24, 80);
    let frame = final_frame_before_cleanup(&transcript);

    assert!(
        cursor_up_counts(&transcript).into_iter().all(|count| count < 24),
        "a redraw exceeded the 24-row viewport"
    );
    assert!(
        transcript_selected_command(&transcript, command(move_count)),
        "scrolled selection was not rendered"
    );
    assert!(transcript_selected_command(&frame, command(move_count)), "selected row was clipped:\n{frame}");
    assert!(frame.lines().count() <= 24, "final frame exceeded 24 rows:\n{frame}");
    assert!(
        frame.lines().all(|line| visible_cols(line) <= 80),
        "final frame exceeded 80 cols:\n{frame}"
    );
}

#[test]
fn zero_size_pty_bounds_initial_transition_to_effective_height() {
    let transcript = run_pty_sized_steps(&[b"E", b"\x7fq"], "never", false, 0, 0);
    let frame = final_frame_before_cleanup(&transcript);
    assert!(
        cursor_up_counts(&transcript).into_iter().all(|count| count < 8),
        "zero-size transition moved above the effective viewport"
    );
    assert!(frame.lines().count() <= 8, "zero-size frame exceeded 8 rows:\n{frame}");
    assert!(
        frame.lines().all(|line| visible_cols(line) <= 50),
        "zero-size frame exceeded 50 cols:\n{frame}"
    );
}

#[test]
fn live_shrink_redraws_categorized_view_within_new_height() {
    let setup = "stty rows 24 cols 80; (sleep 0.4; stty rows 8 cols 50 </dev/tty) &";
    let down = b"\x1b[B\x1b[B\x1b[B\x1b[B\x1b[B";
    let transcript = run_pty_steps(
        setup,
        &[b"", b"", b"", down, b"", b"q"],
        "never",
        false,
    );
    let frame = final_frame_before_cleanup(&transcript);
    assert!(
        cursor_up_counts(&transcript).into_iter().all(|count| count < 8),
        "live shrink moved above the new viewport"
    );
    assert_eq!(frame.lines().count(), 8, "shrunken frame did not use 8 rows:\n{frame}");
    assert!(frame.contains("│> ▸ Reference"), "later selected category was clipped:\n{frame}");
    assert!(
        frame.lines().all(|line| visible_cols(line) <= 50),
        "shrunken frame exceeded 50 cols:\n{frame}"
    );
}

#[test]
fn live_narrow_resize_uses_actual_terminal_size() {
    let setup = "stty rows 24 cols 80; (sleep 0.4; stty rows 7 cols 32 </dev/tty) &";
    let transcript = run_pty_steps(
        setup,
        &[b"", b"", b"", b"\x1b[B", b"", b"q"],
        "never",
        false,
    );
    let frame = final_frame_before_cleanup(&transcript);
    assert!(
        cursor_up_counts(&transcript).into_iter().all(|count| count < 7),
        "live resize moved above the terminal"
    );
    assert_eq!(frame.lines().count(), 7, "resized frame did not use 7 rows:\n{frame}");
    assert!(
        frame.lines().all(|line| visible_cols(line) <= 32),
        "resized frame exceeded 32 cols:\n{frame}"
    );
}

#[test]
fn narrow_fuzzy_results_fit_actual_terminal() {
    let transcript = run_pty_sized_steps(&[b"run", b"\x1b"], "never", false, 7, 32);
    let frame = final_frame_before_cleanup(&transcript);
    assert!(frame.contains("jet [run]"), "fuzzy result was not rendered:\n{frame}");
    assert_eq!(frame.lines().count(), 7, "fuzzy frame exceeded 7 rows:\n{frame}");
    assert!(
        frame.lines().all(|line| visible_cols(line) <= 32),
        "fuzzy frame exceeded 32 cols:\n{frame}"
    );
}

#[test]
fn short_detail_view_scrolls_without_moving_terminal() {
    let mut keys = b"\r\t".to_vec();
    keys.extend(std::iter::repeat_n(b"\x1b[B".as_slice(), 12).flatten());
    let transcript = run_pty_sized_steps(&[&keys, b"\x1b", b"q"], "never", false, 8, 60);
    assert!(
        cursor_up_counts(&transcript).into_iter().all(|count| count < 8),
        "detail redraw moved above the terminal"
    );
    assert!(
        transcript.contains("--target") || transcript.contains("--profile") || transcript.contains("F1 reference"),
        "detail view did not scroll to later content:\n{transcript}"
    );
}

#[test]
fn short_f1_viewport_keeps_later_selection_visible_without_scrolling_terminal() {
    let mut keys = b"\x1bOP".to_vec();
    keys.extend(std::iter::repeat_n(b"\x1b[B".as_slice(), 7).flatten());
    keys.extend_from_slice(b"\x1b\x1b");
    let transcript = run_pty_sized(&keys, "never", false, 8, 60);
    let frame = final_frame_before_cleanup(&transcript);
    let lines: Vec<&str> = frame.lines().collect();
    assert!(frame.contains(">   bench"), "final selected row wrong/clipped:\n{frame}");
    assert_eq!(lines.len(), 8, "final frame scrolled or exceeded rows:\n{frame}");
    assert!(lines.iter().all(|line| visible_cols(line) <= 60), "final frame exceeded 60 cols:\n{frame}");
}
