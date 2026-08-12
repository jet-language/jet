// Comptime has no resident scheduler host. This is the one host-side
// scheduler primitive for the shared deadline kernel; the policy itself
// remains in Prelude/CoreLib/Top/TimeSleep.rs.
pub(super) fn jet_scheduler_sleep_ms(millis: u64) {
    std::thread::sleep(std::time::Duration::from_millis(millis));
}

fn jet_deadline_exceeded(_wait_kind: &str) -> ! {
    unreachable!("comptime has no installed deadline")
}

include!("../../../../jet-codegen/src/Prelude/Deadline.rs");
include!("../../../../jet-codegen/src/Prelude/CoreLib/Top/TimeSleep.rs");
