// Shared Set and Rank algebra.
//
// AOT emission and the JIT host both marshal to these functions.  Keep the
// operation laws here so an execution tier cannot quietly grow a second set
// implementation.

// Tier-0/comptime values cannot implement `Hash`/`Ord` as a whole because
// they also carry closures and diagnostics.  Keep the same six laws available
// over their equality/comparison adapter instead of reimplementing them in
// the evaluator.
pub(crate) fn jet_set_union_by<T: Clone, F: Fn(&T, &T) -> bool>(
    left: &[T],
    right: &[T],
    equal: F,
) -> Vec<T> {
    let mut out = left.to_vec();
    for value in right {
        if !out.iter().any(|candidate| equal(candidate, value)) {
            out.push(value.clone());
        }
    }
    out
}

pub(crate) fn jet_set_intersection_by<T: Clone, F: Fn(&T, &T) -> bool>(
    left: &[T],
    right: &[T],
    equal: F,
) -> Vec<T> {
    left.iter()
        .filter(|value| right.iter().any(|candidate| equal(value, candidate)))
        .cloned()
        .collect()
}

pub(crate) fn jet_set_difference_by<T: Clone, F: Fn(&T, &T) -> bool>(
    left: &[T],
    right: &[T],
    equal: F,
) -> Vec<T> {
    left.iter()
        .filter(|value| !right.iter().any(|candidate| equal(value, candidate)))
        .cloned()
        .collect()
}

pub(crate) fn jet_set_symmetric_difference_by<T: Clone, F: Fn(&T, &T) -> bool>(
    left: &[T],
    right: &[T],
    equal: F,
) -> Vec<T> {
    let mut out = jet_set_difference_by(left, right, &equal);
    out.extend(jet_set_difference_by(right, left, equal));
    out
}

pub(crate) fn jet_set_is_subset_by<T, F: Fn(&T, &T) -> bool>(
    left: &[T],
    right: &[T],
    equal: F,
) -> bool {
    left.iter()
        .all(|value| right.iter().any(|candidate| equal(value, candidate)))
}

pub(crate) fn jet_set_is_superset_by<T, F: Fn(&T, &T) -> bool>(
    left: &[T],
    right: &[T],
    equal: F,
) -> bool {
    jet_set_is_subset_by(right, left, equal)
}

pub(crate) fn jet_set_is_disjoint_by<T, F: Fn(&T, &T) -> bool>(
    left: &[T],
    right: &[T],
    equal: F,
) -> bool {
    left.iter()
        .all(|value| !right.iter().any(|candidate| equal(value, candidate)))
}

pub(crate) fn jet_set_insert<T: Eq + std::hash::Hash>(
    set: &mut std::collections::HashSet<T>,
    value: T,
) {
    set.insert(value);
}

pub(crate) fn jet_set_remove<T: Eq + std::hash::Hash>(
    set: &mut std::collections::HashSet<T>,
    value: T,
) {
    set.remove(&value);
}

/// D-TAG1: `Tally<T>.add(v)` — one more occurrence of `v`.
pub(crate) fn jet_bag_add<T: Eq + std::hash::Hash>(
    bag: &mut std::collections::HashMap<T, usize>,
    value: T,
) {
    *bag.entry(value).or_insert(0) += 1;
}

/// D-TAG1: `Tally<T>.remove(v)` — one fewer occurrence of `v`. Removing an
/// absent member is a no-op: the count never underflows and no member is
/// fabricated.
pub(crate) fn jet_bag_remove<T: Eq + std::hash::Hash>(
    bag: &mut std::collections::HashMap<T, usize>,
    value: T,
) {
    if let Some(count) = bag.get_mut(&value) {
        if *count > 1 {
            *count -= 1;
        } else {
            bag.remove(&value);
        }
    }
}
pub(crate) fn jet_set_update<T: Eq + std::hash::Hash + Clone>(
    left: &mut std::collections::HashSet<T>,
    right: &std::collections::HashSet<T>,
) {
    left.extend(right.iter().cloned());
}
pub(crate) fn jet_set_difference_update<T: Eq + std::hash::Hash>(
    left: &mut std::collections::HashSet<T>,
    right: &std::collections::HashSet<T>,
) {
    left.retain(|value| !right.contains(value));
}

