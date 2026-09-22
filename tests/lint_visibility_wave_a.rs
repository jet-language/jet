//! Card #3420: execution advisory visibility is separate from enforcement.
//!
//! This fixture keeps a known unused-binding advisory while the program still
//! runs successfully. The command matrix checks/builds expose it, execution
//! hides it by default, and explicit verbosity requests it before `--`.

mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

const SOURCE: &str = "fn run() {\n    unused_binding :: 1\n    print(\"ok\")\n}\n";

fn run_jet(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(args)
        .current_dir(root)
        .env("NO_COLOR", "1")
        .output()
        .unwrap_or_else(|error| panic!("run jet {args:?}: {error}"))
}

fn combined(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn assert_advisory_visible(label: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{label} failed:\n{}",
        combined(output)
    );
    assert!(
        combined(output).contains("[L0101]"),
        "{label} should expose the unused-binding advisory:\n{}",
        combined(output)
    );
}

#[test]
fn check_and_build_show_advisories_while_execution_can_request_them() {
    let scratch = common::Scratch::new("lint-visibility-wave-a");
    fs::write(scratch.join("main.jet"), SOURCE).expect("write lint visibility fixture");

    assert_advisory_visible("check", &run_jet(&scratch.path, &["check", "main.jet"]));
    assert_advisory_visible(
        "build",
        &run_jet(&scratch.path, &["build", "--profile=debug", "main.jet"]),
    );

    let quiet_run = run_jet(&scratch.path, &["run", "main.jet"]);
    assert!(
        quiet_run.status.success(),
        "default run failed:\n{}",
        combined(&quiet_run)
    );
    assert_eq!(
        String::from_utf8_lossy(&quiet_run.stdout),
        "ok\n",
        "application stdout must remain intact"
    );
    assert!(
        !combined(&quiet_run).contains("[L0101]"),
        "default run should hide ordinary advisories:\n{}",
        combined(&quiet_run)
    );

    let verbose_run = run_jet(&scratch.path, &["run", "--verbose", "main.jet"]);
    assert_advisory_visible("verbose run", &verbose_run);
    assert_eq!(String::from_utf8_lossy(&verbose_run.stdout), "ok\n");

    let program_arg_run = run_jet(
        &scratch.path,
        &["run", "main.jet", "--", "--verbose", "--json"],
    );
    assert!(
        program_arg_run.status.success(),
        "program-argument run failed:\n{}",
        combined(&program_arg_run)
    );
    assert_eq!(String::from_utf8_lossy(&program_arg_run.stdout), "ok\n");
    assert!(
        !combined(&program_arg_run).contains("[L0101]"),
        "program arguments after -- must not enable advisories:\n{}",
        combined(&program_arg_run)
    );
}

#[test]
fn dev_watch_off_hides_advisories_by_default_and_verbose_requests_them() {
    let scratch = common::Scratch::new("lint-visibility-wave-a-dev");
    fs::write(scratch.join("main.jet"), SOURCE).expect("write dev lint fixture");

    let quiet_dev = run_jet(&scratch.path, &["dev", "main.jet", "--watch=off"]);
    assert!(
        quiet_dev.status.success(),
        "default dev failed:\n{}",
        combined(&quiet_dev)
    );
    assert!(
        !combined(&quiet_dev).contains("[L0101]"),
        "default dev should hide ordinary advisories:\n{}",
        combined(&quiet_dev)
    );

    let verbose_dev = run_jet(
        &scratch.path,
        &["dev", "--verbose", "main.jet", "--watch=off"],
    );
    assert_advisory_visible("verbose dev", &verbose_dev);
}
