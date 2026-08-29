// D-FLOATSORT1: the total ordering used by every collection sort.
//
// Ordinary Float comparisons keep IEEE partial-order behavior. Sorting needs
// a comparator, so its NaN placement is explicit and stable across engines:
// every NaN follows every non-NaN value, and two NaNs compare equal.
pub(crate) fn jet_float_sort_cmp(left: f64, right: f64) -> std::cmp::Ordering {
    match left.partial_cmp(&right) {
        Some(ordering) => ordering,
        None if left.is_nan() && right.is_nan() => std::cmp::Ordering::Equal,
        None if left.is_nan() => std::cmp::Ordering::Greater,
        None => std::cmp::Ordering::Less,
    }
}
