/// Monotonic scalar used by the shared Instant carrier. AOT stores the scalar
/// in the Prelude value; JIT keeps its handle opaque; TIR deopt stores the same
/// Prelude clock sample.
pub fn jet_time_monotonic_now_ns() -> i64 {
    static EPOCH: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    EPOCH
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_nanos()
        .min(i64::MAX as u128) as i64
}