pub(crate) fn jet_set_intersection_update<T: Eq + std::hash::Hash>(
    left: &mut std::collections::HashSet<T>,
    right: &std::collections::HashSet<T>,
) {
    left.retain(|value| right.contains(value));
}

pub(crate) fn jet_set_symmetric_difference_update<T: Eq + std::hash::Hash + Clone>(
    left: &mut std::collections::HashSet<T>,
    right: &std::collections::HashSet<T>,
) {
    let next = left.symmetric_difference(right).cloned().collect();
    *left = next;
}

pub(crate) fn jet_set_union<T: Eq + std::hash::Hash + Clone>(
    left: &std::collections::HashSet<T>,
    right: &std::collections::HashSet<T>,
) -> std::collections::HashSet<T> {
    left.union(right).cloned().collect()
}

pub(crate) fn jet_set_intersection<T: Eq + std::hash::Hash + Clone>(
    left: &std::collections::HashSet<T>,
    right: &std::collections::HashSet<T>,
) -> std::collections::HashSet<T> {
    left.intersection(right).cloned().collect()
}

pub(crate) fn jet_set_difference<T: Eq + std::hash::Hash + Clone>(
    left: &std::collections::HashSet<T>,
    right: &std::collections::HashSet<T>,
) -> std::collections::HashSet<T> {
    left.difference(right).cloned().collect()
}

pub(crate) fn jet_set_symmetric_difference<T: Eq + std::hash::Hash + Clone>(
    left: &std::collections::HashSet<T>,
    right: &std::collections::HashSet<T>,
) -> std::collections::HashSet<T> {
    left.symmetric_difference(right).cloned().collect()
}

pub(crate) fn jet_set_is_subset<T: Eq + std::hash::Hash>(
    left: &std::collections::HashSet<T>,
    right: &std::collections::HashSet<T>,
) -> bool {
    left.is_subset(right)
}

pub(crate) fn jet_set_is_superset<T: Eq + std::hash::Hash>(
    left: &std::collections::HashSet<T>,
    right: &std::collections::HashSet<T>,
) -> bool {
    left.is_superset(right)
}

pub(crate) fn jet_set_is_disjoint<T: Eq + std::hash::Hash>(
    left: &std::collections::HashSet<T>,
    right: &std::collections::HashSet<T>,
) -> bool {
    left.is_disjoint(right)
}

pub(crate) fn jet_sorted_set_union<T: Ord + Clone>(
    left: &std::collections::BTreeSet<T>,
    right: &std::collections::BTreeSet<T>,
) -> std::collections::BTreeSet<T> {
    left.union(right).cloned().collect()
}

pub(crate) fn jet_sorted_set_intersection<T: Ord + Clone>(
    left: &std::collections::BTreeSet<T>,
    right: &std::collections::BTreeSet<T>,
) -> std::collections::BTreeSet<T> {
    left.intersection(right).cloned().collect()
}

pub(crate) fn jet_sorted_set_difference<T: Ord + Clone>(
    left: &std::collections::BTreeSet<T>,
    right: &std::collections::BTreeSet<T>,
) -> std::collections::BTreeSet<T> {
    left.difference(right).cloned().collect()
}

pub(crate) fn jet_sorted_set_symmetric_difference<T: Ord + Clone>(
    left: &std::collections::BTreeSet<T>,
    right: &std::collections::BTreeSet<T>,
) -> std::collections::BTreeSet<T> {
    left.symmetric_difference(right).cloned().collect()
}

pub(crate) fn jet_sorted_set_is_subset<T: Ord>(
    left: &std::collections::BTreeSet<T>,
    right: &std::collections::BTreeSet<T>,
) -> bool {
    left.is_subset(right)
}

pub(crate) fn jet_sorted_set_is_superset<T: Ord>(
    left: &std::collections::BTreeSet<T>,
    right: &std::collections::BTreeSet<T>,
) -> bool {
    left.is_superset(right)
}

pub(crate) fn jet_sorted_set_is_disjoint<T: Ord>(
    left: &std::collections::BTreeSet<T>,
    right: &std::collections::BTreeSet<T>,
) -> bool {
    left.is_disjoint(right)
}
