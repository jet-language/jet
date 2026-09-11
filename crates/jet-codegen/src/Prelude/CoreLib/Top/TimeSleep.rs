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

/// Generic consumers use the shared wait policy after their operation. This
/// adapter supplies only the observed deadline and scheduler shield facts.
fn jet_deadline_check(wait_kind: &str) {
    let deadline =
        jet_std::jet_task_deadline_if_expired(jet_deadline_remaining_ms(), wait_kind);
    if let Err(jet_std::JetTaskWaitInterrupt::Deadline(_)) =
        jet_std::jet_task_wait_policy(deadline, false, jet_scheduler_shielded())
    {
        jet_deadline_exceeded(wait_kind);
    }
}

pub fn jet_std_time_sleep(millis: i64) {
    jet_task_sleep_ms_defaulted(millis);
}

/// D-TYPE2-TIME1=A: one shared boundary from the canonical Duration carrier
/// to the scheduler's millisecond ABI. Engines call this Prelude function;
/// they do not re-encode the Time unit themselves.
pub fn jet_std_time_duration_to_millis(nanos: i64) -> i64 {
    nanos.saturating_div(1_000_000)
}

/// D-TYPE2-TIME1=A: install a task deadline from the canonical Duration
/// carrier. The deadline policy stays in this shared Prelude kernel; engines
/// only marshal the carrier and call this function.
pub fn jet_task_timeout_duration_ns(nanos: i64) {
    let millis = jet_std_time_duration_to_millis(nanos);
    let millis = jet_task_delay_ms_defaulted(millis) as i64;
    jet_ctx_set_deadline_min(jet_std_time_now().saturating_add(millis));
}

/// D-TYPE2-TIME1=A: the core sleep call receives the canonical Duration
/// carrier and delegates the exact wait policy to the shared scheduler.
pub fn jet_std_time_sleep_duration_ns(nanos: i64) {
    jet_task_sleep_duration_ns_defaulted(nanos);
}

