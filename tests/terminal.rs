//! PTY proof for the shared terminal Prelude path.

#![cfg(unix)]

use std::io::Write;
use std::process::{Command, Stdio};

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn run_pty(mode: &str) -> String {
    let jet = shell_quote(env!("CARGO_BIN_EXE_jet"));
    let example = shell_quote(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/features/io/terminal_parity.jet"
    ));
    let profile = match mode {
        "default" => "",
        "release" => " --release",
        "interpret" => " --interpret",
        _ => panic!("unknown PTY mode: {mode}"),
    };
    let shell_line = format!(
        "stty rows 24 cols 100; exec {jet} run --quiet{profile} {example}"
    );
    let mut child = Command::new("script")
        .args(["-qfec", &shell_line, "/dev/null"])
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("util-linux script must allocate a real PTY");
    child
        .stdin
        .take()
        .expect("PTY stdin")
        .write_all(b"\nnot-a-number\n3\n2\nsecret\n")
        .expect("write terminal answers");
    let output = child.wait_with_output().expect("collect PTY transcript");
    assert!(
        output.status.success(),
        "PTY child failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn marker_positions(transcript: &str) -> Vec<usize> {
    let transcript = transcript.replace('\r', "");
    [
        "Continue? [y/N]",
        "Choose a target:",
        "choice: production",
        "Secret: ",
        "secret length: 6",
        "progress",
        "stderr stream",
    ]
    .into_iter()
    .map(|marker| {
        transcript
            .find(marker)
            .unwrap_or_else(|| panic!("PTY transcript missing {marker:?}: {transcript}"))
    })
    .collect()
}

#[test]
fn terminal_parity_uses_tty_prompt_and_progress_order() {
    let default = run_pty("default");
    let release = run_pty("release");
    let interpreter = run_pty("interpret");
    let normalized = |transcript: &str| transcript.replace('\r', "");
    assert_eq!(normalized(&default), normalized(&release));
    assert_eq!(normalized(&default), normalized(&interpreter));
    for transcript in [&default, &release, &interpreter] {
        let positions = marker_positions(transcript);
        assert!(
            positions.windows(2).all(|pair| pair[0] < pair[1]),
            "PTY program order changed: {transcript}"
        );
        assert!(
            transcript.contains("\rprogress"),
            "progress must redraw on a TTY: {transcript:?}"
        );
        assert!(
            !transcript.contains("secret: non-tty"),
            "PTY secret read was treated as non-TTY: {transcript:?}"
        );
    }
}
