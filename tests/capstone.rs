//! Integration tests for the E2 capstone: logbook knowledge-base manager.
//! Smoke tests, golden output comparisons, and unit-test runner (I5).

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

fn logbook_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/capstone/logbook")
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Run logbook.jet with the given args via `jet run`.
/// Runs from the repo root so that error messages contain repo-relative paths,
/// matching the golden expected files.
fn run_logbook(jet: &PathBuf, args: &[&str]) -> std::process::Output {
    let root = repo_root();
    let mut cmd = Command::new(jet);
    cmd.current_dir(&root)
        .arg("run")
        .arg("examples/capstone/logbook/logbook.jet");
    for arg in args {
        cmd.arg(arg);
    }
    cmd.output().unwrap()
}

/// Smoke-test: run logbook.jet with no args and confirm it exits with usage output
/// (no ICE, no parse error, no rustc bleedthrough).
#[test]
fn capstone_files_parse_clean() {
    let jet = jet_bin();
    assert!(jet.exists(), "build the jet binary first (cargo build)");

    // Running with no args should print usage/banner and exit 2 — not an ICE.
    let out = run_logbook(&jet, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("internal compiler error"),
        "logbook.jet triggered an ICE:\n{}",
        stderr
    );
    assert!(
        !stderr.contains("error[E"),
        "rustc leaked through to stderr:\n{}",
        stderr
    );
    // No-args exits with 2 (usage).
    assert_eq!(
        out.status.code(),
        Some(2),
        "expected exit 2 for no-args invocation; stderr:\n{}",
        stderr
    );
}

/// Golden-output tests for each logbook subcommand.
/// Guarded by `have_rustc` — codegen requires a host Rust toolchain.
#[test]
fn capstone_golden_runs() {
    let jet = jet_bin();
    assert!(jet.exists(), "build the jet binary first (cargo build)");

    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: rustc not found; skipping capstone golden runs");
        return;
    }

    let expected_dir = logbook_dir().join("expected");
    // Use repo-relative path so error messages match the golden files.
    let notes = "examples/capstone/logbook/fixtures/notes";

    // --- index ---
    {
        let out = run_logbook(&jet, &["index", &notes]);
        assert!(
            out.status.success(),
            "logbook index failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        let expected = fs::read_to_string(expected_dir.join("index.out"))
            .expect("missing examples/capstone/logbook/expected/index.out");
        assert_eq!(
            stdout.trim(),
            expected.trim(),
            "output mismatch for logbook index\nactual:\n{}\nexpected:\n{}",
            stdout.trim(),
            expected.trim()
        );
    }

    // --- lint (exit 1, errors on stderr) ---
    {
        let out = run_logbook(&jet, &["lint", &notes]);
        assert_eq!(
            out.status.code(),
            Some(1),
            "logbook lint should exit 1 when issues exist; stderr:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stderr_raw = String::from_utf8_lossy(&out.stderr);
        // Strip jet compiler diagnostic lines — we only compare logbook app errors.
        // Compiler warnings have the format: "Warning [Lxxx]:", "  --> file:line",
        // " Why:", " Fix:", code-context lines like "NNN | code", and separator "|" lines.
        let stderr: String = stderr_raw
            .lines()
            .filter(|l| {
                let t = l.trim();
                !l.starts_with("Warning [")
                    && !t.starts_with("-->")
                    && !t.starts_with("|")
                    && !t.starts_with("Why:")
                    && !t.starts_with("Fix:")
                    && !l.contains("warnings emitted")
                    // filter "NNN |" code context lines
                    && !t.chars().next().map_or(false, |c| c.is_ascii_digit())
                    && !t.is_empty()
            })
            .collect::<Vec<_>>()
            .join("\n");
        let expected = fs::read_to_string(expected_dir.join("lint.out"))
            .expect("missing examples/capstone/logbook/expected/lint.out");
        assert_eq!(
            stderr.trim(),
            expected.trim(),
            "stderr mismatch for logbook lint"
        );
    }

    // --- find #feedback (tag search) ---
    {
        let out = run_logbook(&jet, &["find", &notes, "#feedback"]);
        assert!(
            out.status.success(),
            "logbook find #feedback failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        let expected = fs::read_to_string(expected_dir.join("find_tag.out"))
            .expect("missing examples/capstone/logbook/expected/find_tag.out");
        assert_eq!(
            stdout.trim(),
            expected.trim(),
            "output mismatch for logbook find #feedback"
        );
    }

    // --- find owner (text search) ---
    {
        let out = run_logbook(&jet, &["find", &notes, "owner"]);
        assert!(
            out.status.success(),
            "logbook find owner failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        let expected = fs::read_to_string(expected_dir.join("find_text.out"))
            .expect("missing examples/capstone/logbook/expected/find_text.out");
        assert_eq!(
            stdout.trim(),
            expected.trim(),
            "output mismatch for logbook find owner"
        );
    }

    // --- links owner-design ---
    {
        let out = run_logbook(&jet, &["links", &notes, "owner-design"]);
        assert!(
            out.status.success(),
            "logbook links owner-design failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        let expected = fs::read_to_string(expected_dir.join("links.out"))
            .expect("missing examples/capstone/logbook/expected/links.out");
        assert_eq!(
            stdout.trim(),
            expected.trim(),
            "output mismatch for logbook links owner-design"
        );
    }

    // --- graph --json ---
    {
        let out = run_logbook(&jet, &["graph", &notes, "json"]);
        assert!(
            out.status.success(),
            "logbook graph json failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        let expected = fs::read_to_string(expected_dir.join("graph.json"))
            .expect("missing examples/capstone/logbook/expected/graph.json");
        assert_eq!(
            stdout.trim(),
            expected.trim(),
            "output mismatch for logbook graph json"
        );
    }
}

/// Run `jet test` on note.jet and search.jet to exercise the embedded unit tests.
/// Guarded by `have_rustc`.
#[test]
fn capstone_unit_tests() {
    let jet = jet_bin();
    assert!(jet.exists(), "build the jet binary first (cargo build)");

    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: rustc not found; skipping capstone unit tests");
        return;
    }

    let lb = logbook_dir();

    for module in &["note.jet", "search.jet"] {
        let out = Command::new(&jet)
            .arg("test")
            .arg(lb.join(module))
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            out.status.success(),
            "jet test {} failed:\nstdout: {}\nstderr: {}",
            module,
            stdout,
            stderr
        );
    }
}
