//! #1254 — runtime-tier E0956 must not speak in comptime voice.
//!
//! Default `jet run` shares the TIR evaluator with comptime. When that
//! evaluator hits an unsupported construct it emits E0956 with comptime
//! what/why/fix. The runtime-role boundary rewrites those fields so a
//! plain `jet run` user is told about Jet's quick-run gap, not that their
//! (non-comptime) program "can't run at compile time."

use std::fs;
use std::process::Command;

mod common;

use jet::Interpreter::run_jit_once;
use jet_foundation::JitBackend::RunOutcome;

fn skip_if_cranelift_host_unsupported() -> bool {
    if jet_jit::cranelift_host_supported() {
        false
    } else if std::env::var("JET_REQUIRE_CRANELIFT_HOST").as_deref() == Ok("1") {
        panic!(
            "cranelift-jit host path unsupported on this architecture \
             (JET_REQUIRE_CRANELIFT_HOST=1)"
        );
    } else {
        eprintln!(
            "note: cranelift-jit host path unsupported; skipping run-tier diag assertion"
        );
        true
    }
}

fn assert_no_comptime_voice(what: &str, why: &str, fix: &str) {
    for (label, text) in [("what", what), ("why", why), ("fix", fix)] {
        let lower = text.to_ascii_lowercase();
        assert!(
            !lower.contains("comptime") && !lower.contains("compile time"),
            "runtime-tier E0956 {label} must not mention comptime/compile time, got: {text:?}"
        );
    }
}

#[test]
fn jet_run_e0956_uses_quick_run_voice() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let dir = common::unique_tmp("run_tier_e0956");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("events_gap.jet");
    // Minimal stand-in for examples/features/ui/events.jet — hits the same
    // unsupported `event.scope()` seam under whole-program deopt.
    fs::write(
        &file,
        r#"use core.event as event

fn run() {
    scope :: event.scope()
    print(scope)
}
"#,
    )
    .unwrap();

    let path = file.to_str().unwrap();
    let RunOutcome::Problems(diags) = run_jit_once(path) else {
        panic!("expected RunOutcome::Problems for unsupported event.scope under jet run");
    };
    let d = diags
        .iter()
        .find(|d| d.code == "E0956")
        .unwrap_or_else(|| panic!("expected E0956, got: {diags:?}"));

    assert!(
        d.what.contains("event.scope") || d.what.contains("core.event.scope"),
        "what must name the construct, got: {:?}",
        d.what
    );
    assert!(
        d.what.contains("quick-run"),
        "what must name quick-run mode, got: {:?}",
        d.what
    );
    assert!(
        d.why.contains("gap in Jet") && d.why.contains("not a mistake"),
        "why must blame Jet's gap, got: {:?}",
        d.why
    );
    assert!(
        d.fix.contains("jet run --release"),
        "fix must point at jet run --release, got: {:?}",
        d.fix
    );
    assert_no_comptime_voice(&d.what, &d.why, &d.fix);
}

#[test]
fn comptime_e0956_keeps_original_voice() {
    // Sema/comptime path must stay unchanged — ui snapshot + explain copy.
    let out = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["check", "tests/ui/comptime_panic.jet"])
        .output()
        .expect("jet check");
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let text = format!("{stdout}{stderr}");
    assert!(
        text.contains("E0956"),
        "expected comptime-role E0956, got:\n{text}"
    );
    assert!(
        text.contains("can't run at compile time yet"),
        "comptime E0956 what must stay original, got:\n{text}"
    );
    assert!(
        !text.contains("quick-run"),
        "comptime E0956 must not use runtime quick-run voice, got:\n{text}"
    );
}
