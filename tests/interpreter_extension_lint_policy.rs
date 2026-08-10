//! #1678: literal `jet run` commands share the extension/lint-policy gate.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use common::Scratch;

const POLICY: &str = "name: \"extension-policy\"\nversion: \"0.1.0\"\npolicy: .{ lints: .{ deny: [compiler_extension] } }\n";
const SOURCE: &str = "fn x() {}\nfn run() {\n    print(1)\n}\n";

fn compiler_extension() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("crates/jet-pkg-model/fixtures/compiler_extension/lint_no_x.wasm")
}

fn run(dir: &Path, interpret: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_jet"));
    command.arg("run");
    if interpret {
        command.arg("--interpret");
    }
    command
        .arg("main.jet")
        .current_dir(dir)
        .env("JET_COMPILER_EXTENSION", compiler_extension())
        .env(
            "JET_RUN_CACHE_DIR",
            dir.join(if interpret { "cache-interpreter" } else { "cache-default" }),
        )
        .env("NO_COLOR", "1")
        .output()
        .expect("run literal jet CLI")
}

fn assert_denied(label: &str, output: &Output) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{label}:\n{stderr}");
    assert!(output.stdout.is_empty(), "{label} executed user code");
    assert!(
        stderr.contains(
            "Error [E1293]: lint `L1401` is denied by policy: compiler-extension `no-x` (warning): prefer y"
        ),
        "{label} missing E1293 what:\n{stderr}"
    );
    assert!(
        stderr.contains(
            "Why: a configured compiler-extension component reported this finding after type checking (D-DX5-HOOK1). This team's `policy.lints.deny` in `package.jet` turns this warning into a build failure (D-LINTPOLICY1 — the override law); it stays a warning everywhere `package.jet` doesn't opt in."
        ),
        "{label} missing E1293 why:\n{stderr}"
    );
    assert!(
        stderr.contains(
            "Fix: address the finding, or unset JET_COMPILER_EXTENSION to skip the extension"
        ),
        "{label} missing E1293 fix:\n{stderr}"
    );
    assert!(
        !stderr.contains("Warning [L1401]"),
        "{label} printed the denied lint twice:\n{stderr}"
    );
}

fn assert_retired_policy_ignored(label: &str, output: &Output) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(0), "{label}:\n{stderr}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "1\n", "{label}");
    assert!(
        !stderr.contains("E1293"),
        "{label} read policy from retired pkg.jet:\n{stderr}"
    );
}

#[test]
fn literal_cli_run_and_interpret_enforce_compiler_extension_lint_policy() {
    let project = Scratch::new("extension-lint-policy");
    fs::write(project.join("package.jet"), POLICY).unwrap();
    fs::write(project.join("main.jet"), SOURCE).unwrap();

    assert_denied("jet run", &run(&project.path, false));
    assert_denied("jet run --interpret", &run(&project.path, true));
}

#[test]
fn literal_cli_run_and_interpret_ignore_retired_pkg_jet_lint_policy() {
    let project = Scratch::new("retired-extension-lint-policy");
    fs::write(project.join("pkg.jet"), POLICY).unwrap();
    fs::write(project.join("main.jet"), SOURCE).unwrap();

    assert_retired_policy_ignored("jet run", &run(&project.path, false));
    assert_retired_policy_ignored("jet run --interpret", &run(&project.path, true));
}
