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
    let expected = fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "missing snapshot {}; run UPDATE_EXPECT=1 cargo test",
            path.display()
        )
    });
    assert_eq!(actual, expected, "snapshot mismatch for {}", name);
}

/// Write a tiny source file with a known error and return its path. Each test
/// passes a unique `tag` so concurrent tests never share a path — `fs::write`
/// truncates-then-writes, so a shared path would let one test's write race a
/// sibling's `jet check` read (seeing a momentarily-empty file).
fn bad_file(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("jet_cli_bad_{tag}.jet"));
    fs::write(&p, "fn run() {\n    pirnt(\"hi\");\n}\n").unwrap();
    p
}

/// Replace machine-specific temp paths so snapshots are portable.
fn scrub(s: &str, file: &Path) -> String {
    s.replace(&file.display().to_string(), "BAD.jet")
}

/// A private cwd for a `jet run`/`build`/`bench`/`test` subprocess.
///
/// `jet` writes compiled output to `build/<stem>.rs` + `build/<stem>` *relative
/// to its own cwd* (Source/CmdCompile.rs `bin_path`/`stem`/`build`), keyed only
/// by the source file's stem — not its full path. Two concurrent `jet`
/// processes compiling different files that happen to share a stem (e.g. two
/// `main.jet` fixtures) race on that shared `build/` path if both inherit the
/// test harness's cwd (the repo root). Giving each such test its own cwd
/// removes the shared namespace entirely, regardless of stem.
fn isolated_cwd(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("jet_cli_cwd_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

// ── Exit-code table ────────────────────────────────────────────────

#[test]
fn exit_code_ok_check() {
    let p = std::env::temp_dir().join("jet_cli_ok.jet");
    fs::write(&p, "fn run() {\n    print(\"hi\");\n}\n").unwrap();
    let out = Command::new(jet()).arg("check").arg(&p).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "clean check should exit 0");
}

#[test]
fn exit_code_user_error_check() {
    let p = bad_file(&line!().to_string());
    let out = Command::new(jet()).arg("check").arg(&p).output().unwrap();
    assert_eq!(out.status.code(), Some(1), "a program error should exit 1");
}

#[test]
fn exit_code_no_args_starts_repl() {
    // c6vz465: bare `jet` starts the REPL — exit 0 after EOF on piped stdin.
    let out = Command::new(jet()).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "no args should start REPL (exit 0)"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("interactive REPL"),
        "bare jet should print REPL banner:\n{}",
        stdout
    );
}

#[test]
fn exit_code_unknown_subcommand_is_usage() {
    // A typo'd subcommand is a usage error (exit 2) and teaches E2101.
    let out = Command::new(jet()).arg("buld").output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "unknown subcommand should exit 2"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E2101"), "should cite E2101:\n{}", stderr);
    assert!(
        stderr.contains("build"),
        "should suggest `build`:\n{}",
        stderr
    );
}

#[test]
fn frequency_ring_groups_execute_real_handlers() {
    let out = Command::new(jet()).args(["self", "man"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains(".TH JET 1"));

    let out = Command::new(jet()).args(["inspect", "semindex"]).output().unwrap();
    assert_ne!(out.status.code(), Some(2), "group must reach semindex handler");
    assert!(String::from_utf8_lossy(&out.stderr).contains("needs an entry file"));

    let out = Command::new(jet()).args(["store", "generations"]).output().unwrap();
    assert_ne!(out.status.code(), Some(2), "existing grouped handler must remain live");
}

#[test]
fn moved_bare_commands_are_teaching_errors_not_aliases() {
    for (verb, replacement) in [
        ("publish", "jet registry publish"),
        ("semindex", "jet inspect semindex"),
        ("gc", "jet store gc"),
        ("doctor", "jet self doctor"),
        ("lsp", "jet self lsp"),
    ] {
        let out = Command::new(jet()).arg(verb).arg("sentinel").output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{verb} must be rejected");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("E2101"), "{verb}: {stderr}");
        assert!(stderr.contains(replacement), "{verb}: {stderr}");
    }

    let out = Command::new(jet()).args(["lsp", "--json"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stderr.is_empty());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("\"code\":\"E2101\""));
    assert!(stdout.contains("jet self lsp --json"));
}

#[test]
fn every_moved_bare_action_is_e2101_in_human_and_json_modes() {
    for group in jet::CLI::COMMAND_GROUPS {
        for action in group.actions {
            let replacement = format!("jet {} {}", group.name, action.name);
            let out = Command::new(jet()).arg(action.name).arg("sentinel").output().unwrap();
            assert_eq!(out.status.code(), Some(2), "bare {}", action.name);
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert!(stderr.contains("E2101") && stderr.contains(&replacement), "{}: {stderr}", action.name);

            let out = Command::new(jet()).args([action.name, "sentinel\\\"quoted", "--json"]).output().unwrap();
            assert_eq!(out.status.code(), Some(2), "bare {} --json", action.name);
            assert!(out.stderr.is_empty(), "JSON diagnostic leaked stderr for {}", action.name);
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert!(stdout.contains("\"code\":\"E2101\"") && stdout.contains(&replacement), "{}: {stdout}", action.name);
            assert!(stdout.contains("sentinel\\\\\\\"quoted"), "replacement was not JSON escaped: {stdout}");
        }
    }
}

#[test]
fn invalid_nested_action_is_e2101_and_json_escaped() {
    let bad = "bad\\\"action";
    for group in jet::CLI::COMMAND_GROUPS {
        let out = Command::new(jet()).args([group.name, bad]).output().unwrap();
        assert_eq!(out.status.code(), Some(2));
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("E2101") && stderr.contains(bad), "{stderr}");

        let out = Command::new(jet()).args([group.name, bad, "--json"]).output().unwrap();
        assert_eq!(out.status.code(), Some(2));
        assert!(out.stderr.is_empty());
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("bad\\\\\\\"action"), "invalid JSON escaping: {stdout}");
        assert!(!stdout.contains("`bad\\\"action`"), "raw quote leaked into JSON: {stdout}");
    }
}

