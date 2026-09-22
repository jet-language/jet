//! Card #3095 / Wave A: the process-local schedule restart boundary.
//!
//! This target deliberately exercises only the existing Prelude `JetJobClock`.
//! A fresh clock stands in for a new process: a wall-clock occurrence missed
//! before that process existed is not replayed, while the next day's matching
//! minute remains eligible. The test does not claim App shutdown cancellation
//! or full `jet dev`/generated-Core execution; those runtime boundaries still
//! need a focused App-lifetime receipt.

#[test]
fn local_schedule_restart_does_not_replay_a_missed_window() {
    use jet_jit::Job::{JetJobClock, JetJobSchedule};

    const DAY: u64 = 86_400;
    const THREE_AM: u64 = 3 * 3_600;
    let schedule = [(
        "nightly",
        JetJobSchedule::WallClockTime {
            hour: 3,
            minute: 0,
        },
    )];

    // A process that wakes after the one-minute local window has no due work.
    let missed = DAY + THREE_AM + 61;
    let mut before_restart = JetJobClock::new();
    assert!(
        before_restart.due_at(&schedule, missed).is_empty(),
        "a missed local schedule window must not be replayed before restart"
    );

    // Restarting a process-local schedule does not recover an unpersisted tick.
    let mut after_restart = JetJobClock::new();
    assert!(
        after_restart.due_at(&schedule, missed).is_empty(),
        "a fresh local clock must not replay an earlier missed occurrence"
    );

    // The next UTC day is a new occurrence, not a replay of the missed one.
    assert_eq!(
        after_restart.due_at(&schedule, DAY * 2 + THREE_AM),
        vec!["nightly".to_string()]
    );
}
