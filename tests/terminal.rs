//! Cross-tier differential for the shared terminal Prelude path: default
//! `jet run`, AOT (`--release`), and the interpreter must answer the same, on a
//! PTY and off it, and must end when the input stream is closed.

#![cfg(unix)]

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const EXAMPLE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/examples/features/io/terminal_parity.jet"
);

/// A differential means nothing until we know which tier answered. The resident
/// tier has to compile `run` itself: a silent deopt would let tier0 answer for
/// every "tier" under test, and the transcripts would then agree for the wrong
/// reason.
fn assert_resident_tier_compiles_run() {
    let mut bundle = jet::Loader::load_entry(EXAMPLE).expect("terminal_parity loads");
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(
        !diags
            .iter()
            .any(|d| matches!(d.severity, jet::Diagnostics::Severity::Error)),
        "terminal_parity must check: {diags:?}"
    );
    let plan = jet_jit::plan_bundle_tiers(&bundle);
    assert!(
        !plan.whole_interp,
        "the whole program deopted to the interpreter: {plan:?}"
    );
    assert!(
        plan.native.contains("run"),
        "`run` must select the native tier; native={:?} deopt={:?}",
        plan.native,
        plan.deopt
    );
}

/// The same proof against a real default `jet run`: the tier rows must name a
/// natively compiled function and no deopt. Stdin is fed so the run completes,
/// and the trace lands on stderr, so this run stays out of the stream-order
/// comparison below.
fn assert_default_run_traces_native_tier() {
    // A warm run cache would answer from a published artifact and print no tier
    // rows, which would read as "no deopt" without proving anything. Point the
    // cache at a fresh directory so this run plans its tiers.
    let cache = std::env::temp_dir().join(format!(
        "jet_terminal_parity_trace_cache_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&cache);
    let mut child = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", "--quiet", "--trace-tiers", EXAMPLE])
        .env("NO_COLOR", "1")
        .env("JET_RUN_CACHE_DIR", &cache)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("traced jet run must start");
    child
        .stdin
        .take()
        .expect("traced stdin")
        .write_all(b"\nnot-a-number\n3\n2\n")
        .expect("write traced answers");
    let output = child.wait_with_output().expect("collect tier trace");
    let trace = String::from_utf8_lossy(&output.stderr).into_owned();
    let _ = fs::remove_dir_all(&cache);
    assert!(output.status.success(), "traced run failed: {trace}");
    assert!(
        trace.contains("tier1 native"),
        "no function reached the native tier: {trace}"
    );
    assert!(
        !trace.contains("tier0 interp"),
        "default `jet run` silently deopted: {trace}"
    );
    assert!(!trace.contains("E0956"), "{trace}");
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn run_pty(mode: &str) -> String {
    let jet = shell_quote(env!("CARGO_BIN_EXE_jet"));
    let example = shell_quote(EXAMPLE);
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
    let example = EXAMPLE;
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
    let example = EXAMPLE;
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
    assert_resident_tier_compiles_run();
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
    assert_resident_tier_compiles_run();
    assert_default_run_traces_native_tier();
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

/// A closed stdin is what every harness, CI job, and piped invocation gives an
/// interactive program. `choose` must stop there instead of re-prompting, so the
/// run has to end, stay small, and answer the same on all three tiers.
struct ClosedInputRun {
    code: Option<i32>,
    timed_out: bool,
    elapsed: Duration,
    stdout_bytes: u64,
    stdout: String,
    stderr: String,
}

/// Output goes to files, not pipes. A re-prompt loop would fill a pipe and stall
/// against an unread reader, which reads as a hang for the wrong reason, and the
/// size is taken before the text so a regression reports one number instead of
/// replaying a 512MB prompt log.
fn run_without_input(mode: &str) -> ClosedInputRun {
    let mut args = vec!["run", "--quiet"];
    match mode {
        "default" => {}
        "release" => args.push("--release"),
        "interpret" => args.push("--interpret"),
        _ => panic!("unknown closed-input mode: {mode}"),
    }
    args.push(EXAMPLE);
    let base = std::env::temp_dir().join(format!(
        "jet_terminal_parity_closed_{}_{mode}",
        std::process::id()
    ));
    let out_path = base.with_extension("out");
    let err_path = base.with_extension("err");
    let sink = |path: &std::path::Path| {
        OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)
            .expect("closed-input output file")
    };
    let started = Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(args)
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::from(sink(&out_path)))
        .stderr(Stdio::from(sink(&err_path)))
        .spawn()
        .expect("closed-input jet run must start");
    let deadline = Duration::from_secs(120);
    let (code, timed_out) = loop {
        match child.try_wait().expect("poll closed-input child") {
            Some(status) => break (status.code(), false),
            None if started.elapsed() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break (None, true);
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let elapsed = started.elapsed();
    let stdout_bytes = fs::metadata(&out_path)
        .expect("closed-input stdout metadata")
        .len();
    let stdout = if stdout_bytes <= 64 * 1024 {
        fs::read_to_string(&out_path).expect("read closed-input stdout")
    } else {
        String::new()
    };
    let stderr = fs::read_to_string(&err_path).unwrap_or_default();
    let _ = fs::remove_file(&out_path);
    let _ = fs::remove_file(&err_path);
    ClosedInputRun {
        code,
        timed_out,
        elapsed,
        stdout_bytes,
        stdout,
        stderr,
    }
}

/// The closed-input transcript is the checked-in golden with exactly two
/// differences: nothing rejects an answer, so both retry lines are gone, and the
/// documented default answers the stop, so the choice is `staging`.
fn expected_closed_input_stdout() -> String {
    let golden = include_str!("../examples/features/expected/io/terminal_parity.out");
    let retry = "> Enter a number from 1 to 2.\n";
    assert_eq!(
        golden.matches(retry).count(),
        2,
        "the golden no longer shows two rejected answers, so this derivation is stale"
    );
    golden
        .lines()
        .filter(|line| *line != retry.trim_end())
        .fold(String::new(), |mut out, line| {
            out.push_str(&line.replace("> choice: production", "> choice: staging"));
            out.push('\n');
            out
        })
}

/// One expectation for all three tiers is the differential: each mode is
/// compared to the same derived transcript, so any tier that answers differently
/// names itself.
#[test]
fn terminal_parity_ends_on_closed_input_on_every_tier() {
    assert_resident_tier_compiles_run();
    let expected = expected_closed_input_stdout();
    for mode in ["default", "release", "interpret"] {
        let run = run_without_input(mode);
        assert!(
            !run.timed_out,
            "{mode} mode never ended on a closed stdin after {:?}, and wrote {} bytes: {}",
            run.elapsed, run.stdout_bytes, run.stderr
        );
        assert!(
            run.stdout_bytes <= 64 * 1024,
            "{mode} mode wrote an unbounded prompt log: {} bytes",
            run.stdout_bytes
        );
        assert_eq!(
            run.code,
            Some(0),
            "{mode} mode did not finish cleanly on a closed stdin: {} {}",
            run.stdout,
            run.stderr
        );
        assert!(
            !run.stdout.contains("Enter a number from 1 to 2."),
            "{mode} mode retried a closed stream instead of stopping: {}",
            run.stdout
        );
        assert_eq!(
            run.stdout, expected,
            "{mode} mode drifted from the closed-input transcript every tier must produce"
        );
    }
}
