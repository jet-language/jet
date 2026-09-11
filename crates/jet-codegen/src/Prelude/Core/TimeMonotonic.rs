/// Monotonic scalar used by the shared Instant carrier. AOT stores the scalar
/// in the Prelude value; JIT keeps its handle opaque; TIR deopt stores the same
/// Prelude clock sample.
///
/// Foundation compiles this carrier without the scheduler. The provider hook
/// keeps that build independent while letting the one Prelude scheduler install
/// its controlled-world clock at the execution boundary.
pub type JetMonotonicProvider = fn() -> Option<i64>;

thread_local! {
    static JET_MONOTONIC_PROVIDER: std::cell::Cell<Option<JetMonotonicProvider>> =
        const { std::cell::Cell::new(None) };
}

pub fn jet_time_monotonic_provider_set(
    provider: Option<JetMonotonicProvider>,
) -> Option<JetMonotonicProvider> {
    JET_MONOTONIC_PROVIDER.with(|current| current.replace(provider))
}

fn jet_time_monotonic_controlled_now_ns() -> Option<i64> {
    JET_MONOTONIC_PROVIDER.with(|provider| provider.get().and_then(|provider| provider()))
}

pub fn jet_time_monotonic_now_ns() -> i64 {
    if let Some(now) = jet_time_monotonic_controlled_now_ns() {
        return now;
    }
    if let Some(now) = jet_scheduler_world_monotonic_now_ns() {
        return now;
    }
    static EPOCH: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    EPOCH
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_nanos()
        .min(i64::MAX as u128) as i64
}
