//! Source-owned array/list algorithms in `core.collections` across all tiers.

mod common;
mod tir_support;

#[test]
fn core_collection_array_algorithms_match_across_tiers() {
    tir_support::assert_tiers_agree(
        "core_array_foundations",
        r#"
use core.collections as api

fn run() {
    values :: [Int]{7, 1, 4, 1, 9}
    heap :: api.heapify(values)
    print(values == [Int]{7, 1, 4, 1, 9})
    print(heap == [Int]{1, 1, 4, 7, 9})

    pushed :: api.heappush(heap, 0)
    print(pushed == [Int]{0, 1, 1, 7, 9, 4})
    popped :: api.heappop(pushed)
    print(popped.value ?? -1)
    print(popped.heap == [Int]{1, 4, 1, 7, 9})

    print(api.nsmallest(3, values) == [Int]{1, 1, 4})
    print(api.nlargest(2, values) == [Int]{9, 7})
    print(api.merge_sorted([Int]{1, 3, 5}, [Int]{2, 3, 4}) == [Int]{1, 2, 3, 3, 4, 5})

    sorted :: [Int]{1, 3, 3, 5}
    print(api.bisect_left(sorted, 3))
    print(api.bisect_right(sorted, 3))
    print(api.insort_left(sorted, 3) == [Int]{1, 3, 3, 3, 5})
    print(api.insort_right(sorted, 3) == [Int]{1, 3, 3, 3, 5})
}
"#,
        "true\ntrue\ntrue\n0\ntrue\ntrue\ntrue\ntrue\n1\n3\ntrue\ntrue\n",
    );
}

#[test]
fn core_collection_ordered_map_lookup_scans_parallel_arrays() {
    tir_support::assert_tiers_agree(
        "core_array_ordered_map_lookup",
        r#"
use core.collections as api

fn run() {
    m :: api.map_set(api.map_set(api.ordered_map(), "first", "one"), "second", "two")
    print(api.map_get(m, "second") ?? "missing")
    print(api.map_get(m, "missing") == None)
    print(api.map_contains(m, "second"))
    print(!api.map_contains(m, "missing"))
}
"#,
        "two\ntrue\ntrue\ntrue\n",
    );
}
