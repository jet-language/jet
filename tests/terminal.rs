//! PTY proof for the shared terminal Prelude path.

#![cfg(unix)]

use std::fs::{self, OpenOptions};
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

struct NonPtyOutput {
    stdout: String,
    stderr: String,
}

fn run_non_pty(mode: &str) -> NonPtyOutput {
    let jet = env!("CARGO_BIN_EXE_jet");
    let example = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/features/io/terminal_parity.jet");
    let mut args = vec!["run", "--quiet"];
    match mode {
        "default" => {}
        "release" => args.push("--release"),
        "interpret" => args.push("--interpret"),
        _ => panic!("unknown non-PTY mode: {mode}"),
    }
    args.push(example);
    let mut child = Command::new(jet)
        .args(args)
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("non-PTY jet run must start");
    child
        .stdin
        .take()
        .expect("non-PTY stdin")
        .write_all(b"\nnot-a-number\n3\n2\n")
        .expect("write non-PTY answers");
    let output = child.wait_with_output().expect("collect non-PTY output");
    assert!(
        output.status.success(),
        "non-PTY child failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    NonPtyOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn run_non_pty_merged(mode: &str) -> String {
    let jet = env!("CARGO_BIN_EXE_jet");
    let example = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/features/io/terminal_parity.jet");
    let mut args = vec!["run", "--quiet"];
    match mode {
        "default" => {}
        "release" => args.push("--release"),
        "interpret" => args.push("--interpret"),
        _ => panic!("unknown merged non-PTY mode: {mode}"),
    }
    args.push(example);
    let path = std::env::temp_dir().join(format!(
        "jet_terminal_parity_merged_{}_{}",
        std::process::id(),
        mode
    ));
    let sink = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&path)
        .expect("merged output file");
    let stderr = sink.try_clone().expect("clone merged output file");
    let mut child = Command::new(jet)
        .args(args)
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::from(sink))
        .stderr(Stdio::from(stderr))
        .spawn()
        .expect("merged non-PTY jet run must start");
    child
        .stdin
        .take()
        .expect("merged non-PTY stdin")
        .write_all(b"\nnot-a-number\n3\n2\n")
        .expect("write merged non-PTY answers");
    let status = child.wait().expect("collect merged non-PTY status");
    assert!(status.success(), "merged non-PTY child failed: {status}");
    let merged = fs::read_to_string(&path).expect("read merged non-PTY output");
    let _ = fs::remove_file(path);
    merged
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

#[test]
fn terminal_parity_matches_non_tty_golden_and_stream_order() {
    let expected_stdout = include_str!(
        "../examples/features/expected/io/terminal_parity.out"
    );
    let expected_stderr = include_str!(
        "../examples/features/expected/io/terminal_parity.stderr.out"
    );
    let mut expected_merged = expected_stdout.to_string();
    expected_merged.push_str(expected_stderr);

    for mode in ["default", "release", "interpret"] {
        let output = run_non_pty(mode);
        assert_eq!(
            output.stdout, expected_stdout,
            "non-PTY stdout drifted in {mode} mode"
        );
        assert_eq!(
            output.stderr, expected_stderr,
            "non-PTY stderr drifted in {mode} mode"
        );
        assert_eq!(
            run_non_pty_merged(mode),
            expected_merged,
            "non-PTY stream order drifted in {mode} mode"
        );
    }
}
