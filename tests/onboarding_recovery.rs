//! Beginner onboarding recovery checks through the real `jet` CLI.
//!
//! These tests keep the first-hour path on one source of truth: `jet new`
//! writes `run.jet`, the bare resolver selects it, and explicit file/member
//! targets remain available when recovery needs them.

mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn jet(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(args)
        .current_dir(cwd)
        .env("NO_COLOR", "1")
        .output()
        .unwrap_or_else(|error| panic!("jet {args:?} should start: {error}"))
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn repo_text(rel: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(rel))
        .unwrap_or_else(|error| panic!("read {rel}: {error}"))
}

fn write_package(dir: &Path, name: &str) {
    fs::write(dir.join("package.jet"), format!("name: \"{name}\"\n")).unwrap();
}

#[test]
fn first_hour_scaffold_edit_check_test_and_run_recover() {
    let root = common::Scratch::new("onboarding-scaffold");
    let created = jet(&["new", "hello"], &root.path);
    assert!(
        created.status.success(),
        "jet new failed:\n{}",
        stderr(&created)
    );

    let project = root.join("hello");
    for file in ["package.jet", "run.jet", ".gitignore"] {
        assert!(project.join(file).is_file(), "scaffold is missing {file}");
    }
    let manifest = fs::read_to_string(project.join("package.jet")).unwrap();
    assert!(
        manifest.contains("authority: .{ holds: { allow: [IO, Mem.Alloc, Exec] } }"),
        "scaffold must grant the effects used by its generated run.jet:\n{manifest}"
    );
    let source = fs::read_to_string(project.join("run.jet")).unwrap();
    assert!(source.contains("process.argv()"), "native scaffold must read argv");

    let first_run = jet(&["run"], &project);
    assert!(
        first_run.status.success(),
        "bare jet run failed:\n{}",
        stderr(&first_run)
    );
    assert_eq!(stdout(&first_run), "hello, world\n");

    let explicit_run = jet(&["run", "run.jet"], &project);
    assert!(
        explicit_run.status.success(),
        "explicit jet run failed:\n{}",
        stderr(&explicit_run)
    );
    assert_eq!(stdout(&explicit_run), "hello, world\n");

    let argv_run = jet(&["run", "run.jet", "--", "from-argv"], &project);
    assert!(
        argv_run.status.success(),
        "argv scaffold run failed:\n{}",
        stderr(&argv_run)
    );
    assert_eq!(stdout(&argv_run), "from-argv\n");

    let checked = jet(&["check", "run.jet"], &project);
    assert!(
        checked.status.success(),
        "jet check failed:\n{}",
        stderr(&checked)
    );

    let test = jet(&["test", "run.jet"], &project);
    assert!(test.status.success(), "jet test failed:\n{}", stderr(&test));

    let source_path = project.join("run.jet");
    let edited = source.replace("hello, world", "hello from Jet");
    fs::write(&source_path, &edited).unwrap();
    let edited_run = jet(&["run"], &project);
    assert!(
        edited_run.status.success(),
        "edited jet run failed:\n{}",
        stderr(&edited_run)
    );
    assert_eq!(stdout(&edited_run), "hello from Jet\n");

    fs::write(&source_path, edited.replace("print", "pirnt")).unwrap();
    let invalid = jet(&["check", "run.jet"], &project);
    assert!(
        !invalid.status.success(),
        "invalid source unexpectedly passed check"
    );
    assert!(
        stderr(&invalid).contains("E0102"),
        "missing E0102:\n{}",
        stderr(&invalid)
    );
    assert!(
        stderr(&invalid).contains("Fix:"),
        "diagnostic has no fix:\n{}",
        stderr(&invalid)
    );

    let explanation = jet(&["explain", "E0102"], &project);
    assert!(
        explanation.status.success(),
        "jet explain failed:\n{}",
        stderr(&explanation)
    );
    assert!(
        stdout(&explanation).contains("E0102"),
        "explain omitted its code:\n{}",
        stdout(&explanation)
    );

    fs::write(&source_path, edited).unwrap();
    let recovered = jet(&["run"], &project);
    assert!(
        recovered.status.success(),
        "recovery run failed:\n{}",
        stderr(&recovered)
    );
    assert_eq!(stdout(&recovered), "hello from Jet\n");
}

