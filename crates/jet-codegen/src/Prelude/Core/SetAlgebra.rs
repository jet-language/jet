// Shared Set and SortedSet algebra.
//
// AOT emission and the JIT host both marshal to these functions.  Keep the
// operation laws here so an execution tier cannot quietly grow a second set
// implementation.

use std::collections::{BTreeSet, HashSet};
use std::hash::Hash;

pub(crate) fn jet_set_union<T: Eq + Hash + Clone>(left: &HashSet<T>, right: &HashSet<T>) -> HashSet<T> {
    left.union(right).cloned().collect()
}

pub(crate) fn jet_set_intersection<T: Eq + Hash + Clone>(left: &HashSet<T>, right: &HashSet<T>) -> HashSet<T> {
    left.intersection(right).cloned().collect()
}

pub(crate) fn jet_set_difference<T: Eq + Hash + Clone>(left: &HashSet<T>, right: &HashSet<T>) -> HashSet<T> {
    left.difference(right).cloned().collect()
}

pub(crate) fn jet_set_symmetric_difference<T: Eq + Hash + Clone>(
    left: &HashSet<T>,
    right: &HashSet<T>,
) -> HashSet<T> {
    left.symmetric_difference(right).cloned().collect()
}

pub(crate) fn jet_set_is_subset<T: Eq + Hash>(left: &HashSet<T>, right: &HashSet<T>) -> bool {
    left.is_subset(right)
}

pub(crate) fn jet_set_is_superset<T: Eq + Hash>(left: &HashSet<T>, right: &HashSet<T>) -> bool {
    left.is_superset(right)
}

pub(crate) fn jet_set_is_disjoint<T: Eq + Hash>(left: &HashSet<T>, right: &HashSet<T>) -> bool {
    left.is_disjoint(right)
}

pub(crate) fn jet_sorted_set_union<T: Ord + Clone>(
    left: &BTreeSet<T>,
    right: &BTreeSet<T>,
) -> BTreeSet<T> {
    left.union(right).cloned().collect()
}

pub(crate) fn jet_sorted_set_intersection<T: Ord + Clone>(
    left: &BTreeSet<T>,
    right: &BTreeSet<T>,
) -> BTreeSet<T> {
    left.intersection(right).cloned().collect()
}

pub(crate) fn jet_sorted_set_difference<T: Ord + Clone>(
    left: &BTreeSet<T>,
    right: &BTreeSet<T>,
) -> BTreeSet<T> {
    left.difference(right).cloned().collect()
}

pub(crate) fn jet_sorted_set_symmetric_difference<T: Ord + Clone>(
    left: &BTreeSet<T>,
    right: &BTreeSet<T>,
) -> BTreeSet<T> {
    left.symmetric_difference(right).cloned().collect()
}

pub(crate) fn jet_sorted_set_is_subset<T: Ord>(left: &BTreeSet<T>, right: &BTreeSet<T>) -> bool {
    left.is_subset(right)
}

pub(crate) fn jet_sorted_set_is_superset<T: Ord>(left: &BTreeSet<T>, right: &BTreeSet<T>) -> bool {
    left.is_superset(right)
}

pub(crate) fn jet_sorted_set_is_disjoint<T: Ord>(left: &BTreeSet<T>, right: &BTreeSet<T>) -> bool {
    left.is_disjoint(right)
}
