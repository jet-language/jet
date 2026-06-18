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

/// Write a tiny source file with a known error and return its path. Each test
/// passes a unique `tag` so concurrent tests never share a path — `fs::write`
/// truncates-then-writes, so a shared path would let one test's write race a
/// sibling's `jet check` read (seeing a momentarily-empty file).
fn bad_file(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("jet_cli_bad_{tag}.jet"));
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
    let p = bad_file(&line!().to_string());
    let out = Command::new(jet()).arg("check").arg(&p).output().unwrap();
    assert_eq!(out.status.code(), Some(1), "a program error should exit 1");
}

#[test]
fn exit_code_no_args_greets() {
    // E2-M3 Wave B: bare `jet` greets and orients — it is NOT a usage error.
    let out = Command::new(jet()).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "no args should greet (exit 0)");
}

#[test]
fn exit_code_unknown_subcommand_is_usage() {
    // A typo'd subcommand is a usage error (exit 2) and teaches E2101.
    let out = Command::new(jet()).arg("buld").output().unwrap();
    assert_eq!(out.status.code(), Some(2), "unknown subcommand should exit 2");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E2101"), "should cite E2101:\n{}", stderr);
    assert!(stderr.contains("build"), "should suggest `build`:\n{}", stderr);
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

// ── Wave B: greeting, did-you-mean, doctor, completions, fix, externals ──

#[test]
fn no_args_greeting_golden() {
    let out = Command::new(jet()).env("NO_COLOR", "1").output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    check_snapshot("no_args_greeting.txt", &stdout);
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
    fs::write(&p, "fn main() {\n    print(\"hi\");\n}\n").unwrap();
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
    assert!(stderr.contains("--json"), "should suggest --json:\n{}", stderr);
}

#[test]
fn doctor_ok_golden() {
    // On a CI/dev box rustc is present; the report is deterministic except for
    // machine-specific paths and the rustc version, which we scrub.
    let out = Command::new(jet())
        .arg("doctor")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&out.stdout).into_owned();
    // Doctor must never emit ANSI when piped.
    assert!(!s.contains('\x1b'), "doctor output must be ANSI-free when piped");
    // Structural assertions (a full golden would be machine-specific).
    assert!(s.contains("doctor"), "missing header:\n{}", s);
    assert!(s.contains("rustc"), "missing rustc check:\n{}", s);
    assert!(s.contains("pkg-config"), "missing C-FFI section:\n{}", s);
    assert!(s.contains("hangar"), "missing hangar check:\n{}", s);
}

#[test]
fn completions_generate_for_every_shell() {
    for shell in ["bash", "zsh", "fish"] {
        let out = Command::new(jet())
            .arg("completions")
            .arg(shell)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "completions {} should exit 0", shell);
        let s = String::from_utf8_lossy(&out.stdout);
        check_snapshot(&format!("completions_{}.txt", shell), &s);
    }
}

#[test]
fn man_page_golden() {
    let out = Command::new(jet()).arg("man").output().unwrap();
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    // Scrub the version so the snapshot is stable across releases.
    s = s.replace(env!("CARGO_PKG_VERSION"), "VERSION");
    check_snapshot("man.txt", &s);
}

#[test]
fn fix_dry_run_does_not_write() {
    // A file with an autofixable diagnostic: the S14 teaching error E0045
    // (`or` → `??`) carries a machine-applicable `replace` edit, and the parser
    // recovers so sema still runs.
    let p = std::env::temp_dir().join("jet_cli_fix.jet");
    let original = "fn main() {\n    val x: Int? = none;\n    print(x or 0);\n}\n";
    fs::write(&p, original).unwrap();
    let out = Command::new(jet())
        .arg("fix")
        .arg(&p)
        .arg("--dry-run")
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("dry run"), "dry-run should say so:\n{}", s);
    assert!(s.contains("??"), "diff should show the fix:\n{}", s);
    // The file on disk is unchanged.
    assert_eq!(fs::read_to_string(&p).unwrap(), original, "dry-run must not write");

    // And a real fix DOES write.
    let out2 = Command::new(jet()).arg("fix").arg(&p).output().unwrap();
    assert_eq!(out2.status.code(), Some(0));
    assert!(fs::read_to_string(&p).unwrap().contains("x ?? 0"), "fix should rewrite the file");
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
    assert!(s.contains("hi from plugin world"), "external subcommand not forwarded:\n{}", s);
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
    assert!(!s.contains("\x1b]8;;"), "piped output must have no OSC 8 links:\n{:?}", s);
    // The hyperlink layer is gated behind a real TTY; since tests run piped,
    // we exercise the renderer directly to prove the escape appears when asked.
    let src = "fn main() {}\n";
    let d = jet::diag::Diagnostic::error(
        "E0001",
        "x".into(),
        "y".into(),
        "z".into(),
        Some(jet::diag::Span::new(3, 7)),
    );
    let linked = d.render_linked("a.jet", src, true, true);
    assert!(linked.contains("\x1b]8;;"), "render_linked(hyperlinks=true) should emit OSC 8");
    let plain = d.render_linked("a.jet", src, true, false);
    assert!(!plain.contains("\x1b]8;;"), "render_linked(hyperlinks=false) must not");
}

// ── Ext-optional CLI (no syntax decision; pure CLI behavior) ──────────

#[test]
fn ext_optional_check_resolves_dot_jet() {
    // `jet check <path-without-.jet>` resolves to `<path>.jet` when the bare
    // path does not exist but the .jet file does.
    let stem = std::env::temp_dir().join("jet_cli_extopt_check");
    let file = stem.with_extension("jet");
    fs::write(&file, "fn main() {\n    print(\"ok\");\n}\n").unwrap();
    let out = Command::new(jet()).arg("check").arg(&stem).output().unwrap();
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
    fs::write(&file, "fn main() {\n    print(\"hello-extopt\");\n}\n").unwrap();
    let out = Command::new(jet()).arg("run").arg(&stem).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "ext-optional run should exit 0; stderr: {}", String::from_utf8_lossy(&out.stderr));
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
    let out = Command::new(jet()).arg("check").arg(&stem).output().unwrap();
    assert_ne!(out.status.code(), Some(0));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("jet_cli_extopt_absent_xyz"),
        "error should name the original path; stderr: {err}"
    );
}