#[test]
fn grouped_e2101_human_and_json_goldens() {
    let moved = Command::new(jet()).args(["publish", "sentinel"]).output().unwrap();
    assert_eq!(moved.status.code(), Some(2));
    assert!(moved.stdout.is_empty());
    check_snapshot("moved_bare_e2101_human.txt", &String::from_utf8_lossy(&moved.stderr));

    let moved_json = Command::new(jet()).args(["publish", "sentinel\\\"quoted", "--json"]).output().unwrap();
    assert_eq!(moved_json.status.code(), Some(2));
    assert!(moved_json.stderr.is_empty());
    check_snapshot("moved_bare_e2101_json.txt", &String::from_utf8_lossy(&moved_json.stdout));

    let invalid = Command::new(jet()).args(["inspect", "bad\\\"action"]).output().unwrap();
    assert_eq!(invalid.status.code(), Some(2));
    assert!(invalid.stdout.is_empty());
    check_snapshot("invalid_nested_e2101_human.txt", &String::from_utf8_lossy(&invalid.stderr));

    let invalid_json = Command::new(jet()).args(["inspect", "bad\\\"action", "--json"]).output().unwrap();
    assert_eq!(invalid_json.status.code(), Some(2));
    assert!(invalid_json.stderr.is_empty());
    check_snapshot("invalid_nested_e2101_json.txt", &String::from_utf8_lossy(&invalid_json.stdout));
}

#[test]
fn group_help_and_man_inventory_every_nested_description() {
    let man = Command::new(jet()).args(["self", "man"]).output().unwrap();
    assert_eq!(man.status.code(), Some(0));
    let man = String::from_utf8_lossy(&man.stdout);
    for group in jet::CLI::COMMAND_GROUPS {
        let out = Command::new(jet()).args([group.name, "help"]).output().unwrap();
        assert_eq!(out.status.code(), Some(0));
        let help = String::from_utf8_lossy(&out.stdout);
        assert!(help.contains(group.summary), "{} help missing summary", group.name);
        for action in group.actions {
            assert!(help.contains(action.name) && help.contains(action.summary), "{} help missing {}", group.name, action.name);
            assert!(man.contains(&format!(".B {} {}", group.name, action.name)), "man missing {} {}", group.name, action.name);
            assert!(man.contains(action.summary), "man missing summary for {} {}", group.name, action.name);
        }
    }
}

#[test]
fn palette_uses_canonical_nested_routes() {
    for group in jet::CLI::COMMAND_GROUPS {
        for action in group.actions {
            let route = format!("{} {}", group.name, action.name);
            let out = Command::new(jet()).args(["?", &route]).output().unwrap();
            assert_eq!(out.status.code(), Some(0));
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert!(stdout.contains(&route), "palette missing {route}: {stdout}");
            assert!(!stdout.contains(&format!("jet {}   ", action.name)), "palette advertised bare moved action {}", action.name);
        }
    }
}

