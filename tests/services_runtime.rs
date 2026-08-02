//! Focused failure-path checks for the production `core.services` Prelude.

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{build_and_run, have_rustc, run_default_multi};

const SOURCE: &str = r#"
use core.services as services

fn run() {
    tree := services.tree("delivery")
    services.set_delivery(&tree, services.delivery_durable()) ?? panic("delivery")
    worker :: services.worker(&tree, "worker", 1) ?? panic("worker")
    services.start(&tree) ?? panic("start")

    services.send_durable(&tree, worker, "first", "k1") ?? panic("first")
    services.send_durable(&tree, worker, "first", "k1") ?? panic("duplicate")
    conflicting :: services.send_durable(&tree, worker, "different", "k1")
    if conflicting == {
        .Ok(_) -> { print("conflict:accepted") }
        .Err(_) -> { print("conflict:rejected") }
    }
    full :: services.send_durable(&tree, worker, "second", "k2")
    if full == {
        .Ok(_) -> { print("full:accepted") }
        .Err(_) -> { print("full:rejected") }
    }
    print("dead_letters:{services.dead_letter_count(tree)}")

    services.receive(&tree, worker) ?? panic("receive")
    services.drain_worker(&tree, worker) ?? panic("drain")
    stopped_receive :: services.receive(&tree, worker)
    if stopped_receive == {
        .Ok(_) -> { print("drained_receive:accepted") }
        .Err(_) -> { print("drained_receive:rejected") }
    }
}
"#;

#[test]
fn services_reject_duplicate_conflicts_full_mailboxes_and_drained_receive_aot() {
    if !have_rustc() {
        return;
    }
    let (code, stdout) = build_and_run("services_failure_paths", SOURCE);
    assert_eq!(code, 0);
    assert_eq!(stdout, "conflict:rejected\nfull:rejected\ndead_letters:1\ndrained_receive:rejected\n");
}

#[test]
fn services_failure_paths_match_default_run() {
    let (code, stdout, stderr) = run_default_multi("services_failure_paths_jit", "main.jet", &[("main.jet", SOURCE)]);
    assert_eq!(code, 0, "default jet run failed: {stderr}");
    assert_eq!(stdout, "conflict:rejected\nfull:rejected\ndead_letters:1\ndrained_receive:rejected\n");
}

const STATE_AND_LIFECYCLE_SOURCE: &str = r#"
use core.services as services

fn run() {
    snapshot := services.tree("snapshot")
    services.set_state_snapshot(&snapshot) ?? panic("snapshot state")
    snapshot_worker :: services.worker(&snapshot, "worker", 2) ?? panic("snapshot worker")
    services.start(&snapshot) ?? panic("snapshot start")
    services.commit_snapshot(&snapshot, "state-v1") ?? panic("snapshot commit")
    print("snapshot:{services.restore_snapshot(snapshot) ?? panic(\"snapshot restore\")}")
    services.stop(&snapshot) ?? panic("snapshot stop")

    events := services.tree("events")
    services.set_state_event_log(&events) ?? panic("event state")
    event_worker :: services.worker(&events, "worker", 2) ?? panic("event worker")
    services.start(&events) ?? panic("event start")
    services.append_event(&events, "first") ?? panic("event one")
    services.append_event(&events, "second") ?? panic("event two")
    print("events:{services.replay_events(events)}")
    print("event_count:{services.event_count(events)}")
    services.stop(&events) ?? panic("event stop")

    workflow := services.tree("workflow")
    workflow_worker :: services.worker(&workflow, "worker", 2) ?? panic("workflow worker")
    services.start(&workflow) ?? panic("workflow start")
    run_id :: services.workflow_start(&workflow, "checkout", 1) ?? panic("workflow id")
    same_run :: services.workflow_start(&workflow, "checkout", 1) ?? panic("workflow duplicate")
    services.workflow_step(&workflow, run_id, "charge") ?? panic("workflow step")
    print("workflow:{run_id}:{same_run}:{services.workflow_history(workflow, run_id) ?? panic(\"workflow history\")}")
    versioned :: services.workflow_start(&workflow, "checkout", 2)
    if versioned == {
        .Ok(_) -> { print("workflow_version:accepted") }
        .Err(_) -> { print("workflow_version:rejected") }
    }
    services.stop(&workflow) ?? panic("workflow stop")

    cluster := services.tree("cluster")
    endpoint :: services.worker(&cluster, "api", 2) ?? panic("cluster worker")
    services.start(&cluster) ?? panic("cluster start")
    services.directory_register(&cluster, "api", endpoint) ?? panic("directory register")
    services.drain_worker(&cluster, endpoint) ?? panic("cluster drain")
    handed :: services.handoff_generation(&cluster) ?? panic("handoff")
    current :: services.directory_resolve(cluster, "api") ?? panic("directory resolve")
    print("generation:{handed}:{services.directory_generation(cluster)}:{services.endpoint_show(current)}")
    stale :: services.send(&cluster, endpoint, "late")
    if stale == {
        .Ok(_) -> { print("stale:accepted") }
        .Err(_) -> { print("stale:rejected") }
    }
    rolled :: services.rollback_generation(&cluster) ?? panic("rollback")
    print("rollback:{rolled}:{services.directory_generation(cluster)}")
    services.chaos_fail(&cluster) ?? panic("chaos")
    print(services.observe(cluster))
    services.stop(&cluster) ?? panic("cluster stop")
}
"#;

#[test]
fn services_state_workflow_identity_and_upgrade_are_real_aot_paths() {
    if !have_rustc() {
        return;
    }
    let (code, stdout) = build_and_run("services_state_lifecycle", STATE_AND_LIFECYCLE_SOURCE);
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "snapshot:state-v1\nevents:first|second\nevent_count:2\nworkflow:1:1:start@v1|step:charge\nworkflow_version:rejected\ngeneration:2:2:Endpoint(cluster/api@g2)\nstale:rejected\nrollback:1:1\nObserve(workers=1, started=true, generation=1, dead_letters=0, events=0, chaos=1, draining=0)\n"
    );
}

#[test]
fn services_state_workflow_identity_and_upgrade_match_default_run() {
    let (code, stdout, stderr) = run_default_multi(
        "services_state_lifecycle_jit",
        "main.jet",
        &[("main.jet", STATE_AND_LIFECYCLE_SOURCE)],
    );
    assert_eq!(code, 0, "default jet run failed: {stderr}");
    assert_eq!(
        stdout,
        "snapshot:state-v1\nevents:first|second\nevent_count:2\nworkflow:1:1:start@v1|step:charge\nworkflow_version:rejected\ngeneration:2:2:Endpoint(cluster/api@g2)\nstale:rejected\nrollback:1:1\nObserve(workers=1, started=true, generation=1, dead_letters=0, events=0, chaos=1, draining=0)\n"
    );
}
