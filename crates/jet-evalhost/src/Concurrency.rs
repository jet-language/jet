//! Scheduler-backed task, interrupt, and host-fault channels for the
//! evaluator's Core host services.

use jet_codegen::scheduler::{
    jet_scheduler_classify_unwind, jet_scheduler_panic_should_unwind,
    jet_scheduler_wait_without_unwind, JetSchedulerWait, JetShieldExit
};
use jet_codegen::task_group::JetTaskGroupPermit;
use std::cell::{Cell, RefCell};

thread_local! {
    static LOCAL_RICH_PANIC: Cell<bool> = const { Cell::new(false) };
    static PENDING_SHIELD_EXIT: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
    static PENDING_CHILD_DEADLINE: RefCell<Option<String>> = const { RefCell::new(None) };
    static PENDING_TASK_GROUP_PERMIT: RefCell<Option<JetTaskGroupPermit>> = const { RefCell::new(None) };
    static RICH_PANIC_REASON: RefCell<Option<String>> = const { RefCell::new(None) };
    static RICH_PANIC_REPORT: RefCell<Option<String>> = const { RefCell::new(None) };
    static RICH_STREAM_STOP: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
    static WAIT_VALUE: std::cell::Cell<i64> = const { std::cell::Cell::new(0) };
    /// A native task owns its trap until the parent observes its join result.
    /// Keeping this in task-local storage prevents a failing child from
    /// making an unrelated sibling leave at its next loop header.
    static TASK_TRAP: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn set_pending_shield_exit(exit: JetShieldExit) {
    let code = match exit {
        JetShieldExit::None => 0,
        JetShieldExit::Deadline => 2,
        JetShieldExit::Cancelled => 1
    };
    PENDING_SHIELD_EXIT.with(|pending| pending.set(pending.get().max(code)));
}

/// Record a deadline interrupt without unwinding. A child task hands the
/// rendered E3003 report to its parent at the join boundary.
fn record_deadline_interrupt(rendered: String) {
    if jet_scheduler_panic_should_unwind() {
        PENDING_CHILD_DEADLINE.with(|pending| {
            *pending.borrow_mut() = Some(rendered);
        });
    }
}

/// Report an engine defect raised at a host seam. No runtime owns a status
/// channel for it, so never return a silent success: say it on stderr.
fn host_fault(message: &str) {
    let _ = crate::IO::term_prelude::jet_term_write_stderr_bytes(
        format!("internal compiler error: {message}\n").as_bytes(),
        true,
    );
}

/// Put one converted control transfer on the tier's existing status channel.
///
/// I8: this is the single delivery point. The wait-point form
/// ([`contain_seam_unwind`]) and the plain-seam form
/// ([`deliver_caught_unwind`]) differ only in how they catch — never in what a
/// cancel, a blown deadline, or a defect then means.
fn deliver_wait_status(status: JetSchedulerWait<()>) {
    match status {
        JetSchedulerWait::Ready(()) => {}
        JetSchedulerWait::Cancelled => set_pending_shield_exit(JetShieldExit::Cancelled),
        JetSchedulerWait::Deadline(rendered) => record_deadline_interrupt(rendered),
        // A rendered `Stop [E…]` report already reached its owner; only an
        // engine defect needs reporting here.
        JetSchedulerWait::Panicked(message) => {
            if !message.starts_with("Stop [E") {
                host_fault(&message);
            }
        }
    }
}

/// Deliver an unwind caught at a `host_seam` `extern "C"` boundary. Always
/// returns normally; no Rust unwind crosses the foreign frame (I1).
pub(crate) fn deliver_caught_unwind(payload: Box<dyn std::any::Any + Send>) {
    deliver_wait_status(jet_scheduler_classify_unwind(payload));
}

/// Keep an ambient `core.time.sleep` on the no-unwind wait boundary, so
/// cancellation and deadlines reach their owner instead of escaping through
/// the evaluator frame.
pub(crate) fn ambient_time_sleep(nanos: i64) {
    deliver_wait_status(jet_scheduler_wait_without_unwind(|| {
        jet_codegen::scheduler::jet_std_time_sleep_duration_ns(nanos)
    }));
}
