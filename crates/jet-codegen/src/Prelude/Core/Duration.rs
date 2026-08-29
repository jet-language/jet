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

/// D-TIMERES1=A: scalar duration arithmetic is checked at the one nanosecond
/// carrier boundary. A failed multiply or divide has no Duration value.
pub(crate) fn jet_duration_kernel_scale(value: i64, factor: i64) -> Option<i64> {
    value.checked_mul(factor)
}

pub(crate) fn jet_duration_kernel_divide(value: i64, factor: i64) -> Option<i64> {
    value.checked_div(factor)
}

pub(crate) fn jet_duration_kernel_scale_error_reason() -> &'static str {
    "duration scaling overflowed or divided by zero"
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

pub(crate) fn jet_duration_kernel_seconds_value(value: i64) -> f64 {
    value as f64 / 1_000_000_000.0
}

pub(crate) fn jet_duration_kernel_add(left: i64, right: i64) -> i64 {
    left.saturating_add(right)
}

pub(crate) fn jet_duration_kernel_sub(left: i64, right: i64) -> i64 {
    left.saturating_sub(right)
}

pub(crate) fn jet_duration_kernel_difference(left: i64, right: i64) -> i64 {
    left.saturating_sub(right)
}

/// D-TYPE2-TIME1=A: the one `JetShow` rendering of the canonical nanosecond
/// carrier. AOT's `impl JetShow for Duration`, the Cranelift host and the
/// evaluator all marshal the carrier and call this; no engine re-encodes the
/// `ns` suffix.
pub(crate) fn jet_duration_kernel_show(value: i64) -> String {
    format!("{value}ns")
}
