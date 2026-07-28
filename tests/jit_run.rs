//! Tower #1253 — the zero-argument `ProcessSpec` builders stay in the JIT host
//! table.
//!
//! `env_clear`, `detached`, and `terminal` had no entry in `lower_ctx` or in
//! `safety::resident_safe_call_arg`, so default `jet run` deopted and tier 0
//! then refused `process.cmd` with E0956. The program worked under `jet build`
//! and failed under `jet run`, which is the lens gap D-LENS-RUN1 forbids.

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
