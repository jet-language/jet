//! E2-M3 CLI foundations: golden transcripts for the `jet` driver itself.
//!
//! Each transcript lives in tests/cli/NAME.txt and pins the exact bytes the
//! CLI produces for a given argv: the command, its exit code, and its stdout
//! and stderr. Transcripts run with `NO_COLOR=1` so they are deterministic and
//! ANSI-free (the scriptable-output contract, E2-M3): scripts never parse ANSI.
//!
//! To re-bless after an INTENTIONAL change:
//!
//!     UPDATE_EXPECT=1 cargo test --test cli
//!
//! Never bless a transcript you haven't read against docs/spec/diagnostics.md.
//! The error messages ARE the language's UX.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

/// Run `jet ARGS...` with a deterministic, ANSI-free environment and render a
/// transcript that begins with the command and exit code, then the captured
/// streams. The format is line-oriented and stable so a human can read a diff.
fn transcript(args: &[&str]) -> String {
    let out = Command::new(jet_bin())
        .args(args)
        .env("NO_COLOR", "1")
        .env_remove("FORCE_COLOR")
        .output()
        .expect("run jet");
    let mut t = String::new();
    t.push_str(&format!("$ jet {}\n", args.join(" ")));
    t.push_str(&format!("[exit: {}]\n", out.status.code().unwrap_or(-1)));
    t.push_str("--- stdout ---\n");
    t.push_str(&String::from_utf8_lossy(&out.stdout));
    t.push_str("--- stderr ---\n");
    t.push_str(&String::from_utf8_lossy(&out.stderr));
    t
}

fn check(name: &str, args: &[&str]) {
    let actual = transcript(args);
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/cli/{}.txt", name));
    if std::env::var("UPDATE_EXPECT").is_ok() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &actual).unwrap();
    } else {
        let expected = fs::read_to_string(&path).unwrap_or_default();
        assert_eq!(
            actual, expected,
            "\nCLI transcript mismatch for tests/cli/{}.txt\n(if the new output is intentional and matches docs/spec/diagnostics.md, run: UPDATE_EXPECT=1 cargo test --test cli)\n",
            name
        );
    }
}

/// `jet` with no arguments greets and orients — it is NOT a usage error (exit 0).
#[test]
fn no_args_greeting() {
    check("no_args_greeting", &[]);
}

/// E2101: a typo'd subcommand suggests the intended one. E2102: an unknown
/// flag is named with the closest known flag.
#[test]
fn did_you_mean() {
    check("did_you_mean", &["biuld", "x.jet"]);
}

/// E2102: an unknown flag is reported with the what/why/fix voice and exit 2.
#[test]
fn unknown_flag() {
    check("unknown_flag", &["check", "--colour", "x.jet"]);
}

/// The scriptable-output contract: under `NO_COLOR` (and when piped, as the
/// captured pipe here is not a TTY), output carries no ANSI escape bytes.
#[test]
fn output_is_ansi_free_when_piped() {
    let outs = [
        transcript(&[]),
        transcript(&["biuld", "x.jet"]),
        transcript(&["check", "--colour", "x.jet"]),
    ];
    for o in &outs {
        assert!(
            !o.contains('\x1b'),
            "piped/NO_COLOR output must be ANSI-free, found an escape byte in:\n{}",
            o
        );
    }
}
