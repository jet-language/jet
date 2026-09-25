//! Source-owned `core.collections.set` behavior and ownership across all tiers.

mod common;
mod tir_support;

#[test]
fn core_set_order_ownership_and_algebra_match_across_tiers() {
    tir_support::assert_tiers_agree(
        "core_set_foundations",
        r#"
use core.collections.set as api

fn run() {
    seeded :: api.from_list([String]{"b", "a", "b", "c"})
    print(api.to_list(seeded))
    print(api.len(seeded))
    print(api.is_empty(api.new()))

    print(api.to_list(api.add(seeded, "a")))
    print(api.to_list(api.add(seeded, "d")))
    print(api.to_list(seeded))
    print(api.to_list(api.discard(seeded, "a")))
    print(api.to_list(api.remove(seeded, "b") ?? api.new()))
    print(api.to_list(api.remove(seeded, "missing") ?? api.new()))

    other :: api.from_list([String]{"c", "e", "b"})
    print(api.to_list(api.union(seeded, other)))
    print(api.to_list(api.intersection(seeded, other)))
    print(api.to_list(api.difference(seeded, other)))
    print(api.to_list(api.symmetric_difference(seeded, other)))
    print(api.issubset(seeded, api.union(seeded, other)))
    print(api.issuperset(api.union(seeded, other), seeded))
    print(api.isdisjoint(seeded, api.from_list([String]{"z"})))

    cloned_list := api.to_list(api.clone_set(seeded))
    cloned_list.push("tail")
    print(api.to_list(seeded))
    print(cloned_list)
    exported_list := api.to_list(seeded)
    exported_list.push("tail")
    print(api.to_list(seeded))
    print(exported_list)
}
"#,
        "[b, a, c]\n3\ntrue\n[b, a, c]\n[b, a, c, d]\n[b, a, c]\n[b, c]\n[a, c]\n[]\n[b, a, c, e]\n[b, c]\n[a]\n[a, e]\ntrue\ntrue\ntrue\n[b, a, c]\n[b, a, c, tail]\n[b, a, c]\n[b, a, c, tail]\n",
    );
}

#[test]
fn core_set_public_carrier_keeps_backend_order_on_all_tiers() {
    tir_support::assert_tiers_agree(
        "core_set_public_carrier",
        r#"
use core.collections.set as api

fn run() {
    raw :: api.StringSet{items: [String]{"a", "a", "b"}}
    print(api.to_list(api.discard(raw, "missing")))
    print(api.to_list(api.intersection(raw, api.from_list([String]{"a", "b"}))))
    print(api.to_list(api.difference(raw, api.from_list([String]{"b"}))))
    print(api.to_list(api.symmetric_difference(raw, api.from_list([String]{"b"}))))
    print(api.to_list(api.remove(raw, "b") ?? api.new()))
}
"#,
        "[a, a, b]\n[a, a, b]\n[a, a]\n[a, a]\n[a, a]\n",
    );
}
