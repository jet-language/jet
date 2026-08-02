//! Focused convergence and fail-closed checks for `core.sync`.

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{build_and_run, have_rustc, run_default_multi};

const SOURCE: &str = r#"
use core.sync as sync
use app

fn run() {
    text_a0 :: sync.text_new("r1", "hello")
    text_a :: sync.text_set(text_a0, "r1", "hellp")
    text_b :: sync.text_new("r2", "world")
    print("text_converges:{sync.text_show(sync.text_merge(text_a, text_b)) == sync.text_show(sync.text_merge(text_b, text_a))}")

    left :: sync.map_set(sync.map_new(), "k", "left")
    right :: sync.map_set(sync.map_new(), "k", "right")
    lr :: sync.map_show(sync.map_merge(left, right))
    rl :: sync.map_show(sync.map_merge(right, left))
    print("map_converges:{lr == rl}")

    list_a0 :: sync.list_push(sync.list_new(), "r2", "b")
    list_a :: sync.list_push(list_a0, "r1", "a")
    list_b0 :: sync.list_push(sync.list_new(), "r1", "a")
    list_b :: sync.list_push(list_b0, "r2", "b")
    print("list_converges:{sync.list_show(sync.list_merge(list_a, list_b)) == sync.list_show(sync.list_merge(list_b, list_a))}")

    counter0 :: sync.counter_new("r1", 2)
    counter :: sync.counter_inc(counter0, "r1", 3)
    merged_counter :: sync.counter_merge(counter, counter)
    print("counter_idempotent:{sync.counter_value(merged_counter)}")

    invalid :: sync.policy_new("tickets", "owner == admin")
    if invalid == {
        .Ok(_) -> { print("invalid_policy:accepted") }
        .Err(_) -> { print("invalid_policy:rejected") }
    }
    policy :: sync.policy_new("tickets", "owner == user") ?? panic("policy")
    print("owner_allowed:{sync.policy_allows(policy, "alice", "alice")}")
    print("owner_denied:{sync.policy_allows(policy, "alice", "bob")}")
    public :: sync.policy_new("public", "true") ?? panic("public policy")
    print("public_allowed:{sync.policy_allows(public, "alice", "bob")}")

    first :: app.sync(lr, over: "sync-laws")
    second :: app.sync(rl, over: "sync-laws")
    print(first)
    print(second)
}
"#;

#[test]
fn sync_laws_hold_on_aot_path() {
    if !have_rustc() {
        return;
    }
    let (code, stdout) = build_and_run("sync_laws", SOURCE);
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "text_converges:true\nmap_converges:true\nlist_converges:true\ncounter_idempotent:5\ninvalid_policy:rejected\nowner_allowed:true\nowner_denied:false\npublic_allowed:true\nSyncOver(session=sync-laws, generation=1, doc=SyncMap(k=right))\nSyncOver(session=sync-laws, generation=2, doc=SyncMap(k=right))\n"
    );
}

#[test]
fn sync_laws_hold_on_default_run() {
    let (code, stdout, stderr) = run_default_multi("sync_laws_jit", "main.jet", &[("main.jet", SOURCE)]);
    assert_eq!(code, 0, "default jet run failed: {stderr}");
    assert_eq!(
        stdout,
        "text_converges:true\nmap_converges:true\nlist_converges:true\ncounter_idempotent:5\ninvalid_policy:rejected\nowner_allowed:true\nowner_denied:false\npublic_allowed:true\nSyncOver(session=sync-laws, generation=1, doc=SyncMap(k=right))\nSyncOver(session=sync-laws, generation=2, doc=SyncMap(k=right))\n"
    );
}
