//! Focused failure-path checks for the production `core.services` Prelude.

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{build_and_run, have_rustc, run_default_multi};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static RESTART_SEQ: AtomicU64 = AtomicU64::new(0);

const AUTHORITY_SOURCE: &str = r#"
use core.path as path
use core.services as services
use core.testing as testing
use core.time as time

fn receipt_id(receipt: ServiceReceipt) => String {
    if receipt == {
        .Accepted(id) -> { return id }
        .Duplicate(id) -> { return id }
        .Retained(id, _) -> { return id }
        .DeadLettered(id) -> { return id }
    }
    return ""
}

fn receipt_kind(receipt: ServiceReceipt) => String {
    if receipt == {
        .Accepted(_) -> { return "accepted" }
        .Duplicate(_) -> { return "duplicate" }
        .Retained(_, _) -> { return "retained" }
        .DeadLettered(_) -> { return "dead" }
        .Rejected(_) -> { return "rejected" }
        .Unavailable(_) -> { return "unavailable" }
    }
    return "unknown"
}

fn run() {
    temp := testing.temp_dir("service-authority")
    store :: path.join(temp, "authority.log")
    retention :: Duration.seconds(86400) ?? panic("retention")
    runtime := services.runtime(store, retention: retention)
    tree := services.tree("orders")
    endpoint :: services.worker(&tree, "orders", 2) ?? panic("worker")
    services.start(&tree) ?? panic("start")

    first :: runtime.send(endpoint, "order", key: "order-1") ?? panic("first")
    id :: receipt_id(first)
    if id == "" { panic("missing receipt id") }
    print("first:accepted")
    recovered := services.runtime(store, retention: retention)
    recovered_duplicate :: recovered.retry(id) ?? panic("recover")
    print("recovered:{receipt_kind(recovered_duplicate)}")
    delivered :: services.receive(&tree, endpoint) ?? panic("deliver")
    print("delivered:{delivered}")
    runtime.commit(id) ?? panic("commit")
    print("committed:ok")

    duplicate :: recovered.send(endpoint, "order", key: "order-1") ?? panic("duplicate")
    print("duplicate:{receipt_kind(duplicate)}")

    retained :: runtime.retain(id) ?? panic("retain")
    print("retain:{receipt_kind(retained)}")
    retry :: recovered.retry(id) ?? panic("retry")
    print("retry:{receipt_kind(retry)}")
    redelivered :: services.receive(&tree, endpoint) ?? panic("redeliver")
    print("redelivered:{redelivered}")
    runtime.commit(id) ?? panic("redelivery commit")
    dead :: runtime.dead_letter(id) ?? panic("dead")
    print("dead:{receipt_kind(dead)}")
    services.stop(&tree) ?? panic("stop")
    stopped := recovered.send(endpoint, "new-order", key: "order-2")
    if stopped == {
        .Ok(_) -> { print("stopped:accepted") }
        .Err(_) -> { print("stopped:rejected") }
    }
    services.start(&tree) ?? panic("restart")
    replay :: recovered.send(endpoint, "order", key: "order-1") ?? panic("replay")
    print("replay:{receipt_kind(replay)}")
}
"#;

#[test]
fn service_authority_receipts_survive_reopen_and_lifecycle() {
    if !have_rustc() {
        return;
    }
    let (code, stdout) = build_and_run("services_authority", AUTHORITY_SOURCE);
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "first:accepted\nrecovered:duplicate\ndelivered:order\ncommitted:ok\nduplicate:duplicate\nretain:retained\nretry:retained\nredelivered:order\ndead:dead\nstopped:rejected\nreplay:dead\n"
    );
}

#[test]
fn service_authority_receipts_match_default_run() {
    let (code, stdout, stderr) = run_default_multi(
        "services_authority_jit",
        "main.jet",
        &[("main.jet", AUTHORITY_SOURCE)],
    );
    assert_eq!(code, 0, "default jet run failed: {stderr}");
    assert_eq!(
        stdout,
        "first:accepted\nrecovered:duplicate\ndelivered:order\ncommitted:ok\nduplicate:duplicate\nretain:retained\nretry:retained\nredelivered:order\ndead:dead\nstopped:rejected\nreplay:dead\n"
    );
}

const RESTART_SOURCE: &str = r#"
use core.env as env
use core.services as services
use core.time as time

fn receipt_id(receipt: ServiceReceipt) => String {
    if receipt == {
        .Accepted(id) -> { return id }
        .Duplicate(id) -> { return id }
        .Retained(id, _) -> { return id }
        .DeadLettered(id) -> { return id }
    }
    return ""
}

fn receipt_kind(receipt: ServiceReceipt) => String {
    if receipt == {
        .Accepted(_) -> { return "accepted" }
        .Duplicate(_) -> { return "duplicate" }
        .Retained(_, _) -> { return "retained" }
        .DeadLettered(_) -> { return "dead" }
        .Rejected(_) -> { return "rejected" }
        .Unavailable(_) -> { return "unavailable" }
    }
    return "unknown"
}

