//! #1629 — runtime-tier E0956 uses the shared diagnostic voice.
//!
//! Default `jet run` shares the TIR evaluator with comptime. When that
//! evaluator hits an unsupported construct, every tier renders the same
//! E0956 what/why/fix text.

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

#[test]
fn jet_run_e0956_uses_shared_voice() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let dir = common::unique_tmp("run_tier_e0956");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("watcher_gap.jet");
    // Stand-in unsupported Core call under whole-program deopt — proves shared
    // E0956 voice (was `event.scope` before EventLite closed that gap).
    fs::write(
        &file,
        r#"use core.watcher as watcher

fn run() {
    w :: watcher.files(".") ?? panic("x")
    print(w)
}
"#,
    )
    .unwrap();

    let path = file.to_str().unwrap();
    let RunOutcome::Problems(diags) = run_jit_once(path) else {
        panic!("expected RunOutcome::Problems for unsupported watcher.files under jet run");
    };
    let d = diags
        .iter()
        .find(|d| d.code == "E0956")
        .unwrap_or_else(|| panic!("expected E0956, got: {diags:?}"));

    assert!(
        d.what.contains("watcher.files") || d.what.contains("core.watcher.files"),
        "what must name the construct, got: {:?}",
        d.what
    );
    assert!(
        d.what.contains("can't run at compile time yet"),
        "what must use shared E0956 voice, got: {:?}",
        d.what
    );
    assert!(
        d.why == "the canonical TIR evaluator doesn't cover this construct yet",
        "why must use shared E0956 voice, got: {:?}",
        d.why
    );
    assert!(
        d.fix == "use a simpler form, or run via `jet build` / `jet run`",
        "fix must use shared E0956 voice, got: {:?}",
        d.fix
    );
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

/// Same unsupported construct as the runtime-voice test, but evaluated at
/// comptime — must keep the comptime voice (dual-role pin).
#[test]
fn comptime_watcher_files_keeps_comptime_voice() {
    let dir = common::unique_tmp("run_tier_e0956_ct");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("watcher_gap_ct.jet");
    fs::write(
        &file,
        r#"use core.watcher as watcher

$w :: watcher.files(".") ?? panic("x")

fn run() {
    print(w)
}
"#,
    )
    .unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["check", file.to_str().unwrap()])
        .output()
        .expect("jet check");
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let text = format!("{stdout}{stderr}");
    assert!(
        text.contains("E0956"),
        "expected comptime-role E0956 for watcher.files, got:\n{text}"
    );
    assert!(
        text.contains("can't run at compile time yet")
            || text.contains("compile time"),
        "comptime E0956 must keep comptime voice, got:\n{text}"
    );
    assert!(
        !text.contains("quick-run"),
        "comptime E0956 must not use runtime quick-run voice, got:\n{text}"
    );
}
