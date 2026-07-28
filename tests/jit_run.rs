//! Tower #1253 and #1255 — the `ProcessSpec` builders stay in the JIT host
//! table.
//!
//! `env_clear`, `detached`, and `terminal` had no entry in `lower_ctx` or in
//! `safety::resident_safe_call_arg`, so default `jet run` deopted and tier 0
//! then refused `process.cmd` with E0956. The program worked under `jet build`
//! and failed under `jet run`, which is the lens gap D-LENS-RUN1 forbids.
//!
//! `env` and `env_remove` had the same missing entries, and `cwd` had a host
//! shim that dropped its argument, so the child ran in the wrong directory and
//! nothing reported it.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

mod common;

use jet::Interpreter::dev_iteration;
use jet_foundation::JitBackend::RunOutcome;

/// The Cranelift host path is not available on every architecture. Mirrors
/// `tests/dev.rs`: CI sets `JET_REQUIRE_CRANELIFT_HOST=1` so a missing host is
/// a loud failure, never a quiet green skip.
fn skip_if_cranelift_host_unsupported() -> bool {
    if jet_jit::cranelift_host_supported() {
        false
    } else if std::env::var("JET_REQUIRE_CRANELIFT_HOST").as_deref() == Ok("1") {
        panic!(
            "cranelift-jit host path unsupported on this architecture \
             (JET_REQUIRE_CRANELIFT_HOST=1); remove the host from the parity \
             matrix or restore native JIT support"
        );
    } else {
        eprintln!(
            "note: cranelift-jit host path unsupported on this architecture; \
             skipping resident JIT assertion"
        );
        true
    }
}

/// Every zero-argument `ProcessSpec` builder runs resident, and each one
/// reports what the AOT lens reports:
///   * `env_clear` still runs the command and captures its output;
///   * `detached` drops the streams, so the output is empty;
///   * `terminal` refuses, because no PTY or ConPTY backend exists.
///
/// The expected text is the recorded `jet run --release` stdout for the same
/// program, so a divergence here is a lens gap.
#[test]
fn zero_arg_process_spec_builders_run_resident() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let dir = common::unique_tmp("jit_process_zero_arg_builders");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("builders.jet");
    fs::write(
        &file,
        r#"use core.process as process

fn run() {
    cleared :: process.cmd(["echo", "x"]).env_clear().run() ?? panic("env_clear failed")
    print(cleared.success)
    print(cleared.output.trim())
    detached :: process.cmd(["echo", "y"]).detached().run() ?? panic("detached failed")
    print(detached.success)
    print("[{detached.output.trim()}]")
    if process.cmd(["echo", "z"]).terminal().run() == {
        .Ok(v) -> { print("terminal ok") }
        .Err(e) -> { print("terminal err") }
        else -> {}
    }
}
"#,
    )
    .unwrap();

    jet_jit::reset_jit_trace_for_test();
    let outcome = dev_iteration(file.to_str().unwrap(), false, false);
    let stdout = match outcome {
        RunOutcome::Ran { stdout, .. } => stdout,
        RunOutcome::Problems(diags) => panic!(
            "zero-argument ProcessSpec builders must run under default `jet run`: {:?}",
            diags.iter().map(|d| d.code.clone()).collect::<Vec<_>>()
        ),
    };
    assert_eq!(stdout, "true\nx\ntrue\n[]\nterminal err\n");
    assert!(
        jet_jit::jit_executed_for_test(),
        "the builders must lower to host calls, not deopt"
    );
    assert!(
        !jet_jit::fallback_invoked_for_test(),
        "a missing host entry deopts to tier 0, which then raises E0956"
    );

    let _ = fs::remove_dir_all(&dir);
}

