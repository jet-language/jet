// One deadline-aware sleep kernel. AOT, the resident scheduler, and the
// interpreter adapters all call this source; their only tier-specific pieces
// are the scheduler and deadline-boundary functions supplied by the host.

#[cfg(test)]
thread_local! {
    static TEST_DEADLINE_EXCEEDED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub fn jet_deadline_remaining_ms() -> Option<i64> {
    if TEST_DEADLINE_EXCEEDED.with(|deadline| deadline.get()) {
        return Some(0);
    }
    jet_ctx_deadline_ms().map(|d| d.saturating_sub(jet_std_time_now()))
}

#[cfg(not(test))]
pub fn jet_deadline_remaining_ms() -> Option<i64> {
    jet_ctx_deadline_ms().map(|d| d.saturating_sub(jet_std_time_now()))
}

fn jet_deadline_check(wait_kind: &str) {
    if matches!(jet_deadline_remaining_ms(), Some(ms) if ms <= 0) {
        jet_deadline_exceeded(wait_kind);
    }
}

pub fn jet_std_time_sleep(millis: i64) {
    let want = millis.max(0);
    if let Some(remaining) = jet_deadline_remaining_ms() {
        if remaining <= 0 {
            jet_deadline_exceeded("time sleep");
        }
        if want > remaining {
            jet_scheduler_sleep_ms(remaining as u64);
            jet_deadline_exceeded("time sleep");
        }
    }
    jet_scheduler_sleep_ms(want as u64);
    jet_deadline_check("time sleep");
}