#[test]
fn jet_install_teaches_jet_fetch() {
    // `jet install` is not a Jet command; the compiler emits E0043 pointing to `jet store fetch`.
    let out = Command::new(jet()).arg("install").output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E0043"),
        "`jet install` should emit E0043 teaching error:\n{stderr}"
    );
    assert!(
        stderr.contains("jet store fetch"),
        "`jet install` error should mention `jet store fetch`:\n{stderr}"
    );
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
    let p = bad_file(&line!().to_string());
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
    let p = bad_file(&line!().to_string());
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
    let p = bad_file(&line!().to_string());
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
    let p = bad_file(&line!().to_string());
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
    let p = bad_file(&line!().to_string());
    // Default (piped, not a TTY): must be plain.
    let piped = Command::new(jet()).arg("check").arg(&p).output().unwrap();
    let s = String::from_utf8_lossy(&piped.stderr);
    assert!(
        !s.contains('\x1b'),
        "piped output must be ANSI-free:\n{}",
        s
    );

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
    let p = bad_file(&line!().to_string());
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
        let first = line
            .trim_matches('|')
            .split('|')
            .next()
            .unwrap_or("")
            .trim();
        if is_code(first) && !codes.contains(&first.to_string()) {
            codes.push(first.to_string());
        }
    }
    assert!(
        codes.len() > 150,
        "expected the full code registry, found {}",
        codes.len()
    );

    let index = jet::Explain::index();
    for code in &codes {
        assert!(
            index.contains_key(code),
            "code {} is registered in diagnostics.md but has no explain entry",
            code
        );
        // And `jet explain <code>` must succeed at the CLI for every code.
        let out = Command::new(jet())
            .arg("explain")
            .arg(code)
            .output()
            .unwrap();
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

#[test]
fn jetpack_missing_build_log_golden() {
    let cwd = isolated_cwd(&line!().to_string());
    let root = cwd.join("jetpack-root");
    let out = Command::new(jet())
        .args(["logs", "definitely_missing", "--no-color"])
        .current_dir(&cwd)
        .env("JETPACK_ROOT", &root)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "missing log is usage-class error"
    );
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    check_snapshot("e1274_missing_build_log.txt", &stderr);
}

fn is_code(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 5 && (b[0] == b'E' || b[0] == b'L') && b[1..].iter().all(|c| c.is_ascii_digit())
}

// ── Wave B: greeting, did-you-mean, doctor, completions, fix, externals ──

#[test]
fn no_args_repl_banner_golden() {
    let out = Command::new(jet()).env("NO_COLOR", "1").output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    check_snapshot("no_args_repl_banner.txt", &stdout);
}

#[test]
fn question_mark_is_help_golden() {
    let out = Command::new(jet())
        .arg("?")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "`jet ?` should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    check_snapshot("question_mark_help.txt", &stdout);
}

