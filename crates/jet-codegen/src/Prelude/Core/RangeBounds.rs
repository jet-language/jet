// D-RANGE-VALUE1=A: one bounds rule for AOT, resident JIT, and TIR evaluation.
// The returned end is exclusive so empty half-open ranges remain representable.
pub(crate) fn jet_range_bounds(
    start: i64,
    end: i64,
    exclusive: bool,
    len: i64,
) -> Option<(i64, i64)> {
    if start < 0 || end < 0 || start > end {
        return None;
    }
    let end_exclusive = if exclusive { end } else { end.checked_add(1)? };
    (end_exclusive <= len).then_some((start, end_exclusive))
}

pub(crate) fn jet_range_contains(
    start: i64,
    end: i64,
    exclusive: bool,
    value: i64,
) -> bool {
    value >= start && if exclusive { value < end } else { value <= end }
}

pub(crate) fn jet_range_structural_text(start: i64, end: i64, exclusive: bool) -> String {
    jet_debug_range(start, end, exclusive)
}

pub(crate) fn jet_range_equal(
    left_start: i64,
    left_end: i64,
    left_exclusive: bool,
    right_start: i64,
    right_end: i64,
    right_exclusive: bool,
) -> bool {
    left_start == right_start
        && left_end == right_end
        && left_exclusive == right_exclusive
}
