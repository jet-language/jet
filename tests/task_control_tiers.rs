//! D-CONC-SPAWN1=D + D-COROUTINE1=A (card #1685): the canonical task surface
//! means the same thing on every execution tier.

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{build_and_run, have_rustc, run_default_multi};

const SOURCE: &str = r#"
fn run() {
    handle :: task 7
    handle.pause()
    handle.resume()
    print(handle.join() ?? 0)
    print(task.all { 1, 2 })
    print(task.race { 3, 4 })
    print(task.any { 5, 6 })
}
"#;

const EXPECTED: &str = "\
7
[1, 2]
3
5
";

#[test]
fn task_control_plane_matches_on_default_run() {
    let (code, stdout, stderr) = run_default_multi("task_control", "main.jet", &[("main.jet", SOURCE)]);
    assert_eq!(code, 0, "default `jet run` must succeed\n{stdout}\n{stderr}");
    // The canonical keyword and nested combinators must stay on one path.
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