/// D-FE-HELP1=D: `jet ? <query>` (piped, i.e. non-TTY) is the non-interactive
/// floor — best matches for the query, printed once, no raw mode.
#[test]
fn question_mark_query_prints_matches_non_interactively() {
    let out = Command::new(jet())
        .args(["?", "run"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "`jet ? run` should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(stdout.contains("jet run"), "expected a `run` match, got:\n{}", stdout);
}

/// A query that looks like a diagnostic code renders the verbatim I4 essay —
/// byte-identical to `jet explain <CODE>`, since both go through
/// `jet::Explain::render` over the same registry (single source of truth).
#[test]
fn question_mark_code_query_matches_explain_verbatim() {
    let via_help = Command::new(jet())
        .args(["?", "E0102"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let via_explain = Command::new(jet())
        .args(["explain", "E0102"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(via_help.status.code(), Some(0));
    assert_eq!(via_explain.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&via_help.stdout),
        String::from_utf8_lossy(&via_explain.stdout),
        "`jet ? E0102` must render the same verbatim essay as `jet explain E0102` (I4)"
    );
}

/// A multi-word task/outcome phrase still resolves to a real command line —
/// the owner-modified default (2026-07-08): keywords are aliases on command
/// entries, never a separate goal menu, but they must still be findable.
#[test]
fn question_mark_task_phrase_resolves_to_a_real_command() {
    let out = Command::new(jet())
        .args(["?", "add", "a", "dependency"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(stdout.contains("jet add"), "expected `add` to surface, got:\n{}", stdout);
}

#[test]
fn file_sugar_runs_without_run_subcommand() {
    let stem = std::env::temp_dir().join("jet_cli_file_sugar");
    let file = stem.with_extension("jet");
    fs::write(&file, "fn run() {\n    print(\"file-sugar\");\n}\n").unwrap();
    let out = Command::new(jet()).arg(&file).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "jet <file> sugar should exit 0; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("file-sugar"),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn file_sugar_ext_optional() {
    let stem = std::env::temp_dir().join("jet_cli_file_sugar_extopt");
    let file = stem.with_extension("jet");
    fs::write(&file, "fn run() {\n    print(\"ext-sugar\");\n}\n").unwrap();
    let out = Command::new(jet()).arg(&stem).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "jet <stem> sugar should resolve .jet; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("ext-sugar"),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn file_sugar_missing_jet_file_errors() {
    let missing = std::env::temp_dir().join("jet_cli_file_sugar_absent.jet");
    let _ = fs::remove_file(&missing);
    let out = Command::new(jet())
        .arg(&missing)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_ne!(out.status.code(), Some(0));
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(
        combined.contains("jet_cli_file_sugar_absent"),
        "missing file should be named in output: {combined}"
    );
}

#[test]
fn did_you_mean_golden() {
    let out = Command::new(jet())
        .arg("buld")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    check_snapshot("did_you_mean.txt", &stderr);
}

#[test]
fn unknown_flag_is_e2102() {
    let p = std::env::temp_dir().join("jet_cli_ok2.jet");
    fs::write(&p, "fn run() {\n    print(\"hi\");\n}\n").unwrap();
    let out = Command::new(jet())
        .arg("check")
        .arg(&p)
        .arg("--jsn")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "unknown flag should exit 2");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E2102"), "should cite E2102:\n{}", stderr);
    assert!(
        stderr.contains("--json"),
        "should suggest --json:\n{}",
        stderr
    );
}

#[test]
fn doctor_ok_golden() {
    // On a CI/dev box rustc is present; the report is deterministic except for
    // machine-specific paths and the rustc version, which we scrub.
    let out = Command::new(jet())
        .args(["self", "doctor"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&out.stdout).into_owned();
    // Doctor must never emit ANSI when piped.
    assert!(
        !s.contains('\x1b'),
        "doctor output must be ANSI-free when piped"
    );
    // Structural assertions (a full golden would be machine-specific).
    assert!(s.contains("doctor"), "missing header:\n{}", s);
    assert!(s.contains("rustc"), "missing rustc check:\n{}", s);
    assert!(s.contains("pkg-config"), "missing C-FFI section:\n{}", s);
    assert!(s.contains("hangar"), "missing hangar check:\n{}", s);
}

#[test]
fn doctor_failure_is_l2101_snapshot() {
    let out = Command::new(jet())
        .args(["self", "doctor"])
        .env("PATH", "")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let start = stdout.find("Warning [L2101]:").expect("L2101 diagnostic");
    check_snapshot("doctor_l2101.txt", &stdout[start..]);
}

#[test]
fn fetch_without_git_is_e1203_snapshot() {
    let dir = isolated_cwd("fetch_no_git");
    fs::write(
        dir.join("pkg.jet"),
        "payload: { name: \"app\", version: \"0.1.0\", jet: \">=0.1.0\", description: \"\", license: \"MIT\" }\npackages: { app: executable }\ndeps: { tool: { git: \"https://example.invalid/tool.git\", tag: \"v1\" } }\n",
    )
    .unwrap();
    let out = Command::new(jet())
        .args(["store", "fetch"])
        .current_dir(&dir)
        .env("PATH", "")
        .env("HOME", &dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "unexpected stderr:\n{stderr}");
    let start = stderr.find("Error [E1203]:").expect("E1203 diagnostic");
    check_snapshot("fetch_no_git_e1203.txt", &stderr[start..]);
}

#[test]
fn bind_missing_header_is_e3208() {
    let missing = std::env::temp_dir().join("jet_missing_bind_header.h");
    let _ = fs::remove_file(&missing);
    let out = Command::new(jet())
        .args(["inspect", "bind"])
        .arg(&missing)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "unexpected stderr:\n{stderr}");
    assert!(stderr.contains("Error [E3208]:"), "missing bind diagnostic:\n{stderr}");
    assert!(stderr.contains("Why:"), "missing E3208 reason:\n{stderr}");
    assert!(stderr.contains("Fix:"), "missing E3208 fix:\n{stderr}");
    check_snapshot("bind_missing_e3208.txt", &scrub(&stderr, &missing));
}

#[test]
fn unknown_cross_target_is_e3302() {
    let src = std::env::temp_dir().join("jet_unknown_cross_target.jet");
    fs::write(&src, "fn run() { print(\"target\") }\n").unwrap();
    let out = Command::new(jet())
        .arg("build")
        .arg(&src)
        .arg("--target=definitely-not-a-rust-target")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "unexpected stderr:\n{stderr}");
    assert!(stderr.contains("Error [E3302]:"), "missing target diagnostic:\n{stderr}");
    assert!(stderr.contains("Why:"), "missing E3302 reason:\n{stderr}");
    assert!(stderr.contains("Fix:"), "missing E3302 fix:\n{stderr}");
    check_snapshot("unknown_target_e3302.txt", &stderr);
}

#[test]
fn completions_generate_for_every_shell() {
    for shell in ["bash", "zsh", "fish", "powershell"] {
        let out = Command::new(jet())
            .args(["self", "completions"])
            .arg(shell)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "completions {} should exit 0",
            shell
        );
        let s = String::from_utf8_lossy(&out.stdout);
        for flag in ["--structural", "--out", "--report", "--repo"] {
            assert!(s.contains(flag), "{shell} completion missing {flag}");
        }
        check_snapshot(&format!("completions_{}.txt", shell), &s);
    }
}

#[test]
fn man_page_golden() {
    let out = Command::new(jet()).args(["self", "man"]).output().unwrap();
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    // Scrub the version so the snapshot is stable across releases.
    s = s.replace(env!("CARGO_PKG_VERSION"), "VERSION");
    for flag in ["--structural", "--out", "--report", "--repo"] {
        assert!(s.contains(flag), "man page missing {flag}");
    }
    check_snapshot("man.txt", &s);
}

#[test]
fn fix_dry_run_does_not_write() {
    // A file with an autofixable diagnostic. S14 teaching fixes are paused, so
    // use the still-live Core habit fix (`println` -> `print`).
    let p = std::env::temp_dir().join("jet_cli_fix.jet");
    let original = "fn run() {\n    println(\"hi\")\n}\n";
    fs::write(&p, original).unwrap();
    let out = Command::new(jet())
        .arg("fix")
        .arg(&p)
        .arg("--dry-run")
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("dry run"), "dry-run should say so:\n{}", s);
    assert!(s.contains("print"), "diff should show the fix:\n{}", s);
    // The file on disk is unchanged.
    assert_eq!(
        fs::read_to_string(&p).unwrap(),
        original,
        "dry-run must not write"
    );

    // And a real fix DOES write.
    let out2 = Command::new(jet()).arg("fix").arg(&p).output().unwrap();
    assert_eq!(out2.status.code(), Some(0));
    assert!(
        fs::read_to_string(&p).unwrap().contains("print(\"hi\")"),
        "fix should rewrite the file"
    );
}

#[test]
fn external_subcommand_is_discovered() {
    // A fake `jet-greet` on a temp PATH should be invokable as `jet greet`.
    let dir = std::env::temp_dir().join("jet_ext_test_bin");
    fs::create_dir_all(&dir).unwrap();
    let script = dir.join("jet-greet");
    fs::write(&script, "#!/bin/sh\necho \"hi from plugin $1\"\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = fs::metadata(&script).unwrap().permissions();
        perm.set_mode(0o755);
        fs::set_permissions(&script, perm).unwrap();
    }
    let path = format!(
        "{}:{}",
        dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = Command::new(jet())
        .arg("greet")
        .arg("world")
        .env("PATH", path)
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        s.contains("hi from plugin world"),
        "external subcommand not forwarded:\n{}",
        s
    );
}

#[test]
fn osc8_hyperlinks_only_when_forced_on() {
    let p = bad_file(&line!().to_string());
    // Piped + NO_COLOR: never an OSC 8 link (existing snapshots stay clean).
    let piped = Command::new(jet())
        .arg("check")
        .arg(&p)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&piped.stderr);
    assert!(
        !s.contains("\x1b]8;;"),
        "piped output must have no OSC 8 links:\n{:?}",
        s
    );
    // The hyperlink layer is gated behind a real TTY; since tests run piped,
    // we exercise the renderer directly to prove the escape appears when asked.
    let src = "fn run() {}\n";
    let d = jet::Diagnostics::Diagnostic::error(
        "E0001",
        "x".into(),
        "y".into(),
        "z".into(),
        Some(jet::Diagnostics::Span::new(3, 7)),
    );
    let linked = d.render_linked("a.jet", src, true, true);
    assert!(
        linked.contains("\x1b]8;;"),
        "render_linked(hyperlinks=true) should emit OSC 8"
    );
    let plain = d.render_linked("a.jet", src, true, false);
    assert!(
        !plain.contains("\x1b]8;;"),
        "render_linked(hyperlinks=false) must not"
    );
}

// ── Ext-optional CLI (no syntax decision; pure CLI behavior) ──────────

#[test]
fn ext_optional_check_resolves_dot_jet() {
    // `jet check <path-without-.jet>` resolves to `<path>.jet` when the bare
    // path does not exist but the .jet file does.
    let stem = std::env::temp_dir().join("jet_cli_extopt_check");
    let file = stem.with_extension("jet");
    fs::write(&file, "fn run() {\n    print(\"ok\");\n}\n").unwrap();
    let out = Command::new(jet())
        .arg("check")
        .arg(&stem)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "ext-optional check should resolve {}.jet and exit 0; stderr: {}",
        stem.display(),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn ext_optional_run_resolves_dot_jet() {
    // Same resolution for `jet run`.
    let stem = std::env::temp_dir().join("jet_cli_extopt_run");
    let file = stem.with_extension("jet");
    fs::write(&file, "fn run() {\n    print(\"hello-extopt\");\n}\n").unwrap();
    let out = Command::new(jet()).arg("run").arg(&stem).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "ext-optional run should exit 0; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("hello-extopt"),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn ext_optional_missing_path_keeps_original_name() {
    // Neither `<path>` nor `<path>.jet` exists: the original name must surface
    // in the file-not-found error (resolution returns it unchanged).
    let stem = std::env::temp_dir().join("jet_cli_extopt_absent_xyz");
    let out = Command::new(jet())
        .arg("check")
        .arg(&stem)
        .output()
        .unwrap();
    assert_ne!(out.status.code(), Some(0));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("jet_cli_extopt_absent_xyz"),
        "error should name the original path; stderr: {err}"
    );
}

// ── D-ILE1: implicit executable inference (no pkg.jet) ───────────────

#[test]
fn simple_exec_runs_without_a_manifest() {
    // A single file with a top-level `fn run` and no pkg.jet runs as an
    // executable with zero ceremony (R9 / D-ILE1).
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/simple_exec/main.jet");
    // Isolated cwd: this fixture's stem is `main`, a common stem other tests
    // and examples also use — see `isolated_cwd`.
    let out = Command::new(jet())
        .arg("run")
        .arg(&path)
        .current_dir(isolated_cwd("simple_exec"))
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("simple exec, no manifest"),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

// ── D-CLI1: `--` separator passthrough (c11) ──────────────────────────────

/// Write a Jet fixture that prints its argument count via `io.args()`.
fn args_fixture(tag: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("jet_cli_args_{tag}.jet"));
    fs::write(
        &p,
        "use core.io as io\nfn run() {\n    args :: io.args()\n    print(args.len())\n}\n",
    )
    .unwrap();
    p
}

#[test]
fn passthrough_forwards_tokens_after_separator() {
    // `jet run file.jet -- --port 8080 x` — program sees 4 args: argv[0] +
    // three forwarded tokens. io.args().len() == 4.
    let p = args_fixture(&line!().to_string());
    let out = Command::new(jet())
        .args(["run", p.to_str().unwrap(), "--", "--port", "8080", "x"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.trim() == "4",
        "expected 4 args (argv[0] + 3 forwarded), got: {stdout}"
    );
}

#[test]
fn bare_separator_gives_empty_passthrough() {
    // `jet run file.jet --` — bare `--` with nothing after; program sees 1 arg.
    let p = args_fixture(&line!().to_string());
    let out = Command::new(jet())
        .args(["run", p.to_str().unwrap(), "--"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.trim() == "1",
        "expected 1 arg (just argv[0]), got: {stdout}"
    );
}

#[test]
fn no_separator_positional_regression() {
    // Plain positional words with no `--` still reach the program (regression
    // guard). `jet run file.jet hello` → len == 2.
    let p = args_fixture(&line!().to_string());
    let out = Command::new(jet())
        .args(["run", p.to_str().unwrap(), "hello"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.trim() == "2",
        "expected 2 args (argv[0] + hello), got: {stdout}"
    );
}

#[test]
fn unknown_flag_before_separator_is_e2102_with_passthrough_hint() {
    // `jet run file.jet --port` (no `--`) — unknown flag before `--` is E2102
    // and the Fix line teaches the `--` form (D-CLI1=A).
    let p = args_fixture(&line!().to_string());
    let out = Command::new(jet())
        .args(["run", p.to_str().unwrap(), "--port"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "unknown flag before -- should exit 2"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E2102"), "should cite E2102:\n{stderr}");
    assert!(
        stderr.contains("--"),
        "Fix should mention `--` separator:\n{stderr}"
    );
}

// ── D-BUILDPROFILE1: --release / --profile=<name> ─────────────────────────────

#[test]
fn profile_unknown_name_emits_e1219() {
    // D-BUILDPROFILE1: `--profile=<unknown>` with no pkg.jet defining that name
    // must emit E1219 and exit 1 (user error).
    let p = std::env::temp_dir().join("jet_cli_profile_test.jet");
    fs::write(&p, "fn run() { print(\"ok\") }\n").unwrap();
    let out = Command::new(jet())
        .args(["build", p.to_str().unwrap(), "--profile=staging"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "unknown --profile should exit 1 (user error)"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E1219"),
        "unknown profile should cite E1219:\n{stderr}"
    );
    assert!(
        stderr.contains("staging"),
        "E1219 should name the unknown profile:\n{stderr}"
    );
}

#[test]
fn profile_release_flag_is_accepted() {
    // `--release` is valid (blessed profile) and must not emit E1219.
    let p = std::env::temp_dir().join("jet_cli_release_test.jet");
    fs::write(&p, "fn run() { print(\"ok\") }\n").unwrap();
    // We can't guarantee rustc is in PATH for the binary build, but `jet check`
    // doesn't accept --release yet, so test that `jet build --release` at least
    // doesn't emit E1219. We check that the exit code is NOT 1-with-E1219.
    let out = Command::new(jet())
        .args(["build", p.to_str().unwrap(), "--release"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("E1219"),
        "--release must not emit E1219 (it's a blessed profile):\n{stderr}"
    );
}

#[test]
fn profile_ci_flag_is_accepted() {
    let p = std::env::temp_dir().join("jet_cli_ci_test.jet");
    fs::write(&p, "fn run() { print(\"ok\") }\n").unwrap();
    let out = Command::new(jet())
        .args(["build", p.to_str().unwrap(), "--profile=ci"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("E1219"),
        "--profile=ci must not emit E1219 (it's a blessed profile):\n{stderr}"
    );
}

#[test]
fn profile_custom_name_from_pkg_jet() {
    let dir = std::env::temp_dir().join(format!(
        "jet_cli_custom_profile_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("pkg.jet"),
        r#"payload: { name: "p", version: "0.1.0" }
build: { staging: Build.{ optimize: basic } }
"#,
    )
    .unwrap();
    let main = dir.join("main.jet");
    fs::write(&main, "fn run() { print(\"ok\") }\n").unwrap();
    // Isolated cwd: this fixture's stem is `main` — see `isolated_cwd`. Also
    // the semantically correct place for `build/` to land, since it's this
    // fixture's own project directory.
    let out = Command::new(jet())
        .args(["build", main.to_str().unwrap(), "--profile=staging"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("E1219"),
        "pkg.jet-defined profile must resolve:\n{stderr}"
    );
}

// ── D-EXPANDCLI1 (card #183): `jet inspect expand` transparency command ────

/// Fixture exercising the `inline` lens: an `@Inline` fn and an
/// `@InlineAlways` method.
fn expand_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/expand_facts.jet")
}

/// Replace the fixture's machine-specific absolute path with a stable token.
fn scrub_fixture(s: &str, fixture: &Path) -> String {
    s.replace(&fixture.display().to_string(), "FIXTURE.jet")
}

#[test]
fn expand_inline_golden() {
    let p = expand_fixture();
    let out = Command::new(jet())
        .args(["inspect", "expand", "--facts", "inline"])
        .arg(&p)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "expand --facts inline should exit 0:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = scrub_fixture(&String::from_utf8_lossy(&out.stdout), &p);
    check_snapshot("expand_inline.txt", &s);
}

#[test]
fn expand_all_golden() {
    let p = expand_fixture();
    // Bare `jet inspect expand <file>`: every lens, grouped, magic default.
    let out = Command::new(jet())
        .args(["inspect", "expand"])
        .arg(&p)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "bare expand should exit 0:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = scrub_fixture(&String::from_utf8_lossy(&out.stdout), &p);
    check_snapshot("expand_all.txt", &s);
}

#[test]
fn expand_unknown_lens_golden() {
    let p = expand_fixture();
    let out = Command::new(jet())
        .args(["inspect", "expand", "--facts", "bogus"])
        .arg(&p)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "unknown lens should exit 1 (USER_ERROR), listing available lenses"
    );
    let s = scrub_fixture(&String::from_utf8_lossy(&out.stderr), &p);
    check_snapshot("expand_unknown_lens.txt", &s);
}

#[test]
fn expand_missing_file_is_user_error() {
    let out = Command::new(jet()).args(["inspect", "expand"]).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "missing entry file is USER_ERROR"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("needs an entry file"),
        "should explain the missing file:\n{}",
        stderr
    );
}

#[test]
fn expand_compile_error_reports_ordinary_diagnostics() {
    let p = bad_file(&line!().to_string());
    let out = Command::new(jet())
        .args(["inspect", "expand"])
        .arg(&p)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "a program that fails to compile can't print facts (USER_ERROR)"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E0102"),
        "should render the ordinary front-end diagnostic:\n{}",
        stderr
    );
}

// ── D-JPK-FILENAME2=B (A2): retired manifest filenames → E1226 ──────

#[test]
fn stale_manifest_name_pack_jet_is_e1226() {
    let dir = isolated_cwd("stale_pack_jet");
    fs::write(
        dir.join("pack.jet"),
        "payload: { name: \"x\", version: \"0.1.0\" }\n",
    )
    .unwrap();
    let out = Command::new(jet())
        .arg("add")
        .arg("dep")
        .arg("--path")
        .arg("../dep")
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E1226"),
        "expected E1226 in stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("pack.jet"),
        "names the found file:\n{stderr}"
    );
    assert!(
        stderr.contains("pkg.jet"),
        "names the fix target:\n{stderr}"
    );
}

#[test]
fn stale_manifest_name_jet_toml_is_e1226() {
    let dir = isolated_cwd("stale_jet_toml");
    fs::write(dir.join("jet.toml"), "").unwrap();
    let out = Command::new(jet())
        .arg("build")
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E1226"),
        "expected E1226 in stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("jet.toml"),
        "names the found file:\n{stderr}"
    );
}

#[test]
fn stale_manifest_name_payload_jet_is_e1226() {
    let dir = isolated_cwd("stale_payload_jet");
    fs::write(dir.join("payload.jet"), "").unwrap();
    let out = Command::new(jet())
        .args(["inspect", "schema"])
        .arg("status")
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E1226"),
        "expected E1226 in stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("payload.jet"),
        "names the found file:\n{stderr}"
    );
}

/// `jetpack.toml` is a different, still-live file (D-JPK-FILES repo
/// metadata) — it must NOT be mistaken for a retired manifest name.
#[test]
fn jetpack_toml_alone_is_not_e1226() {
    let dir = isolated_cwd("jetpacktoml_not_stale");
    fs::write(dir.join("jetpack.toml"), "[repo]\nname = \"x\"\n").unwrap();
    let out = Command::new(jet())
        .arg("build")
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("E1226"),
        "jetpack.toml is a different live file, not a retired manifest name:\n{stderr}"
    );
    assert!(
        stderr.contains("no file given and no `pkg.jet` found") || stderr.contains("E1225"),
        "should fall back to the generic no-manifest message:\n{stderr}"
    );
}

/// D-PLUGIN1=B (c81): a `target: plugin` package is deny-by-default — its own
/// code using any effect (here `core.env`) must fail cleanly at build time
/// (E1258), not defer to a runtime instantiation failure. This check lives in
/// the CLI's post-compile effect-budget pass (`Source/CmdCompile.rs`), so it
/// needs the real subprocess (not the `jet::compile_plugin` library call the
/// `tests/ui` `@plugin_target` harness drives).
#[test]
fn plugin_using_an_effect_is_e1258() {
    let dir = isolated_cwd("plugin_effect_denied");
    fs::write(
        dir.join("main.jet"),
        "use core.env as env\n\npub fn get_secret() -> Int {\n    _ :: env.get(\"SECRET\")\n    return 1\n}\n",
    )
    .unwrap();
    let out = Command::new(jet())
        .arg("build")
        .arg("main.jet")
        .arg("--target=plugin")
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E1258"),
        "expected E1258 (plugin capability-denied) in stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("Env"),
        "should name the offending effect:\n{stderr}"
    );
}

/// D-DEP-WASM1=A (c81): `jet build --target=plugin` shells out to
/// `wasm-tools` to lift the rustc-built core wasm module into a Component. A
/// PATH without `wasm-tools` on it (but with `rustc` still reachable, so the
/// core-module half of the build succeeds) must fail as a clean E1259, never
/// a raw "No such file or directory" panic (I2).
#[test]
fn plugin_missing_wasm_tools_is_e1259() {
    let which = |tool: &str| -> Option<String> {
        Command::new("which")
            .arg(tool)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let (Some(rustc_path), Some(lld_path)) = (which("rustc"), which("lld")) else {
        eprintln!("note: skipping plugin_missing_wasm_tools_is_e1259 (no `rustc`/`lld` on PATH to re-expose)");
        return;
    };

    let dir = isolated_cwd("plugin_no_wasmtools");
    fs::write(
        dir.join("main.jet"),
        "pub fn scale(a: Float, b: Float) -> Float {\n    return a * b\n}\n",
    )
    .unwrap();

    // A minimal PATH exposing only `rustc` + `lld` (via symlinks), so the
    // core-wasm-module half of the build still works but `wasm-tools`
    // resolves to nothing.
    let bin_dir = dir.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let _ = symlink(&rustc_path, bin_dir.join("rustc"));
        let _ = symlink(&lld_path, bin_dir.join("lld"));
    }

    let out = Command::new(jet())
        .arg("build")
        .arg("main.jet")
        .arg("--target=plugin")
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .env("PATH", &bin_dir)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E1259"),
        "expected E1259 (missing wasm-tools) in stderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("panicked"),
        "must never panic, only report a clean diagnostic (I2):\n{stderr}"
    );
}