fn jet_string(value: &Path) -> String {
    value
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

fn run_jet(file: &Path, release: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_jet"));
    command.arg("run");
    if release {
        command.arg("--release");
    }
    command
        .arg(file)
        .env("NO_COLOR", "1")
        .env("JET_SPEC_REMOVE", "host-value")
        .output()
        .expect("run ProcessSpec lens fixture")
}

/// Portable child for the ProcessSpec fixture. The fixture invokes this test
/// binary by absolute path, so it needs no shell or platform utility.
#[test]
fn process_probe_helper() {
    if std::env::var("JET_PROCESS_PROBE").as_deref() != Ok("1") {
        return;
    }
    let cwd = std::env::current_dir().unwrap();
    let logical = std::env::var("JET_LOGICAL_ENV").unwrap();
    let spec_set = std::env::var("JET_SPEC_SET").unwrap();
    let removed = std::env::var_os("JET_SPEC_REMOVE").is_none();
    fs::write(
        "process-probe.txt",
        format!(
            "cwd={}|logical={logical}|set={spec_set}|removed={removed}",
            cwd.display()
        ),
    )
    .unwrap();
}

/// Argument-taking builders stay resident and match the AOT lens byte for
/// byte. The child also proves that `core.env` mutations and ProcessSpec
/// overrides share one logical environment.
#[test]
fn arg_process_spec_builders_reach_the_child() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let dir = common::unique_tmp("jit_process_arg_builders");
    fs::create_dir_all(&dir).unwrap();
    let child_dir = fs::canonicalize(&dir).unwrap();
    let child_probe = child_dir.join("process-probe.txt");
    let bad_value_path = child_dir.join("bad-env-value.txt");
    fs::write(&bad_value_path, b"bad\0value").unwrap();
    let test_binary = std::env::current_exe().unwrap();
    let file = dir.join("builders.jet");
    fs::write(
        &file,
        format!(
            r#"use core.env as env
use core.files as files
use core.process as process

fn run() {{
    env.set("JET_PROCESS_PROBE", "1")
    env.set("JET_LOGICAL_ENV", "logical-value")
    print((env.get("JET_LOGICAL_ENV") ?? "missing") == "logical-value")
    names :: env.vars() ?? panic("env vars failed")
    print(names.contains("JET_LOGICAL_ENV"))
    env.set("JET_LOGICAL_GONE", "gone")
    print(env.unset("JET_LOGICAL_GONE") ?? false)
    print((env.get("JET_LOGICAL_GONE") ?? "missing") == "missing")
    child :: process.cmd(["{test_binary}", "--exact", "process_probe_helper", "--nocapture"])
        .cwd("{child_dir}")
        .env("JET_SPEC_SET", "spec-value")
        .env_remove("JET_SPEC_REMOVE")
        .run() ?? panic("child failed")
    print(child.success)
    print(files.read("{child_probe}") ?? panic("probe read failed"))
    if process.cmd(["{test_binary}"]).env("BAD=NAME", "value").run() == {{
        .Ok(v) -> {{ print("process name accepted") }}
        .Err(e) -> {{ print("process name rejected") }}
        else -> {{}}
    }}
    bad_value :: files.read("{bad_value_path}") ?? panic("bad value read failed")
    if process.cmd(["{test_binary}"]).env("JET_BAD_VALUE", bad_value).run() == {{
        .Ok(v) -> {{ print("process value accepted") }}
        .Err(e) -> {{ print("process value rejected") }}
        else -> {{}}
    }}
}}
"#,
            test_binary = jet_string(&test_binary),
            child_dir = jet_string(&child_dir),
            child_probe = jet_string(&child_probe),
            bad_value_path = jet_string(&bad_value_path),
        ),
    )
    .unwrap();

    // Default `jet run` cannot deopt this program: tier 0 rejects
    // `process.cmd` with E0956. Removing any cwd/env/env_remove residency or
    // dispatch entry therefore makes this command fail.
    let default = run_jet(&file, false);
    let release = run_jet(&file, true);
    assert_eq!(
        default.status.code(),
        release.status.code(),
        "default stdout:\n{}\ndefault stderr:\n{}\nrelease stdout:\n{}\nrelease stderr:\n{}",
        String::from_utf8_lossy(&default.stdout),
        String::from_utf8_lossy(&default.stderr),
        String::from_utf8_lossy(&release.stdout),
        String::from_utf8_lossy(&release.stderr),
    );
    assert_eq!(default.stdout, release.stdout);
    let release_stderr = String::from_utf8(release.stderr.clone()).unwrap();
    let (_, release_program_stderr) = release_stderr
        .split_once('\n')
        .filter(|(line, _)| line.starts_with("effects: "))
        .expect("release lens must report its compile-time effect summary");
    assert_eq!(
        String::from_utf8_lossy(&default.stderr),
        release_program_stderr
    );
    assert!(
        default.status.success(),
        "default lens failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&default.stdout),
        String::from_utf8_lossy(&default.stderr)
    );
    let stdout = String::from_utf8_lossy(&default.stdout);
    assert!(
        stdout.starts_with("true\ntrue\ntrue\ntrue\ntrue\ncwd="),
        "{stdout}"
    );
    assert!(stdout.contains("|logical=logical-value|set=spec-value|removed=true\n"));
    assert!(stdout.ends_with("process name rejected\nprocess value rejected\n"));

    let _ = fs::remove_dir_all(&dir);
}
