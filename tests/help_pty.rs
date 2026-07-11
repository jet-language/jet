//! Real-PTY acceptance for the `jet ?` raw-mode app (D-FE-HELP1=D).

#![cfg(unix)]

use std::io::Write;
use std::process::{Command, Stdio};

fn run_pty(keys: &[u8], color: &str, no_color: bool) -> String {
    let jet = env!("CARGO_BIN_EXE_jet");
    let shell_line = format!("'{}' '?' '--color={}'", jet.replace('\'', "'\\''"), color);
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

#[test]
fn enter_expands_then_emits_prefill_without_running_it() {
    let transcript = run_pty(b"\r\r", "never", false);
    assert!(transcript.contains("jet run examples/features/basics/hello.jet"));
    assert!(!transcript.contains("Hello, Jet!"), "Enter executed selected command:\n{transcript}");
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
    assert!(transcript.contains("E0102 · code reference"), "F1 E-code search missed shared index:\n{transcript}");
}
