/// Monotonic scalar used by erased interpreter carriers. Native AOT/JIT keep
/// the opaque Instant; TIR deopt stores this same Prelude clock sample.
pub fn jet_time_monotonic_now_ns() -> i64 {
    static EPOCH: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    EPOCH
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_nanos()
        .min(i64::MAX as u128) as i64
}
