//! D-FMTPROJECT1=D — integration tests for the project-level `jet fmt` command.
//!
//! Covers:
//!   - no-arg discovery (format all .jet under project root)
//!   - explicit file and directory arguments
//!   - `--check`: exit 1 when files would change, exit 0 when clean
//!   - `--check --diff`: prints unified diffs
//!   - `--dry-run`: prints unified diffs without writing
//!   - `--changed` outside a git repo: exit 2 with a diagnostic
//!   - `jet fmt -` stdin mode (including `--stdin-path`)
//!   - preflight zero-write: one bad file among good ones writes NOTHING
//!   - ignore: `vendor/` and other generated dirs are skipped on discovery
//!   - idempotence: formatting twice equals once
//!   - exit-code table: 0 = clean/formatted, 1 = preview/check dirty, 2 = error

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn jet() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

/// Create an isolated temp directory for a test.
fn tmpdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("jet_fmt_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// A well-formed .jet file that needs formatting (extra spaces).
const UNFORMATTED: &str = "fn run()  {\n  print( \"hi\" );\n}\n";

/// A well-formed, already-canonical .jet file. Formatting it again must be a no-op.
/// D-FMTCOLLAPSE1 keeps a fitting simple body on one line.
const CANONICAL: &str = "fn run() { print(\"hi\") }\n";

/// A source file with a deliberate parse error.
const INVALID: &str = "fn run( {\n";

// ── helpers ──────────────────────────────────────────────────────────────────

fn write(dir: &Path, rel: &str, content: &str) -> PathBuf {
    let p = dir.join(rel);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&p, content).unwrap();
    p
}

fn read(p: &Path) -> String {
    fs::read_to_string(p).unwrap()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// `jet fmt` with no args from a directory containing .jet files formats them.
#[test]
fn no_arg_discovery_formats_files() {
    let dir = tmpdir(&line!().to_string());
    let f = write(&dir, "main.jet", UNFORMATTED);

    let out = Command::new(jet())
        .arg("fmt")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "fmt with no args should exit 0\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let after = read(&f);
    assert_ne!(after, UNFORMATTED, "unformatted file should have been rewritten");
}

/// `jet fmt <dir>` formats .jet files found in the given directory.
#[test]
fn explicit_dir_arg_formats_files() {
    let dir = tmpdir(&line!().to_string());
    let sub = dir.join("src");
    fs::create_dir_all(&sub).unwrap();
    let f = write(&sub, "foo.jet", UNFORMATTED);

    let out = Command::new(jet())
        .arg("fmt")
        .arg(&sub)
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_ne!(read(&f), UNFORMATTED);
}

/// Directory traversal validates `package.jet`; an explicit path stays intentional.
#[test]
fn explicit_dir_reports_invalid_package_manifest() {
    let dir = tmpdir(&line!().to_string());
    let source = write(&dir, "src/main.jet", UNFORMATTED);
    let manifest = write(
        &dir,
        jet::Syntax::PACKAGE_FILE,
        "name: \"demo\"\ndeps: {\n    helpers: notaref,\n}\n",
    );
    let manifest_before = fs::read(&manifest).unwrap();

    let check = Command::new(jet())
        .args(["fmt", "--check"])
        .arg(&dir)
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(
        check.status.code(),
        Some(2),
        "invalid package manifest must fail project formatting\nstderr: {}",
        String::from_utf8_lossy(&check.stderr)
    );
    assert_eq!(read(&source), UNFORMATTED);
    assert_eq!(fs::read(&manifest).unwrap(), manifest_before);

    let format = Command::new(jet())
        .arg("fmt")
        .arg(&dir)
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(
        format.status.code(),
        Some(2),
        "directory format should reject invalid package.jet\nstderr: {}",
        String::from_utf8_lossy(&format.stderr)
    );
    assert_eq!(read(&source), UNFORMATTED);
    assert_eq!(fs::read(&manifest).unwrap(), manifest_before);

    let explicit = Command::new(jet())
        .arg("fmt")
        .arg(&manifest)
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(
        explicit.status.code(),
        Some(2),
        "an explicit package.jet path must still reach the formatter"
    );
}

#[test]
fn epoch5_package_and_config_files_use_typed_formatter() {
    let dir = tmpdir(&line!().to_string());
    let package = write(
        &dir,
        jet::Syntax::PACKAGE_FILE,
        "name: \"demo\"\noutputs: .{app: .Executable.{entry: run}}\n",
    );
    let config = write(
        &dir,
        "config/dev.jet",
        "pub dev :: Config.{deps: {ripgrep: \"ripgrep@nixpkgs\"} environments: {dev: Environment.{tools: [\"ripgrep@nixpkgs\"]}}}\n",
    );
    let before_package = read(&package);
    let before_config = read(&config);

    let formatted = Command::new(jet())
        .arg("fmt")
        .arg(&dir)
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(
        formatted.status.code(),
        Some(0),
        "Epoch 5 package/config formatting failed\nstderr: {}",
        String::from_utf8_lossy(&formatted.stderr)
    );
    assert_ne!(read(&package), before_package);
    assert_ne!(read(&config), before_config);

    let clean = Command::new(jet())
        .args(["fmt", "--check"])
        .arg(&dir)
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(
        clean.status.code(),
        Some(0),
        "typed package/config formatter must be idempotent\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&clean.stdout),
        String::from_utf8_lossy(&clean.stderr)
    );
    assert!(
        jet::Package::PackageFacts::parse(&read(&package), "package.jet").is_ok(),
        "formatted package must remain typed"
    );
    assert!(
        jet::Package::ConfigFacts::parse(&read(&config), "config/dev.jet").is_ok(),
        "formatted Config must remain typed"
    );
}

/// `jet fmt <file>` formats a single explicit file.
#[test]
fn explicit_file_arg_formats_file() {
    let dir = tmpdir(&line!().to_string());
    let f = write(&dir, "a.jet", UNFORMATTED);

    let out = Command::new(jet())
        .arg("fmt")
        .arg(&f)
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_ne!(read(&f), UNFORMATTED);
}

/// Already-canonical file: `jet fmt` exits 0 and does not rewrite it.
#[test]
fn already_canonical_no_write() {
    let dir = tmpdir(&line!().to_string());
    let f = write(&dir, "main.jet", CANONICAL);

    let before = read(&f);
    let out = Command::new(jet())
        .arg("fmt")
        .arg(&f)
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(read(&f), before, "canonical file must not be rewritten");
}

/// `jet fmt --check` exits 1 when a file would change.
#[test]
fn check_exits_1_when_dirty() {
    let dir = tmpdir(&line!().to_string());
    let f = write(&dir, "main.jet", UNFORMATTED);
    let before = read(&f);

    let out = Command::new(jet())
        .arg("fmt")
        .arg("--check")
        .arg(&f)
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "--check should exit 1 when file would change\nstdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    // --check must NOT write the file.
    assert_eq!(read(&f), before, "--check must not modify the file");
}

/// `jet fmt --check` exits 0 when no file would change.
#[test]
fn check_exits_0_when_clean() {
    let dir = tmpdir(&line!().to_string());
    let f = write(&dir, "main.jet", CANONICAL);

    let out = Command::new(jet())
        .arg("fmt")
        .arg("--check")
        .arg(&f)
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "--check should exit 0 when no file would change"
    );
}

