// D-TIMERES1=A: one checked nanosecond arithmetic kernel for every tier.

pub(crate) fn jet_duration_kernel_from_int(value: i64, scale: i64) -> Option<i64> {
    value.checked_mul(scale)
}

pub(crate) fn jet_duration_kernel_from_float(value: f64, scale: i64) -> Option<i64> {
    let nanoseconds = value * scale as f64;
    (nanoseconds.is_finite()
        && nanoseconds >= i64::MIN as f64
        && nanoseconds < 9_223_372_036_854_775_808.0)
        .then_some(nanoseconds.trunc() as i64)
}

pub(crate) fn jet_duration_kernel_int_error_reason() -> &'static str {
    "duration is outside the supported range"
}

pub(crate) fn jet_duration_kernel_float_error_reason() -> &'static str {
    "duration must be finite and inside the supported range"
}

pub(crate) fn jet_duration_kernel_in(value: i64, scale: i64) -> i64 {
    value / scale
}

pub(crate) fn jet_duration_kernel_is_zero(value: i64) -> bool {
    value == 0
}

pub(crate) fn jet_duration_kernel_total_seconds(value: i64) -> i64 {
    value / 1_000_000_000
}

pub(crate) fn jet_duration_kernel_difference(left: i64, right: i64) -> i64 {
    left.saturating_sub(right)
}
