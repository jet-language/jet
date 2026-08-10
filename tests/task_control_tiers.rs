//! D-VERDICT-1323-1 + D-COROUTINE1=A (cards #1323, #1254): the task control
//! plane means the same thing on every execution tier.
//!
//! Each `*_all` list method is exactly its single-handle counterpart applied in
//! order, and `trace` renders one string that AOT, the Cranelift JIT, and the
//! TIR interpreter all produce identically. The interpreter tier matters here
//! even though it is a deopt target: any program the JIT declines to lower
//! resident falls through to it, and a whole family of these programs does.

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{build_and_run, have_rustc, run_default_multi};

/// Every method in the ratified set, singles beside their list twins. Flags are
/// read back through `trace`/`trace_all`, so a tier that silently no-ops any of
/// them prints a different transcript.
const SOURCE: &str = r#"
use core.tasks as tasks

fn run() {
    handle :: tasks.spawn(() => 7)
    handle.pause()
    print(handle.trace())
    handle.resume()
    print(handle.trace())
    handle.cancel()
    print(handle.trace())
    print(handle.wait())

    group :: tasks.spawn_group(3, () => 5)
    group.pause_all()
    print(group.trace_all())
    group.resume_all()
    print(group.trace_all())
    group.cancel_all()
    print(group.trace_all())
    print(group.wait_all())

    pair :: [tasks.spawn(() => 1), tasks.spawn(() => 2)]
    print(pair.join_all())

    detached :: tasks.spawn_group(2, () => 0)
    detached.detach_all()
}
"#;

const EXPECTED: &str = "\
paused=true,cancel=false
paused=false,cancel=false
paused=false,cancel=true
7
[paused=true,cancel=false, paused=true,cancel=false, paused=true,cancel=false]
[paused=false,cancel=false, paused=false,cancel=false, paused=false,cancel=false]
[paused=false,cancel=true, paused=false,cancel=true, paused=false,cancel=true]
[5, 5, 5]
[1, 2]
";

#[test]
fn task_control_plane_matches_on_default_run() {
    let (code, stdout, stderr) = run_default_multi("task_control", "main.jet", &[("main.jet", SOURCE)]);
    assert_eq!(code, 0, "default `jet run` must succeed\n{stdout}\n{stderr}");
    // The gap this test was written for: `pause_all` / `resume_all` /
    // `trace_all` built under AOT and died here with E0956.
    assert!(
        !stderr.contains("E0956"),
        "no tier gap may reach the surface for the ratified set\n{stderr}"
    );
    assert_eq!(strip_tier_trace(&stdout), EXPECTED, "stderr:\n{stderr}");
}

#[test]
fn task_control_plane_matches_under_aot() {
    if !have_rustc() {
        return;
    }
    let (code, stdout) = build_and_run("task_control", SOURCE);
    assert_eq!(code, 0, "AOT build must run\n{stdout}");
    assert_eq!(stdout, EXPECTED);
}

/// `run_default_multi` passes `--trace-tiers`, which may prefix diagnostics of
/// its own on stdout. Keep only the program's transcript.
fn strip_tier_trace(stdout: &str) -> String {
    stdout
        .lines()
        .filter(|line| !line.starts_with("[tier"))
        .map(|line| format!("{line}\n"))
        .collect()
}
