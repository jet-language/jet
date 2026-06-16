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

/// `jet explain E2101`: an offline essay sourced from docs/spec/diagnostics.md.
#[test]
fn explain_e2101() {
    check("explain_E2101", &["explain", "E2101"]);
}

/// `jet explain E0102`: a second, unrelated code proves the index is general.
#[test]
fn explain_e0102() {
    check("explain_E0102", &["explain", "E0102"]);
}

/// `jet explain` is case-insensitive on the code.
#[test]
fn explain_lowercase() {
    check("explain_lowercase", &["explain", "e2101"]);
}

/// An unknown code fails cleanly (exit 2, no panic) with a helpful message.
#[test]
fn explain_unknown_code() {
    check("explain_unknown", &["explain", "BOGUS"]);
}

/// `jet explain` with no code teaches what it wants and exits 2.
#[test]
fn explain_missing_code() {
    check("explain_missing", &["explain"]);
}

/// Index obligation (I4 spirit): every diagnostic code registered in
/// docs/spec/diagnostics.md must round-trip through `jet explain` — the explain
/// index can never silently drift from the registry. Retired rows excluded.
#[test]
fn every_registered_code_is_explainable() {
    let codes = jet::explain::live_codes();
    assert!(
        codes.len() > 100,
        "expected the full diagnostics registry to be indexed, got {}",
        codes.len()
    );
    for code in &codes {
        let entry = jet::explain::lookup(code).unwrap_or_else(|| {
            panic!(
                "code `{}` is registered in docs/spec/diagnostics.md but `jet explain {}` finds nothing",
                code, code
            )
        });
        let essay = entry.essay();
        assert!(
            essay.contains(code) && essay.contains("What this means:"),
            "essay for `{}` is missing its code or a 'What this means:' section",
            code
        );
    }
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

// ── `--json` machine-readable diagnostics (D-DX1, E2-M3) ─────────────────────
//
// The `--json` flag emits one versioned JSON object per diagnostic to STDOUT
// (the machine stream), never any human prose or ANSI. The schema is documented
// in docs/spec/diagnostics.md ("Machine-readable diagnostics (`--json`)") and is
// the single source of truth shared by the future `jet fix` and the LSP.

/// Capture only the raw stdout bytes of `jet ARGS...` (where `--json` writes),
/// under a deterministic, ANSI-free environment.
fn json_stdout(args: &[&str]) -> String {
    let out = Command::new(jet_bin())
        .args(args)
        .env("NO_COLOR", "1")
        .env_remove("FORCE_COLOR")
        .output()
        .expect("run jet");
    String::from_utf8(out.stdout).expect("jet --json stdout must be UTF-8")
}

/// A deliberately tiny, dependency-free JSON validator (I6: std-only): it walks
/// the bytes tracking string/escape state and bracket/brace nesting, and rejects
/// anything structurally unbalanced. Enough to prove the emitter produced
/// well-formed JSON without pulling in serde.
fn is_valid_json(s: &str) -> bool {
    let mut depth: i32 = 0;
    let mut in_str = false;
    let mut escaped = false;
    let mut saw_value = false;
    for c in s.chars() {
        if in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                saw_value = true;
            }
            '{' | '[' => {
                depth += 1;
                saw_value = true;
            }
            '}' | ']' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            c if !c.is_whitespace() => saw_value = true,
            _ => {}
        }
    }
    depth == 0 && !in_str && saw_value
}