fn run() {
    store :: env.get("JET_SERVICE_AUTH_STORE") ?? panic("store")
    phase :: env.get("JET_SERVICE_AUTH_PHASE") ?? panic("phase")
    retention :: Duration.seconds(86400) ?? panic("retention")
    runtime := services.runtime(store, retention: retention)
    tree := services.tree("restart-orders")
    endpoint :: services.worker(&tree, "orders", 2) ?? panic("worker")
    services.start(&tree) ?? panic("start")
    if phase == "send" {
        receipt :: runtime.send(endpoint, "order", key: "order-restart") ?? panic("send")
        print(receipt_id(receipt))
    } else {
        id :: env.get("JET_SERVICE_AUTH_ID") ?? panic("id")
        retry :: runtime.retry(id) ?? panic("retry")
        print(receipt_kind(retry))
        message :: services.receive(&tree, endpoint) ?? panic("receive")
        print(message)
        runtime.commit(id) ?? panic("commit")
    }
}
"#;

fn compile_restart_binary(source: &str) -> (PathBuf, PathBuf) {
    let serial = RESTART_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("jet_service_restart_{}_{}", std::process::id(), serial));
    fs::create_dir_all(&dir).unwrap();
    let jet_path = dir.join("restart.jet");
    fs::write(&jet_path, source).unwrap();
    let shown = jet_path.to_string_lossy().into_owned();
    let out = jet::compile_with_path(source, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected:\n{}",
            jet::render_diagnostics(&shown, source, &diags)
        )
    });
    let rs = dir.join("restart.rs");
    let bin = dir.join("restart");
    fs::write(&rs, &out.rust).unwrap();
    let mut rustc = Command::new("rustc");
    rustc.args(["--edition", "2021", rs.to_str().unwrap(), "-o", bin.to_str().unwrap()]);
    if let Some(link) = &out.ffi {
        rustc
            .arg("--extern")
            .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        for deps_dir in link.dependency_dirs().filter(|dir| dir.is_dir()) {
            rustc.arg("-L").arg(format!("dependency={}", deps_dir.display()));
        }
    }
    let result = rustc.output().unwrap();
    assert!(
        result.status.success(),
        "rustc rejected generated restart code:\n{}",
        String::from_utf8_lossy(&result.stderr)
    );
    (dir, bin)
}

fn run_restart_process(bin: &Path, store: &Path, phase: &str, id: Option<&str>) -> String {
    let mut command = Command::new(bin);
    command
        .env("JET_SERVICE_AUTH_STORE", store)
        .env("JET_SERVICE_AUTH_PHASE", phase);
    if let Some(id) = id {
        command.env("JET_SERVICE_AUTH_ID", id);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "restart process failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn run_restart_default_process(dir: &Path, store: &Path, phase: &str, id: Option<&str>) -> String {
    let mut command = Command::new(env!("CARGO_BIN_EXE_jet"));
    command
        .args(["run", "restart.jet", "--trace-tiers"])
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .env("JET_SERVICE_AUTH_STORE", store)
        .env("JET_SERVICE_AUTH_PHASE", phase);
    if let Some(id) = id {
        command.env("JET_SERVICE_AUTH_ID", id);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "default restart process failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn service_authority_recovers_pending_delivery_across_process_restart() {
    if !have_rustc() {
        return;
    }
    let (dir, bin) = compile_restart_binary(RESTART_SOURCE);
    let store = dir.join("authority.log");
    let id = run_restart_process(&bin, &store, "send", None);
    let id = id.trim();
    assert!(!id.is_empty(), "send process did not return a receipt id");
    assert_eq!(
        run_restart_process(&bin, &store, "recover", Some(id)),
        "duplicate\norder\n"
    );

    let default_store = dir.join("authority-default.log");
    let id = run_restart_default_process(&dir, &default_store, "send", None);
    let id = id.trim();
    assert!(!id.is_empty(), "default send process did not return a receipt id");
    assert_eq!(
        run_restart_process(&bin, &default_store, "recover", Some(id)),
        "duplicate\norder\n"
    );

    let aot_store = dir.join("authority-aot-to-default.log");
    let id = run_restart_process(&bin, &aot_store, "send", None);
    let id = id.trim();
    assert!(!id.is_empty(), "AOT send process did not return a receipt id");
    assert_eq!(
        run_restart_default_process(&dir, &aot_store, "recover", Some(id)),
        "duplicate\norder\n"
    );
}

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
    restored :: services.restore_snapshot(snapshot) ?? panic("snapshot restore")
    print("snapshot:{restored}")
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
    history :: services.workflow_history(workflow, run_id) ?? panic("workflow history")
    print("workflow:{run_id}:{same_run}:{history}")
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
