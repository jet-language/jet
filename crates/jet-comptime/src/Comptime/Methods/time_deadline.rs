// Comptime has no resident scheduler host. These are the host-side scheduler
// primitives for the shared deadline kernel; the policy itself remains in
// Prelude/CoreLib/Top/TimeSleep.rs.
pub(super) mod jet_std {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) enum JetTaskWaitInterrupt<D> {
        Deadline(D),
        Cancelled,
    }

    pub(super) fn jet_task_deadline_if_expired(
        remaining_ms: Option<i64>,
        _wait_kind: &str,
    ) -> Option<i64> {
        remaining_ms.filter(|remaining| *remaining <= 0)
    }

    pub(super) fn jet_task_wait_policy<D>(
        deadline: Option<D>,
        cancelled: bool,
        shielded: bool,
    ) -> Result<(), JetTaskWaitInterrupt<D>> {
        if shielded {
            return Ok(());
        }
        if let Some(deadline) = deadline {
            return Err(JetTaskWaitInterrupt::Deadline(deadline));
        }
        if cancelled {
            return Err(JetTaskWaitInterrupt::Cancelled);
        }
        Ok(())
    }
}

pub(super) fn jet_scheduler_sleep_ms(millis: u64) {
    jet_scheduler_sleep_duration("time sleep", std::time::Duration::from_millis(millis));
}

pub(super) fn jet_scheduler_sleep_duration(
    _wait_kind: &'static str,
    duration: std::time::Duration,
) {
    std::thread::sleep(duration);
}

pub(super) fn jet_task_delay_ms_defaulted(millis: i64) -> u64 {
    millis.max(0) as u64
}

pub(super) fn jet_task_delay_duration_ns_defaulted(nanos: i64) -> std::time::Duration {
    std::time::Duration::from_nanos(nanos.max(0) as u64)
}

pub(super) fn jet_task_sleep_ms_defaulted(millis: i64) {
    jet_task_sleep_duration_defaulted(std::time::Duration::from_millis(
        jet_task_delay_ms_defaulted(millis),
    ));
}

pub(super) fn jet_task_sleep_duration_ns_defaulted(nanos: i64) {
    jet_task_sleep_duration_defaulted(jet_task_delay_duration_ns_defaulted(nanos));
}

fn jet_task_sleep_duration_defaulted(duration: std::time::Duration) {
    if !jet_scheduler_shielded()
        && jet_std::jet_task_deadline_if_expired(jet_deadline_remaining_ms(), "time sleep")
            .is_some()
    {
        jet_deadline_exceeded("time sleep");
    }
    jet_scheduler_sleep_duration("time sleep", duration);
    if !jet_scheduler_shielded()
        && jet_std::jet_task_deadline_if_expired(jet_deadline_remaining_ms(), "time sleep")
            .is_some()
    {
        jet_deadline_exceeded("time sleep");
    }
}

fn jet_scheduler_shielded() -> bool {
    false
}

fn jet_deadline_exceeded(_wait_kind: &str) -> ! {
    unreachable!("comptime has no installed deadline")
}

pub(super) use jet_foundation::Monotonic::jet_time_monotonic_now_ns;
// Comptime has no resident Scheduler scope. Keep wall-clock control explicit:
// the shared deadline kernel sees no provider rather than sampling a host value.
fn jet_scheduler_world_now_ms() -> Option<i64> {
    None
}
include!("../../../../jet-codegen/src/Prelude/Deadline.rs");
include!("../../../../jet-codegen/src/Prelude/CoreLib/Top/TimeSleep.rs");

pub(super) fn jet_time_sleep_until(deadline_ns: i64) {
    let remaining = deadline_ns
        .saturating_sub(jet_time_monotonic_now_ns())
        .max(0);
    jet_std_time_sleep_duration_ns(remaining);
}