/// Pin the exact `--json` stdout bytes for `name`, and on every run assert the
/// output is ANSI-free and that each non-empty line is structurally valid JSON
/// carrying the versioned schema key. JSON Lines: one object per line.
fn check_json(name: &str, args: &[&str]) {
    let actual = json_stdout(args);

    // Scriptable-output contract: the JSON stream never carries ANSI.
    assert!(
        !actual.contains('\x1b'),
        "`{}` --json stdout must be ANSI-free, found an escape byte:\n{}",
        name,
        actual
    );
    // JSON Lines: every non-empty line is one valid, versioned diagnostic object.
    assert!(
        actual.lines().any(|l| !l.trim().is_empty()),
        "`{}` --json should emit at least one diagnostic line",
        name
    );
    for line in actual.lines().filter(|l| !l.trim().is_empty()) {
        assert!(
            is_valid_json(line),
            "`{}` --json emitted a structurally invalid JSON line:\n{}",
            name,
            line
        );
        assert!(
            line.starts_with("{\"schema_version\":1,"),
            "`{}` --json line must start with the versioned schema header:\n{}",
            name,
            line
        );
    }

    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/cli/{}.txt", name));
    if std::env::var("UPDATE_EXPECT").is_ok() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &actual).unwrap();
    } else {
        let expected = fs::read_to_string(&path).unwrap_or_default();
        assert_eq!(
            actual, expected,
            "\n--json stdout mismatch for tests/cli/{}.txt\n(if intentional and matching docs/spec/diagnostics.md, run: UPDATE_EXPECT=1 cargo test --test cli)\n",
            name
        );
    }
}

/// `jet doctor --plain`: environment self-diagnosis (D-DX2). The healthy nix
/// dev shell must exit 0. The transcript is made DETERMINISTIC by `--plain`:
/// it redacts volatile values (rustc/cc versions become `<version>`, absolute
/// store/cache/PATH locations become `<path>`) and uses plain ASCII status
/// words (`[ ok ]` / `[fail]` / `[warn]`) instead of color/glyphs, so the bytes
/// are identical across machines and reproducible in CI. We also pin the
/// store/cache dirs to temp paths so the cache/store section is healthy and
/// stable regardless of the developer's `~/.jet`.
#[test]
fn doctor_ok() {
    let tmp = std::env::temp_dir().join(format!("jet-doctor-test-{}", std::process::id()));
    let store = tmp.join("store");
    let cache = tmp.join("cache");
    fs::create_dir_all(&store).unwrap();
    fs::create_dir_all(&cache).unwrap();
    let out = Command::new(jet_bin())
        .args(["doctor", "--plain"])
        .env("NO_COLOR", "1")
        .env_remove("FORCE_COLOR")
        .env("JET_STORE_DIR", &store)
        .env("JET_CACHE_DIR", &cache)
        .output()
        .expect("run jet doctor");
    let mut actual = String::new();
    actual.push_str("$ jet doctor --plain\n");
    actual.push_str(&format!("[exit: {}]\n", out.status.code().unwrap_or(-1)));
    actual.push_str("--- stdout ---\n");
    actual.push_str(&String::from_utf8_lossy(&out.stdout));
    actual.push_str("--- stderr ---\n");
    actual.push_str(&String::from_utf8_lossy(&out.stderr));
    let _ = fs::remove_dir_all(&tmp);

    assert!(
        !actual.contains('\x1b'),
        "doctor --plain output must be ANSI-free, found an escape byte:\n{}",
        actual
    );
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/cli/doctor_ok.txt");
    if std::env::var("UPDATE_EXPECT").is_ok() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &actual).unwrap();
    } else {
        let expected = fs::read_to_string(&path).unwrap_or_default();
        assert_eq!(
            actual, expected,
            "\ndoctor transcript mismatch for tests/cli/doctor_ok.txt\n(if intentional, run: UPDATE_EXPECT=1 cargo test --test cli)\n"
        );
    }
}

/// `jet check --json`: a teaching error (E0037) serializes with a populated
/// `suggestions` array (the machine-applicable quick-fix the fix engine/LSP
/// will apply).
#[test]
fn json_check() {
    check_json("json_check", &["check", "tests/cli/json_check.jet", "--json"]);
}

/// `jet build --json`: a sema error (E0111) serializes; no mechanical fix, so
/// `suggestions` is an empty array (always present, never null).
#[test]
fn json_build() {
    check_json("json_build", &["build", "tests/cli/json_build.jet", "--json"]);
}

/// `jet test --json`: a sema error inside a `test` block serializes through the
/// same shared emitter.
#[test]
fn json_test() {
    check_json("json_test", &["test", "tests/cli/json_test.jet", "--json"]);
}