#[test]
fn entry_recovery_covers_missing_ambiguous_legacy_and_stale_layouts() {
    let missing = common::Scratch::new("onboarding-missing-entry");
    write_package(&missing.path, "missing_entry");
    let missing_run = jet(&["run"], &missing.path);
    assert!(!missing_run.status.success());
    assert!(
        stderr(&missing_run).contains("run.jet"),
        "missing recovery omitted run.jet:\n{}",
        stderr(&missing_run)
    );
    assert!(
        stderr(&missing_run).contains("Fix:"),
        "missing recovery omitted Fix:\n{}",
        stderr(&missing_run)
    );
    fs::write(
        missing.join("run.jet"),
        "fn run() { print(\"restored\") }\n",
    )
    .unwrap();
    let restored = jet(&["run"], &missing.path);
    assert!(
        restored.status.success(),
        "restored entry failed:\n{}",
        stderr(&restored)
    );
    assert_eq!(stdout(&restored), "restored\n");

    let workspace = common::Scratch::new("onboarding-ambiguous-workspace");
    fs::write(
        workspace.join("workspace.jet"),
        "module workspace { members: [\"./packages/one\", \"./packages/two\"] }\n",
    )
    .unwrap();
    for name in ["one", "two"] {
        let package = workspace.join(&format!("packages/{name}"));
        fs::create_dir_all(&package).unwrap();
        write_package(&package, name);
        fs::write(
            package.join("run.jet"),
            format!("fn run() {{ print(\"{name}\") }}\n"),
        )
        .unwrap();
    }
    let ambiguous = jet(&["run"], &workspace.path);
    assert!(
        !ambiguous.status.success(),
        "ambiguous workspace unexpectedly ran"
    );
    assert!(
        stderr(&ambiguous).contains("ambiguous"),
        "ambiguity was not named:\n{}",
        stderr(&ambiguous)
    );
    assert!(
        stderr(&ambiguous).contains("-p"),
        "ambiguity omitted member recovery:\n{}",
        stderr(&ambiguous)
    );
    let selected = jet(&["run", "-p", "one"], &workspace.path);
    assert!(
        selected.status.success(),
        "selected workspace member failed:\n{}",
        stderr(&selected)
    );
    assert_eq!(stdout(&selected), "one\n");

    let legacy = common::Scratch::new("onboarding-legacy-entry");
    write_package(&legacy.path, "legacy_entry");
    fs::write(legacy.join("main.jet"), "fn run() { print(\"legacy\") }\n").unwrap();
    let migrated = jet(&["run"], &legacy.path);
    assert!(
        migrated.status.success(),
        "legacy entry did not recover:\n{}",
        stderr(&migrated)
    );
    assert_eq!(stdout(&migrated), "legacy\n");
    assert!(legacy.join("run.jet").is_file());
    assert!(!legacy.join("main.jet").exists());
    assert!(
        stderr(&migrated).contains("migrated"),
        "migration notice missing:\n{}",
        stderr(&migrated)
    );

    let conflict = common::Scratch::new("onboarding-legacy-conflict");
    write_package(&conflict.path, "legacy_conflict");
    fs::write(
        conflict.join("run.jet"),
        "fn run() { print(\"current\") }\n",
    )
    .unwrap();
    fs::write(conflict.join("main.jet"), "fn run() { print(\"old\") }\n").unwrap();
    let conflict_run = jet(&["run"], &conflict.path);
    assert!(!conflict_run.status.success());
    assert!(
        stderr(&conflict_run).contains("E2105"),
        "conflict lost its CLI diagnostic:\n{}",
        stderr(&conflict_run)
    );
    assert!(
        stderr(&conflict_run).contains("ambiguous project entry"),
        "conflict omitted recovery:\n{}",
        stderr(&conflict_run)
    );
    fs::remove_file(conflict.join("main.jet")).unwrap();
    let conflict_recovered = jet(&["run"], &conflict.path);
    assert!(
        conflict_recovered.status.success(),
        "conflict recovery failed:\n{}",
        stderr(&conflict_recovered)
    );
    assert_eq!(stdout(&conflict_recovered), "current\n");

    let stale = common::Scratch::new("onboarding-stale-manifest");
    fs::write(stale.join("pkg.jet"), "name: \"stale\"\n").unwrap();
    fs::write(stale.join("run.jet"), "fn run() { print(\"stale\") }\n").unwrap();
    let stale_run = jet(&["run"], &stale.path);
    assert!(!stale_run.status.success());
    assert!(
        stderr(&stale_run).contains("E1226"),
        "stale layout lost E1226:\n{}",
        stderr(&stale_run)
    );
    assert!(
        stderr(&stale_run).contains("package.jet"),
        "stale layout omitted package.jet fix:\n{}",
        stderr(&stale_run)
    );
    fs::rename(stale.join("pkg.jet"), stale.join("package.jet")).unwrap();
    let stale_recovered = jet(&["run"], &stale.path);
    assert!(
        stale_recovered.status.success(),
        "stale layout recovery failed:\n{}",
        stderr(&stale_recovered)
    );
    assert_eq!(stdout(&stale_recovered), "stale\n");
}

#[test]
fn install_host_and_offline_failures_have_recovery() {
    let guide = repo_text("docs/first-hour.md");
    let exercises = repo_text("docs/diagnostic-recovery.md");
    for phrase in [
        "x86_64 Linux and x86_64 macOS",
        "platform-specific project track",
        "Installation fails",
        "repeat the install command",
        "Do not continue with a partial install",
        "The first install is offline",
        "reconnect and repeat the install",
        "The host is unsupported",
    ] {
        assert!(
            guide.contains(phrase),
            "first-hour recovery lost {phrase:?}"
        );
    }
    for phrase in [
        "uname -s",
        "uname -m",
        "nix --version",
        "Recover install and host failures",
        "If the install fails because the network is offline",
        "If the install succeeds but `jet` is not found",
        "shell and run `jet version`",
    ] {
        assert!(
            exercises.contains(phrase),
            "diagnostic recovery lost {phrase:?}"
        );
    }

    let missing_tools = common::Scratch::new("onboarding-install-offline");
    let doctor = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["self", "doctor"])
        .current_dir(&missing_tools.path)
        .env("NO_COLOR", "1")
        .env("PATH", &missing_tools.path)
        .output()
        .expect("jet self doctor should start after an install failure");
    let report = stdout(&doctor);
    assert!(
        !doctor.status.success(),
        "missing toolchain unexpectedly passed"
    );
    for phrase in [
        "rustc: not found on PATH",
        "Fix: install Rust",
        "registry: skipped (offline; pass --online to check)",
        "Warning [L2101]",
        "run `jet self doctor` again",
    ] {
        assert!(
            report.contains(phrase),
            "doctor recovery lost {phrase:?}:\n{report}"
        );
    }
}