/// `jet fmt --check` lists the changed path on stdout (sorted).
#[test]
fn check_lists_changed_paths() {
    let dir = tmpdir(&line!().to_string());
    let _f = write(&dir, "main.jet", UNFORMATTED);

    let out = Command::new(jet())
        .arg("fmt")
        .arg("--check")
        .arg("main.jet")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("main.jet"),
        "stdout should list the changed file, got: {}",
        stdout
    );
}

/// `jet fmt --dry-run` prints a unified diff and does not write changed files.
#[test]
fn dry_run_prints_unified_diff() {
    let dir = tmpdir(&line!().to_string());
    let f = write(&dir, "main.jet", UNFORMATTED);
    let before = read(&f);

    let out = Command::new(jet())
        .arg("fmt")
        .arg("--dry-run")
        .arg("main.jet")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    // A unified diff contains `---` or `+++` markers.
    assert!(
        stdout.contains("---") || stdout.contains("+++") || stdout.contains("@@"),
        "expected diff output, got: {}",
        stdout
    );
    assert_eq!(read(&f), before, "--dry-run must not modify the file");
}

/// `jet fmt --changed` outside a git repo exits 2 with a diagnostic.
#[test]
fn changed_outside_git_exits_2() {
    let dir = tmpdir(&line!().to_string());
    write(&dir, "main.jet", CANONICAL);

    let out = Command::new(jet())
        .arg("fmt")
        .arg("--changed")
        .current_dir(&dir)
        // verify-full puts TMPDIR under the checkout. Without a ceiling, git
        // walks through target/test-tmp and discovers the checkout's .git.
        // A single native path is portable: no platform-specific path-list
        // separator is needed.
        .env(
            "GIT_CEILING_DIRECTORIES",
            dir.parent().expect("isolated temp directory has a parent"),
        )
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "--changed outside git should exit 2"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("git"),
        "diagnostic should mention git, got: {}",
        stderr
    );
}

/// `jet fmt - --stdin-path=src/a.jet` reads from stdin and writes formatted source to stdout.
#[test]
fn stdin_mode_formats_and_writes_stdout() {
    let dir = tmpdir(&line!().to_string());

    let mut child = Command::new(jet())
        .arg("fmt")
        .arg("-")
        .arg("--stdin-path=src/a.jet")
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(UNFORMATTED.as_bytes()).unwrap();
    }
    let out = child.wait_with_output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stdin mode should exit 0\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.is_empty(), "stdout should contain formatted source");
}

