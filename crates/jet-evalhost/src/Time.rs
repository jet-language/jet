//! `core.time` / civil-time marshalling hosts for the shared Prelude kernel.

// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.

use super::Concurrency;

pub(crate) mod time_rt {
    use jet_codegen::scheduler::jet_time_wall_override_ms;
    // `TimeInstant.rs` names the shared sleep seam unqualified; the resident
    // adapter below routes it to the ambient scheduler.
    use super::jet_std_time_sleep_duration_ns;
    fn jet_scheduler_world_monotonic_now_ns() -> Option<i64> {
        jet_codegen::scheduler::jet_scheduler_world_monotonic_now_ns()
    }
    fn jet_scheduler_world_reject_uncontrolled(effect: &'static str) {
        jet_codegen::scheduler::jet_scheduler_world_reject_uncontrolled(effect);
    }
    include!("../../jet-codegen/src/Prelude/Core/TimeMonotonic.rs");
    include!("../../jet-codegen/src/Prelude/Core/Duration.rs");
    fn jet_scheduler_world_now_ms() -> Option<i64> {
        jet_codegen::scheduler::jet_scheduler_world_now_ms()
    }
    include!("../../jet-codegen/src/Prelude/Core/Time.rs");
    include!("../../jet-codegen/src/Prelude/Core/TimeInstant.rs");
    include!("../../jet-codegen/src/Prelude/Core/Realtime.rs");
}
/// The JIT adapter keeps the canonical nanosecond sleep contract while
/// delegating parking and cancellation to the shared scheduler host.
pub(crate) fn jet_std_time_sleep_duration_ns(nanos: i64) {
    Concurrency::ambient_time_sleep(nanos);
}

