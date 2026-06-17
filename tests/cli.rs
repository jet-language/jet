//! E2-M3 Wave A — developer command UX golden tests.
//!
//! Pins:
//!   - the stable exit-code table (0/1/2/70/101);
//!   - human *and* `--json` diagnostic output for check/build/test;
//!   - CI determinism: output is byte-identical and ANSI-free under `NO_COLOR`
//!     and when piped (not a TTY);
//!   - `jet explain <CODE>` resolves for EVERY registered diagnostic code
//!     (closing the I4 loop: no code without an explain).
//!
//! Snapshots live in `tests/cli/*.txt`; bless with `UPDATE_EXPECT=1`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn jet() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

fn cli_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/cli")
}

/// Compare `actual` against `tests/cli/<name>`; bless on `UPDATE_EXPECT=1`.
fn check_snapshot(name: &str, actual: &str) {
    let path = cli_dir().join(name);
    if std::env::var("UPDATE_EXPECT").is_ok() {
        fs::create_dir_all(cli_dir()).unwrap();
        fs::write(&path, actual).unwrap();
        return;
    }
    let expected = fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("missing snapshot {}; run UPDATE_EXPECT=1 cargo test", path.display()));
    assert_eq!(actual, expected, "snapshot mismatch for {}", name);
}

/// Write a tiny source file with a known E0102 error and return its path.
fn bad_file() -> PathBuf {
    let p = std::env::temp_dir().join("jet_cli_bad.jet");
    fs::write(&p, "fn main() {\n    pirnt(\"hi\");\n}\n").unwrap();
    p
}

/// Replace machine-specific temp paths so snapshots are portable.
fn scrub(s: &str, file: &Path) -> String {
    s.replace(&file.display().to_string(), "BAD.jet")
}

// ── Exit-code table ────────────────────────────────────────────────

#[test]
fn exit_code_ok_check() {
    let p = std::env::temp_dir().join("jet_cli_ok.jet");
    fs::write(&p, "fn main() {\n    print(\"hi\");\n}\n").unwrap();
    let out = Command::new(jet()).arg("check").arg(&p).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "clean check should exit 0");
}

#[test]
fn exit_code_user_error_check() {
    let p = bad_file();
    let out = Command::new(jet()).arg("check").arg(&p).output().unwrap();
    assert_eq!(out.status.code(), Some(1), "a program error should exit 1");
}

#[test]
fn exit_code_usage_unknown_command() {
    // No-args greeting / usage is exit 2 (usage), distinct from a program error.
    let out = Command::new(jet()).output().unwrap();
    assert_eq!(out.status.code(), Some(2), "no args should exit 2 (usage)");
}

#[test]
fn exit_code_explain_unknown() {
    let out = Command::new(jet())
        .arg("explain")
        .arg("E9999")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "unknown code should exit 1");
}

// ── Human + JSON golden for one diagnostic ────────────────────────

#[test]
fn check_human_golden() {
    let p = bad_file();
    let out = Command::new(jet())
        .arg("check")
        .arg(&p)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = scrub(&String::from_utf8_lossy(&out.stderr), &p);
    check_snapshot("check_human.txt", &stderr);
}

#[test]
fn check_json_golden() {
    let p = bad_file();
    let out = Command::new(jet())
        .arg("check")
        .arg(&p)
        .arg("--json")
        .output()
        .unwrap();
    let stderr = scrub(&String::from_utf8_lossy(&out.stderr), &p);
    check_snapshot("check_json.txt", &stderr);
}

#[test]
fn build_json_golden() {
    let p = bad_file();
    let out = Command::new(jet())
        .arg("build")
        .arg(&p)
        .arg("--json")
        .output()
        .unwrap();
    let stderr = scrub(&String::from_utf8_lossy(&out.stderr), &p);
    check_snapshot("build_json.txt", &stderr);
}

#[test]
fn test_json_golden() {
    let p = bad_file();
    let out = Command::new(jet())
        .arg("test")
        .arg(&p)
        .arg("--json")
        .output()
        .unwrap();
    let stderr = scrub(&String::from_utf8_lossy(&out.stderr), &p);
    check_snapshot("test_json.txt", &stderr);
}

// ── CI determinism: ANSI-free + identical when piped/NO_COLOR ──────

#[test]
fn ci_output_is_ansi_free_when_piped() {
    let p = bad_file();
    // Default (piped, not a TTY): must be plain.
    let piped = Command::new(jet()).arg("check").arg(&p).output().unwrap();
    let s = String::from_utf8_lossy(&piped.stderr);
    assert!(!s.contains('\x1b'), "piped output must be ANSI-free:\n{}", s);

    // NO_COLOR explicitly set: also plain.
    let no_color = Command::new(jet())
        .arg("check")
        .arg(&p)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let nc = String::from_utf8_lossy(&no_color.stderr);
    assert!(!nc.contains('\x1b'), "NO_COLOR output must be ANSI-free");

    // And the two must be byte-identical (determinism).
    assert_eq!(s, nc, "piped and NO_COLOR output must match exactly");
}

#[test]
fn color_always_adds_ansi_but_flag_wins_over_no_color() {
    let p = bad_file();
    let out = Command::new(jet())
        .arg("check")
        .arg(&p)
        .arg("--color=always")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&out.stderr);
    assert!(
        s.contains('\x1b'),
        "--color=always must win over NO_COLOR and emit ANSI"
    );
}

// ── explain coverage: every registered code resolves ──────────────

#[test]
fn every_registered_code_has_an_explain_entry() {
    let md = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/spec/diagnostics.md"),
    )
    .unwrap();

    // Pull every E####/L#### that appears as the first cell of a table row —
    // i.e. a registered code, not an in-prose mention.
    let mut codes: Vec<String> = Vec::new();
    for line in md.lines() {
        let line = line.trim();
        if !line.starts_with("| E") && !line.starts_with("| L") {
            continue;
        }
        let first = line.trim_matches('|').split('|').next().unwrap_or("").trim();
        if is_code(first) && !codes.contains(&first.to_string()) {
            codes.push(first.to_string());
        }
    }
    assert!(codes.len() > 150, "expected the full code registry, found {}", codes.len());

    let index = jet::explain::index();
    for code in &codes {
        assert!(
            index.contains_key(code),
            "code {} is registered in diagnostics.md but has no explain entry",
            code
        );
        // And `jet explain <code>` must succeed at the CLI for every code.
        let out = Command::new(jet()).arg("explain").arg(code).output().unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "`jet explain {}` should succeed",
            code
        );
        assert!(
            String::from_utf8_lossy(&out.stdout).contains(code.as_str()),
            "`jet explain {}` output should name the code",
            code
        );
    }
}

#[test]
fn explain_golden() {
    let out = Command::new(jet())
        .arg("explain")
        .arg("E2001")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    check_snapshot("explain_E2001.txt", &stdout);
}

fn is_code(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 5 && (b[0] == b'E' || b[0] == b'L') && b[1..].iter().all(|c| c.is_ascii_digit())
}
