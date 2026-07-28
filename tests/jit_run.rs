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

/// The argument-taking `ProcessSpec` builders run resident and the argument
/// reaches the child.
///
/// The child prints its own working directory, so the assertion fails if `cwd`
/// is dropped the way the old host shim dropped it. It also prints one name set
/// by `env` and one name that `env` set and `env_remove` then took away, so a
/// no-op `env_remove` prints `dropped` instead of an empty value.
///
/// The expected text is the recorded `jet run --release` stdout for the same
/// program, so a divergence here is a lens gap.
#[test]
fn arg_process_spec_builders_reach_the_child() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let dir = common::unique_tmp("jit_process_arg_builders");
    fs::create_dir_all(&dir).unwrap();
    // `pwd` reports the physical path, so compare against the resolved one.
    let child_dir = fs::canonicalize(&dir).unwrap();
    let child_dir = child_dir.to_str().unwrap().to_string();
    let file = dir.join("builders.jet");
    fs::write(
        &file,
        format!(
            r#"use core.process as process

fn run() {{
    here :: process.cmd(["pwd"]).cwd("{child_dir}").run() ?? panic("cwd failed")
    print(here.output.trim())
    envs :: process.cmd(["sh", "-c", "echo [$JET_PROBE_KEEP] [$JET_PROBE_DROP]"])
        .env("JET_PROBE_KEEP", "kept")
        .env("JET_PROBE_DROP", "dropped")
        .env_remove("JET_PROBE_DROP")
        .run() ?? panic("env failed")
    print(envs.output.trim())
}}
"#
        ),
    )
    .unwrap();

    jet_jit::reset_jit_trace_for_test();
    let outcome = dev_iteration(file.to_str().unwrap(), false, false);
    let stdout = match outcome {
        RunOutcome::Ran { stdout, .. } => stdout,
        RunOutcome::Problems(diags) => panic!(
            "the argument-taking ProcessSpec builders must run under default \
             `jet run`: {:?}",
            diags.iter().map(|d| d.code.clone()).collect::<Vec<_>>()
        ),
    };
    assert_eq!(stdout, format!("{child_dir}\n[kept] []\n"));
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
