// The compiler/runtime seam owns this source so every in-process adapter calls
// one epoch. Foundation has no resident scheduler; the absent provider keeps
// this carrier independent while AOT/JIT define the scheduler-backed hook.
fn jet_scheduler_world_monotonic_now_ns() -> Option<i64> {
    None
}
include!("../../jet-codegen/src/Prelude/Core/TimeMonotonic.rs");
