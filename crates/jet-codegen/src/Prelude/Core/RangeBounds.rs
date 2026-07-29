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
