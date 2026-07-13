//! Real-PTY acceptance for the `jet ?` raw-mode app (D-FE-HELP1=D).

#![cfg(unix)]

use std::io::Write;
use std::process::{Command, Stdio};

fn run_pty(keys: &[u8], color: &str, no_color: bool) -> String {
    run_pty_sized(keys, color, no_color, 24, 120)
}

fn run_pty_sized(keys: &[u8], color: &str, no_color: bool, rows: usize, cols: usize) -> String {
    let jet = env!("CARGO_BIN_EXE_jet");
    let shell_line = format!(
        "stty rows {rows} cols {cols}; exec '{}' '?' '--color={}'",
        jet.replace('\'', "'\\''"),
        color
    );
    let mut command = Command::new("script");
    command.args(["-qfec", &shell_line, "/dev/null"])
        .env_remove("NO_COLOR").env_remove("FORCE_COLOR")
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if no_color { command.env("NO_COLOR", "1"); }
    let mut child = command.spawn()
        .expect("util-linux script must allocate a real PTY");
    child.stdin.take().unwrap().write_all(keys).unwrap();
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

#[test]
fn enter_expands_then_emits_prefill_without_running_it() {
    let transcript = run_pty(b"\r\r", "never", false);
    assert!(transcript.contains("jet run examples/features/basics/hello.jet"));
    assert!(!transcript.contains("Hello, Jet!"), "Enter executed selected command:\n{transcript}");
}

#[test]
fn shell_prefill_mode_keeps_palette_on_tty_while_stdout_is_captured() {
    let jet = env!("CARGO_BIN_EXE_jet");
    let picked = std::env::temp_dir().join(format!("jet-help-picked-{}", std::process::id()));
    let shell_line = format!(
        "JET_HELP_SHELL_PREFILL=1 '{}' '?' --color=never > '{}'; printf '\\nJET_HELP_CAPTURED\\n'; cat '{}'",
        jet.replace('\'', "'\\''"),
        picked.display(),
        picked.display(),
    );
    let mut command = Command::new("script");
    command.args(["-qfec", &shell_line, "/dev/null"])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().expect("util-linux script must allocate a real PTY");
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"\r").unwrap();
    stdin.flush().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(150));
    stdin.write_all(b"\r").unwrap();
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    let _ = std::fs::remove_file(&picked);
    assert!(out.status.success(), "PTY child failed: {}", String::from_utf8_lossy(&out.stderr));
    let transcript = String::from_utf8_lossy(&out.stdout);
    assert!(transcript.contains("command palette"), "captured stdout disabled palette:\n{transcript}");
    assert!(transcript.contains("JET_HELP_CAPTURED"), "shell did not regain control:\n{transcript}");
    assert!(transcript.contains("jet run examples/features/basics/hello.jet"), "selection was not captured:\n{transcript}");
    assert!(!transcript.contains("Hello, Jet!"), "captured selection executed:\n{transcript}");
}

#[test]
fn explicit_color_law_reaches_interactive_renderer() {
    let colored = run_pty(b"q", "always", false);
    assert!(colored.contains("\x1b[1;36m"), "color renderer not active:\n{colored:?}");
    let plain = run_pty(b"q", "never", false);
    assert!(!plain.contains("\x1b[1;36m"), "--color=never leaked styles:\n{plain:?}");
    let no_color = run_pty(b"q", "auto", true);
    assert!(!no_color.contains("\x1b[1;36m"), "NO_COLOR leaked styles:\n{no_color:?}");
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