/// Stdin mode with a parse error exits 2.
#[test]
fn stdin_mode_parse_error_exits_2() {
    let dir = tmpdir(&line!().to_string());

    let mut child = Command::new(jet())
        .arg("fmt")
        .arg("-")
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    {
        use std::io::Write;
        child.stdin.as_mut().unwrap().write_all(INVALID.as_bytes()).unwrap();
    }
    let out = child.wait_with_output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "stdin parse error should exit 2"
    );
}

/// Preflight: ONE bad file among good ones → nothing is written, exit 2.
#[test]
fn preflight_zero_write_on_bad_file() {
    let dir = tmpdir(&line!().to_string());
    let good = write(&dir, "good.jet", UNFORMATTED);
    let bad = write(&dir, "bad.jet", INVALID);

    let before_good = read(&good);
    let before_bad = read(&bad);

    let out = Command::new(jet())
        .arg("fmt")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "preflight failure should exit 2\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // Neither file must be written.
    assert_eq!(read(&good), before_good, "good file must not be written when preflight fails");
    assert_eq!(read(&bad), before_bad, "bad file must not be written");
}

/// Parse error on an explicit file exits 2 (not 1).
#[test]
fn parse_error_exits_2_not_1() {
    let dir = tmpdir(&line!().to_string());
    let f = write(&dir, "bad.jet", INVALID);

    let out = Command::new(jet())
        .arg("fmt")
        .arg(&f)
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "parse error should exit 2, not 1"
    );
}

/// `jet fmt --check` parse errors use the same user-error code as `jet check`.
#[test]
fn check_parse_error_exits_1_like_check() {
    let dir = tmpdir(&line!().to_string());
    let _f = write(&dir, "bad.jet", INVALID);

    let fmt = Command::new(jet())
        .args(["fmt", "--check", "bad.jet"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let check = Command::new(jet())
        .args(["check", "bad.jet"])
        .current_dir(&dir)
        .output()
        .unwrap();

    assert_eq!(fmt.status.code(), Some(jet::ExitCodes::USER_ERROR));
    assert_eq!(check.status.code(), Some(jet::ExitCodes::USER_ERROR));
}

/// `vendor/` and other IGNORED_DIRS are skipped during no-arg discovery.
#[test]
fn ignore_vendor_dir_on_discovery() {
    let dir = tmpdir(&line!().to_string());
    // A bad file inside vendor/ — if it were formatted, preflight would abort.
    write(&dir, "vendor/lib.jet", INVALID);
    // A good file at the root — should still be formatted.
    let good = write(&dir, "main.jet", UNFORMATTED);

    let before = read(&good);
    let out = Command::new(jet())
        .arg("fmt")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "fmt should succeed (vendor/ ignored)\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_ne!(read(&good), before, "root file should have been formatted");
}

/// Other IGNORED_DIRS (`target/`, `build/`, `.git/`, `node_modules/`, `.jet/`) are skipped.
#[test]
fn ignore_other_generated_dirs() {
    let dir = tmpdir(&line!().to_string());
    for ignored in ["target", "build", "node_modules"] {
        write(&dir, &format!("{}/junk.jet", ignored), INVALID);
    }
    let good = write(&dir, "main.jet", UNFORMATTED);

    let out = Command::new(jet())
        .arg("fmt")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "generated dirs should be ignored\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_ne!(read(&good), UNFORMATTED);
}

/// Idempotence: formatting an already-formatted file produces no further changes.
#[test]
fn idempotent_format() {
    let dir = tmpdir(&line!().to_string());
    let f = write(&dir, "main.jet", UNFORMATTED);

    // First pass: format.
    let out1 = Command::new(jet())
        .arg("fmt")
        .arg(&f)
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(out1.status.code(), Some(0));
    let after_first = read(&f);

    // Second pass: must be a no-op.
    let out2 = Command::new(jet())
        .arg("fmt")
        .arg(&f)
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(out2.status.code(), Some(0));
    assert_eq!(
        read(&f),
        after_first,
        "formatting twice must produce the same result"
    );
}

/// `jet fmt --check --json` emits a machine-readable JSON result.
#[test]
fn check_json_output() {
    let dir = tmpdir(&line!().to_string());
    let _f = write(&dir, "main.jet", UNFORMATTED);

    let out = Command::new(jet())
        .args(["fmt", "--check", "--json", "main.jet"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("schema_version") && stdout.contains("dirty"),
        "JSON output should contain schema_version and dirty status, got: {}",
        stdout
    );
}

/// Empty directory: `jet fmt` exits 0 silently (no files to format).
#[test]
fn empty_dir_exits_0() {
    let dir = tmpdir(&line!().to_string());

    let out = Command::new(jet())
        .arg("fmt")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "empty dir should exit 0\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
