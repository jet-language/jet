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

fn allow_io(root: &Path) {
    fs::write(
        root.join("package.jet"),
        "name: \"lint-visibility\"\nversion: \"0.1.0\"\nauthority: { holds: { allow: [IO] } }\n",
    )
    .expect("write IO authority for lint visibility fixture");
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
    allow_io(&scratch.path);

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
    allow_io(&scratch.path);

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

const TESTED_SOURCE: &str = "fn run() {\n    unused_binding :: 1\n    print(\"ok\")\n}\n\n#Test(\"adds\") {\n    assert_eq(1 + 1, 2)\n}\n";

fn package(root: &Path, allow: &str, extra: &str) {
    fs::write(
        root.join("package.jet"),
        format!(
            "name: \"lint-visibility\"\nversion: \"0.1.0\"\nauthority: {{ holds: {{ allow: [{allow}] }} }}\n{extra}"
        ),
    )
    .expect("write lint visibility package");
}

fn assert_fails_with(label: &str, output: &Output, code: &str) {
    assert!(!output.status.success(), "{label} must fail:\n{}", combined(output));
    assert!(
        combined(output).contains(&format!("[{code}]")),
        "{label} must report {code}:\n{}",
        combined(output)
    );
}

#[test]
fn test_hides_advisories_unless_verbose() {
    let scratch = common::Scratch::new("lint-visibility-wave-a-test");
    fs::write(scratch.join("main.jet"), TESTED_SOURCE).expect("write test lint fixture");
    allow_io(&scratch.path);

    let quiet = run_jet(&scratch.path, &["test", "--capture=all", "main.jet"]);
    assert!(quiet.status.success(), "default test failed:\n{}", combined(&quiet));
    assert!(
        !combined(&quiet).contains("[L0101]"),
        "default test should hide ordinary advisories:\n{}",
        combined(&quiet)
    );
    assert!(combined(&quiet).contains("1 passed, 0 failed"), "{}", combined(&quiet));

    // The default run above stored a cached result; the verbose run must
    // still show the advisory and aggregate the same pass count.
    let verbose = run_jet(&scratch.path, &["test", "--verbose", "--capture=all", "main.jet"]);
    assert_advisory_visible("verbose test", &verbose);
    assert!(combined(&verbose).contains("1 passed, 0 failed"), "{}", combined(&verbose));
}

#[test]
fn enforcement_survives_hidden_advisories() {
    let deny = common::Scratch::new("lint-visibility-wave-a-deny");
    fs::write(deny.join("main.jet"), TESTED_SOURCE).expect("write deny fixture");
    package(
        &deny.path,
        "IO",
        "policy: { lints: { deny: [unused_local_binding] } }\n",
    );
    for args in [
        &["run", "main.jet"][..],
        &["run", "--verbose", "main.jet"],
        &["test", "main.jet"],
        &["test", "--verbose", "main.jet"],
    ] {
        assert_fails_with(&format!("denied lint {args:?}"), &run_jet(&deny.path, args), "E1293");
    }

    let sema = common::Scratch::new("lint-visibility-wave-a-sema");
    fs::write(
        sema.join("main.jet"),
        TESTED_SOURCE.replace("    print(\"ok\")", "    x :: 1 + \"a\"\n    print(\"ok\")"),
    )
    .expect("write sema fixture");
    allow_io(&sema.path);
    for args in [&["run", "main.jet"][..], &["test", "main.jet"]] {
        assert_fails_with(&format!("sema error {args:?}"), &run_jet(&sema.path, args), "E0109");
    }

    let runtime = common::Scratch::new("lint-visibility-wave-a-runtime");
    fs::write(
        runtime.join("main.jet"),
        "fn boom() {\n    panic(\"boom\")\n}\n\nfn run() {\n    unused_binding :: 1\n    boom()\n}\n\n#Test(\"stops\") {\n    boom()\n}\n",
    )
    .expect("write runtime fixture");
    allow_io(&runtime.path);
    for args in [&["run", "main.jet"][..], &["run", "--verbose", "main.jet"]] {
        assert_fails_with(&format!("runtime stop {args:?}"), &run_jet(&runtime.path, args), "E3001");
    }
    let failing_test = run_jet(&runtime.path, &["test", "main.jet"]);
    assert!(!failing_test.status.success(), "{}", combined(&failing_test));
    assert!(combined(&failing_test).contains("0 passed, 1 failed"), "{}", combined(&failing_test));

    let authority = common::Scratch::new("lint-visibility-wave-a-authority");
    fs::write(authority.join("main.jet"), TESTED_SOURCE).expect("write authority fixture");
    package(&authority.path, "", "");
    for args in [&["run", "main.jet"][..], &["run", "--verbose", "main.jet"]] {
        assert_fails_with(&format!("authority {args:?}"), &run_jet(&authority.path, args), "E1220");
    }
}

#[test]
fn json_output_keeps_structure_with_hidden_advisories() {
    let scratch = common::Scratch::new("lint-visibility-wave-a-json");
    fs::write(scratch.join("main.jet"), TESTED_SOURCE).expect("write json fixture");
    allow_io(&scratch.path);

    let run = run_jet(&scratch.path, &["run", "--json", "main.jet"]);
    assert!(run.status.success(), "{}", combined(&run));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "ok\n", "application stdout stays clean");
    assert!(!combined(&run).contains("[L0101]"), "{}", combined(&run));

    // A warnings-only check exits 0, so its envelope must not say it failed.
    let check = run_jet(&scratch.path, &["check", "--json", "main.jet"]);
    let stdout = String::from_utf8_lossy(&check.stdout);
    assert!(check.status.success(), "{}", combined(&check));
    assert!(stdout.contains("L0101"), "check --json must report the advisory:\n{stdout}");
    assert!(
        stdout.contains("\"ok\":true") && !stdout.contains("\"ok\":false"),
        "warnings-only check --json must report ok:true:\n{stdout}"
    );

    let test = run_jet(&scratch.path, &["test", "--json", "main.jet"]);
    let stdout = String::from_utf8_lossy(&test.stdout);
    assert!(test.status.success(), "{}", combined(&test));
    assert!(stdout.trim_start().starts_with('{'), "{stdout}");
    assert!(stdout.contains("\"action\":\"test\"") && stdout.contains("\"ok\":true"), "{stdout}");
    assert!(!combined(&test).contains("L0101"), "{}", combined(&test));
}
