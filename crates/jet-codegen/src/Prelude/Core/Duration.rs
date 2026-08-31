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

/// D-TIMEDEPTH1: fixed-duration units are resolved once in the Prelude. The
/// string form is used by Temporal-style methods; the older enum-backed `.in`
/// path remains a separate checked whole-unit read.
pub(crate) fn jet_duration_kernel_unit_nanoseconds(unit: &str) -> Option<i128> {
    match unit {
        "nanosecond" | "nanoseconds" | "ns" => Some(1),
        "microsecond" | "microseconds" | "us" | "µs" => Some(1_000),
        "millisecond" | "milliseconds" | "ms" => Some(1_000_000),
        "second" | "seconds" | "s" => Some(1_000_000_000),
        "minute" | "minutes" | "min" => Some(60_000_000_000),
        "hour" | "hours" | "h" => Some(3_600_000_000_000),
        "day" | "days" | "d" => Some(86_400_000_000_000),
        "week" | "weeks" | "w" => Some(604_800_000_000_000),
        _ => None,
    }
}

pub(crate) fn jet_duration_kernel_total_in(value: i64, unit: &str) -> f64 {
    jet_duration_kernel_unit_nanoseconds(unit)
        .map(|unit_ns| value as f64 / unit_ns as f64)
        .unwrap_or(0.0)
}

fn jet_duration_kernel_round_quotient(value: i128, quantum: i128, mode: &str) -> Option<i128> {
    if quantum <= 0 {
        return None;
    }
    let trunc = value / quantum;
    let remainder = value % quantum;
    if remainder == 0 {
        return Some(trunc);
    }
    let sign = if value < 0 { -1 } else { 1 };
    let away = trunc + sign;
    let floor = if value < 0 { trunc - 1 } else { trunc };
    let ceil = if value > 0 { trunc + 1 } else { trunc };
    match mode {
        "trunc" | "toward_zero" => Some(trunc),
        "expand" | "away_from_zero" => Some(away),
        "floor" => Some(floor),
        "ceil" => Some(ceil),
        "half_trunc" | "halfTrunc" | "half-toward-zero" => {
            let twice = remainder.abs().checked_mul(2)?;
            match twice.cmp(&quantum) {
                std::cmp::Ordering::Less => Some(trunc),
                std::cmp::Ordering::Greater => Some(away),
                std::cmp::Ordering::Equal => Some(trunc),
            }
        }
        "half_expand" | "halfExpand" | "half-away-from-zero" => {
            let twice = remainder.abs().checked_mul(2)?;
            if twice < quantum {
                Some(trunc)
            } else {
                Some(away)
            }
        }
        "half_even" | "halfEven" => {
            let twice = remainder.abs().checked_mul(2)?;
            match twice.cmp(&quantum) {
                std::cmp::Ordering::Less => Some(trunc),
                std::cmp::Ordering::Greater => Some(away),
                std::cmp::Ordering::Equal if trunc % 2 == 0 => Some(trunc),
                std::cmp::Ordering::Equal => Some(away),
            }
        }
        "half_ceil" | "halfCeil" | "half-toward-positive" => {
            let twice = remainder.abs().checked_mul(2)?;
            if twice < quantum {
                Some(trunc)
            } else if twice > quantum {
                Some(away)
            } else {
                Some(ceil)
            }
        }
        "half_floor" | "halfFloor" | "half-toward-negative" => {
            let twice = remainder.abs().checked_mul(2)?;
            if twice < quantum {
                Some(trunc)
            } else if twice > quantum {
                Some(away)
            } else {
                Some(floor)
            }
        }
        _ => None,
    }
}

pub(crate) fn jet_duration_kernel_round_i128(
    value: i128,
    unit: &str,
    increment: i64,
    mode: &str,
) -> Option<i128> {
    if increment <= 0 {
        return None;
    }
    let quantum = jet_duration_kernel_unit_nanoseconds(unit)?.checked_mul(increment as i128)?;
    jet_duration_kernel_round_quotient(value, quantum, mode)?.checked_mul(quantum)
}

pub(crate) fn jet_duration_kernel_round(
    value: i64,
    unit: &str,
    increment: i64,
    mode: &str,
) -> Option<i64> {
    jet_duration_kernel_round_i128(value as i128, unit, increment, mode)?
        .try_into()
        .ok()
}

pub(crate) fn jet_duration_kernel_abs(value: i64) -> i64 {
    value.saturating_abs()
}

pub(crate) fn jet_duration_kernel_negated(value: i64) -> i64 {
    value.saturating_neg()
}

pub(crate) fn jet_duration_kernel_sign(value: i64) -> i64 {
    value.signum()
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
