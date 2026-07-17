//! D-JPK-TASKRUN1 (card #476): `jetpack run <task>` discovers `@Task fn`s.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

mod common;
use common::jetpack_bin;

fn jet() -> Command {
    Command::new(env!("CARGO_BIN_EXE_jet"))
}

fn jetpack() -> Command {
    Command::new(jetpack_bin())
}

struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "jpk-task-{tag}-{nanos}-{:?}",
            std::thread::current().id()
        ));
        fs::create_dir_all(&path).unwrap();
        Scratch { path }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn write_main(dir: &std::path::Path, src: &str) {
    fs::write(dir.join("main.jet"), src).unwrap();
}

#[test]
fn jet_run_task_invokes_marked_fn() {
    let scratch = Scratch::new("jet-task");
    write_main(
        &scratch.path,
        r#"
@Task
fn greet() {
    print("hello-task")
}
fn run() { print("run-entry") }
"#,
    );
    let entry = scratch.path.join("main.jet");
    let out = jet()
        .args(["run", "--task=greet", entry.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("hello-task"),
        "stdout: {stdout}"
    );
    assert!(
        !stdout.contains("run-entry"),
        "must not run fn run when --task is set: {stdout}"
    );
}

#[test]
fn jetpack_run_task_and_unknown_lists_declared() {
    let scratch = Scratch::new("jpk-task");
    write_main(
        &scratch.path,
        r#"
@Task
fn greet() {
    print("from-jetpack")
}
@Task
fn seed() {
    print("seeded")
}
fn run() {}
"#,
    );

    let ok = jetpack()
        .args(["run", "greet", "--no-color"])
        .current_dir(&scratch.path)
        .env("PATH", format!("{}:{}", env!("CARGO_BIN_EXE_jet").rsplit_once('/').unwrap().0, std::env::var("PATH").unwrap_or_default()))
        .output()
        .unwrap();
    assert!(
        ok.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&ok.stderr)
    );
    assert!(
        String::from_utf8_lossy(&ok.stdout).contains("from-jetpack"),
        "stdout: {}",
        String::from_utf8_lossy(&ok.stdout)
    );

    let bad = jetpack()
        .args(["run", "deploy", "--no-color"])
        .current_dir(&scratch.path)
        .env("PATH", format!("{}:{}", env!("CARGO_BIN_EXE_jet").rsplit_once('/').unwrap().0, std::env::var("PATH").unwrap_or_default()))
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&bad.stderr);
    assert!(stderr.contains("E1294"), "stderr: {stderr}");
    assert!(stderr.contains("no task named `deploy`"), "stderr: {stderr}");
    assert!(
        stderr.contains("declared tasks:") && stderr.contains("greet") && stderr.contains("seed"),
        "stderr: {stderr}"
    );
}

#[test]
fn jet_run_task_leaf_stays_callable_from_sibling() {
    // D-JPK-TASKRUN1: dependency = plain call. Selecting leaf `greet` as
    // entry must not rename it away — sibling `seed`'s `greet()` must not
    // die with E0102, and both tasks must run.
    let scratch = Scratch::new("task-dep");
    write_main(
        &scratch.path,
        r#"
@Task
fn greet() {
    print("leaf-ok")
}
@Task
fn seed() {
    greet()
    print("dep-ok")
}
fn run() { print("run-entry") }
"#,
    );
    let entry = scratch.path.join("main.jet");
    let path = entry.to_str().unwrap();

    let greet = jet()
        .args(["run", "--task=greet", path])
        .output()
        .unwrap();
    assert!(
        greet.status.success(),
        "leaf --task=greet must not E0102; stderr: {}",
        String::from_utf8_lossy(&greet.stderr)
    );
    let greet_out = String::from_utf8_lossy(&greet.stdout);
    assert!(
        greet_out.contains("leaf-ok"),
        "stdout: {greet_out}"
    );
    assert!(
        !String::from_utf8_lossy(&greet.stderr).contains("E0102"),
        "stderr: {}",
        String::from_utf8_lossy(&greet.stderr)
    );

    let seed = jet()
        .args(["run", "--task=seed", path])
        .output()
        .unwrap();
    assert!(
        seed.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&seed.stderr)
    );
    let seed_out = String::from_utf8_lossy(&seed.stdout);
    assert!(
        seed_out.contains("leaf-ok") && seed_out.contains("dep-ok"),
        "stdout: {seed_out}"
    );
}

#[test]
fn jet_run_task_typed_cli_args() {
    let scratch = Scratch::new("task-cli");
    write_main(
        &scratch.path,
        r#"
@Cli
struct MigrateArgs {
    @[Doc("target"), Default("latest")] to: String
}
@Task
fn migrate(args: MigrateArgs) {
    print(args.to)
}
fn run() {}
"#,
    );
    let entry = scratch.path.join("main.jet");
    let out = jet()
        .args([
            "run",
            "--task=migrate",
            entry.to_str().unwrap(),
            "--",
            "--to",
            "004",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "004");
}
