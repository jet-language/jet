//! D-DEV3 save-to-diagnostic latency budget, measured with nothing else running.
//!
//! Card #2005. This assertion used to live in `tests/dev.rs`, sharing one
//! process with the rest of the dev suite. Wall clock there reports scheduler
//! contention as much as compiler work: the same commit failed in the full
//! parallel run and passed under `--test-threads=1 --exact`, and the verdict
//! moved when the suite grew by 32 reached tests without the front end
//! changing at all. A verdict that moves with the suite's size is not a
//! latency detector.
//!
//! D-DEV3 is a wall-clock promise to the user — "a save gives feedback in well
//! under 200ms" (`docs/spec/diagnostics.md`) — so the measurement basis stays
//! wall clock and the budget stays 200ms. What changed is where it is
//! measured. This file is its own cargo test target holding exactly one test,
//! and both `cargo test` and `tools/ci/test-shards.sh` run one target per
//! process, one after another. So nothing else in the run is executing while
//! the number is taken, and the count of tests elsewhere cannot move it.
//!
//! It still catches the regression the budget exists for. The measured round is
//! the same front-end check the watch loop performs on every save, so any
//! change that pushes it past 200ms — a slower parse, a standard-library
//! re-read per check, a quadratic sema walk, a newly blocking lookup — fails
//! here.
//!
//! Two alternatives were considered and rejected. Counting work (steps, bytes)
//! instead of time needs a machine-specific steps-to-milliseconds constant
//! before it can say anything about the 200ms promise, and it stays green
//! through a regression that spends its time blocking rather than computing. A
//! ratio against a same-run baseline check cancels fixed per-save cost, since
//! both sides pay it — and fixed per-save cost is exactly what D-DEV3 bounds.

/// The example whose per-save round is measured.
const EXAMPLE: &str = "examples/features/collections/wordcount.jet";

/// The D-DEV3 budget for one check-only save-to-diagnostic round, in
/// milliseconds. Never widen this to make a run agree: a trip is a latency
/// defect to report, not a limit to raise.
const BUDGET_MS: u128 = 200;

/// Rounds timed after the warm-up. The best of them is the sample least
/// disturbed by anything outside this process.
const ROUNDS: usize = 5;

#[test]
fn check_latency_under_budget_measured_alone() {
    // A second test in this target would run beside the measurement and put
    // #2005's load dependence straight back. Pinned rather than requested in a
    // comment. The needle is assembled at run time so it cannot match itself.
    let attribute = format!("{}{}", "#[", "test]");
    assert_eq!(
        include_str!("dev_latency.rs").matches(&attribute).count(),
        1,
        "this target measures wall clock, so it must hold exactly one test (#2005)"
    );

    // Warm up: the first load touches the filesystem and fills caches.
    let warm = jet::check_with_path(EXAMPLE);
    // A check that fails to load is fast for the wrong reason, and would hold
    // the budget green while measuring almost nothing.
    let errors = warm
        .iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .count();
    assert_eq!(
        errors, 0,
        "{EXAMPLE} must check clean, or the timed round is not a real check: {warm:?}"
    );

    let mut best = u128::MAX;
    for _ in 0..ROUNDS {
        let started = std::time::Instant::now();
        let _ = jet::check_with_path(EXAMPLE);
        best = best.min(started.elapsed().as_millis());
    }
    assert!(
        best < BUDGET_MS,
        "save-to-diagnostic latency {best} ms exceeds the {BUDGET_MS}ms budget (D-DEV3)"
    );
}
