// Card #1747: the `#Context(deadline: …)` clock and budget are the same
// plumbing on every tier — AOT embeds this file into the generated program
// and `jet_codegen::scheduler` compiles the same source for the Cranelift
// JIT host, so the two cannot drift. The E3003 raise stays local to each
// caller (`jet_deadline_exceeded` in `Prelude/CoreLib/Top/MathRandomTime.rs`
// and `SchedulerHost.rs`): the unwind boundary and process-exit path are
// real per-tier marshalling (AOT has an interrupt-handler frame and exits
// through the panic-based `jet_runtime_exit`; the JIT host has no
// interrupt-handler prelude and exits the resident process directly), but
// both render the same text through the one E3003 renderer in
// `Prelude/TaskGroup.rs`.

pub fn jet_std_time_now() -> i64 {
    if let Ok(s) = std::env::var("JET_PROVE_REPLAY_TIME_MS") {
        if let Ok(n) = s.parse::<i64>() {
            return n;
        }
    }
    if let Ok(s) = std::env::var("LEX_TEST_EPOCH") {
        if let Ok(n) = s.parse::<i64>() {
            return n;
        }
    }
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

thread_local! {
    static JET_CTX_DEADLINE_MS: std::cell::Cell<Option<i64>> = const { std::cell::Cell::new(None) };
}

pub struct JetDeadlineGuard {
    saved: Option<i64>,
}

impl Drop for JetDeadlineGuard {
    fn drop(&mut self) {
        JET_CTX_DEADLINE_MS.with(|c| c.set(self.saved));
    }
}

/// Absolute deadline millis currently installed for this task/thread, if any.
pub fn jet_ctx_deadline_ms() -> Option<i64> {
    JET_CTX_DEADLINE_MS.with(|c| c.get())
}

/// Push a `#Context(deadline: …)` budget; drop restores the previous value.
pub fn jet_ctx_push_deadline(deadline_ms: i64) -> JetDeadlineGuard {
    let saved = JET_CTX_DEADLINE_MS.with(|c| c.replace(Some(deadline_ms)));
    JetDeadlineGuard { saved }
}
