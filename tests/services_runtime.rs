//! Focused failure-path checks for the production `core.service` Prelude.

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use tir_support::{
    build_and_run, build_and_run_full, have_rustc, interpreter_run, run_default_multi,
};

static RESTART_SEQ: AtomicU64 = AtomicU64::new(0);

#[test]
fn service_runtime_exports_typed_counters_on_all_tiers() {
    if !have_rustc() {
        return;
    }
    let source_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/features/tooling/service_runtime.jet");
    let source = fs::read_to_string(source_path).unwrap();
    let expected = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples/features/expected/tooling/service_runtime.out"),
    )
    .unwrap();
    let (aot_code, aot_stdout, aot_stderr) =
        build_and_run_full("services_observability", "service_runtime", &source);
    assert_eq!(
        aot_code, 0,
        "AOT observability dogfood failed: {aot_stderr}"
    );
    assert_eq!(
        aot_stdout, expected,
        "AOT observability dogfood diverged from its golden"
    );

    let (jit_code, jit_stdout, jit_stderr) = run_default_multi(
        "services_observability_jit",
        "main.jet",
        &[("main.jet", source.as_str())],
    );
    assert_eq!(
        jit_code, 0,
        "default observability dogfood failed: {jit_stderr}"
    );
    assert_eq!(
        jit_stdout, expected,
        "default observability dogfood diverged from AOT/golden\n{jit_stderr}"
    );

    let (interpreter_code, interpreter_stdout, interpreter_stderr) =
        interpreter_run("services_observability_interpreter", &source);
    assert_eq!(
        interpreter_code, 0,
        "interpreter observability dogfood failed: {interpreter_stderr}"
    );
    assert_eq!(
        interpreter_stdout, expected,
        "interpreter observability dogfood diverged from AOT/golden\n{interpreter_stderr}"
    );
}

const AUTHORITY_SOURCE: &str = r#"
use core.service as services
use core.testing as testing
use core.time as time

fn orders_worker() {}

fn receipt_id(receipt: ^Delivery) Delivery -> {
    return receipt
}

fn receipt_kind(receipt: ^Delivery) String -> {
    state :: receipt.status() ?? panic("status")
    if state == {
        .Pending -> { return "pending" }
        .Accepted -> { return "accepted" }
        .Delivering -> { return "delivering" }
        .Delivered -> { return "delivered" }
        .DeadLettered -> { return "dead" }
        .Cancelled -> { return "cancelled" }
    }
    return "unknown"
}

fn run() {
    temp := testing.temp_dir("service-authority")
    store :: Path.from(temp).join("authority.log").to_string()
    retention :: Duration.seconds(86400) ?? panic("retention")
    runtime := services.runtime(store, retention: retention)
    tree := services.tree("orders")
    endpoint :: tree.worker("orders", orders_worker, capacity: 2) ?? panic("worker")
    tree.group("orders-supervisor", ["orders"]) ?? panic("group")
    tree.start() ?? panic("start")

    first :: runtime.send(endpoint, "order", key: "order-1") ?? panic("first")
    id :: receipt_id(~first)
    receipt :: (~first).receipt() ?? panic("receipt")
    history :: (~first).events() ?? panic("events")
    print("audit:{receipt.show().contains(\"DeliveryReceipt\")}:{history.len()}")
    print("first:accepted")
    recovered := services.runtime(store, retention: retention)
    recovered_duplicate :: recovered.retry(~id) ?? panic("recover")
    print("recovered:{receipt_kind(^recovered_duplicate)}")
    delivered :: tree.receive(endpoint) ?? panic("deliver")
    print("delivered:{delivered}")
    runtime.commit(first) ?? panic("commit")
    print("committed:ok")
    tree.fail_worker(endpoint) ?? panic("restart")
    print("restarted:{tree.restarts(endpoint) ?? panic("restarts")}")

    duplicate :: recovered.send(endpoint, "order", key: "order-1") ?? panic("duplicate")
    print("duplicate:{receipt_kind(^duplicate)}")

    retained :: runtime.retain(~id) ?? panic("retain")
    print("retain:{receipt_kind(^retained)}")
    retry :: recovered.retry(~id) ?? panic("retry")
    print("retry:{receipt_kind(^retry)}")
    redelivered :: tree.receive(endpoint) ?? panic("redeliver")
    print("redelivered:{redelivered}")
    runtime.commit(~id) ?? panic("redelivery commit")
    dead :: runtime.dead_letter(id) ?? panic("dead")
    print("dead:{receipt_kind(^dead)}")
    tree.stop() ?? panic("stop")
    stopped := recovered.send(endpoint, "new-order", key: "order-2")
    if stopped == {
        .Ok(_) -> { print("stopped:accepted") }
        .Err(_) -> { print("stopped:rejected") }
    }
    tree.start() ?? panic("restart")
    replay :: recovered.send(endpoint, "order", key: "order-1") ?? panic("replay")
    print("replay:{receipt_kind(^replay)}")
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
        "audit:true:1\nfirst:accepted\nrecovered:accepted\ndelivered:order\ncommitted:ok\nrestarted:1\nduplicate:delivered\nretain:accepted\nretry:accepted\nredelivered:order\ndead:dead\nstopped:rejected\nreplay:dead\n"
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
        "first:accepted\nrecovered:accepted\ndelivered:order\ncommitted:ok\nrestarted:1\nduplicate:delivered\nretain:accepted\nretry:accepted\nredelivered:order\ndead:dead\nstopped:rejected\nreplay:dead\n"
    );
}

#[test]
fn service_authority_receipts_match_interpreter() {
    let (code, stdout, stderr) =
        interpreter_run("services_authority_interpreter", AUTHORITY_SOURCE);
    assert_eq!(
        code, 0,
        "interpreter service authority run failed: {stderr}"
    );
    assert_eq!(
        stdout,
        "first:accepted\nrecovered:accepted\ndelivered:order\ncommitted:ok\nrestarted:1\nduplicate:delivered\nretain:accepted\nretry:accepted\nredelivered:order\ndead:dead\nstopped:rejected\nreplay:dead\n"
    );
}

const DURABLE_AUDIT_SOURCE: &str = r#"
use core.service as service
use core.testing as testing
use core.time as time

fn worker() {}

fn state_name(delivery: ^Delivery) String -> {
    state :: delivery.status() ?? panic("status")
    if state == {
        .Pending -> { return "pending" }
        .Accepted -> { return "accepted" }
        .Delivering -> { return "delivering" }
        .Delivered -> { return "delivered" }
        .DeadLettered -> { return "dead" }
        .Cancelled -> { return "cancelled" }
    }
    return "unknown"
}

fn run() {
    temp := testing.temp_dir("durable-audit")
    store :: Path.from(temp).join("authority.log").to_string()
    retention :: Duration.seconds(86400) ?? panic("retention")
    runtime := service.runtime(store, retention: retention)
    tree := service.tree("durable-audit")
    endpoint :: tree.worker("worker", worker, capacity: 2) ?? panic("worker")
    tree.start() ?? panic("start")

    accepted :: runtime.send(endpoint, "accepted", key: "accepted") ?? panic("accepted")
    receipt :: (~accepted).receipt() ?? panic("receipt")
    events :: (~accepted).events() ?? panic("events")
    print("audit:{receipt.show().contains(\"DeliveryReceipt\")}:{events.len()}")
    accepted.status() ?? panic("accepted status")

    cancelled_handle :: runtime.send(endpoint, "cancelled", key: "cancelled") ?? panic("cancelled")
    cancelled :: cancelled_handle.cancel() ?? panic("cancel")
    print("cancel:{state_name(^cancelled)}")

    dead_handle :: runtime.send(endpoint, "dead", key: "dead") ?? panic("dead")
    dead :: runtime.dead_letter(dead_handle) ?? panic("dead letter")
    print("dead:{state_name(^dead)}")

    short_runtime := service.runtime(store, retention: Duration.milliseconds(1) ?? panic("short retention"))
    expiring :: short_runtime.send(endpoint, "expiring", key: "expiring") ?? panic("expiring")
    time.sleep(Duration.milliseconds(5) ?? panic("sleep"))
    print("expired:{state_name(^expiring)}")
}
"#;

const DURABLE_AUDIT_OUTPUT: &str = "audit:true:1\ncancel:cancelled\ndead:dead\nexpired:dead\n";

#[test]
fn durable_lifecycle_audit_matches_aot_default_and_interpreter() {
    if !have_rustc() {
        return;
    }
    let (aot_code, aot_stdout) = build_and_run("services_durable_audit", DURABLE_AUDIT_SOURCE);
    assert_eq!(aot_code, 0);
    assert_eq!(aot_stdout, DURABLE_AUDIT_OUTPUT);

    let (jit_code, jit_stdout, jit_stderr) = run_default_multi(
        "services_durable_audit_jit",
        "main.jet",
        &[("main.jet", DURABLE_AUDIT_SOURCE)],
    );
    assert_eq!(jit_code, 0, "default durable audit failed: {jit_stderr}");
    assert_eq!(jit_stdout, DURABLE_AUDIT_OUTPUT);

    let (interpreter_code, interpreter_stdout, interpreter_stderr) =
        interpreter_run("services_durable_audit_interpreter", DURABLE_AUDIT_SOURCE);
    assert_eq!(
        interpreter_code, 0,
        "interpreter durable audit failed: {interpreter_stderr}"
    );
    assert_eq!(interpreter_stdout, DURABLE_AUDIT_OUTPUT);
}

const RESTART_SOURCE: &str = r#"
use core.sys as env
use core.service as services
use core.time as time

fn orders_worker() {}

fn receipt_id(receipt: ^Delivery) Delivery -> {
    return receipt
}

fn receipt_kind(receipt: ^Delivery) String -> {
    state :: receipt.status() ?? panic("status")
    if state == {
        .Pending -> { return "pending" }
        .Accepted -> { return "accepted" }
        .Delivering -> { return "delivering" }
        .Delivered -> { return "delivered" }
        .DeadLettered -> { return "dead" }
        .Cancelled -> { return "cancelled" }
    }
    return "unknown"
}

fn run() {
    store :: env.get("JET_SERVICE_AUTH_STORE") ?? panic("store")
    phase :: env.get("JET_SERVICE_AUTH_PHASE") ?? panic("phase")
    retention :: Duration.seconds(86400) ?? panic("retention")
    runtime := services.runtime(store, retention: retention)
    tree := services.tree("restart-orders")
    endpoint :: tree.worker("orders", orders_worker, capacity: 2) ?? panic("worker")
    tree.group("orders-supervisor", ["orders"]) ?? panic("group")
    tree.start() ?? panic("start")
    if phase == "send" {
        receipt :: runtime.send(endpoint, "order", key: "order-restart") ?? panic("send")
        receipt.status() ?? panic("send status")
        print("accepted")
    } else {
        tree.fail_worker(endpoint) ?? panic("restart")
        print("restarted:{tree.restarts(endpoint) ?? panic("restarts")}")
        receipt :: runtime.send(endpoint, "order", key: "order-restart") ?? panic("recover")
        retry :: runtime.retry(receipt) ?? panic("retry")
        retry_copy :: ~retry
        print(receipt_kind(^retry_copy))
        message :: tree.receive(endpoint) ?? panic("receive")
        print(message)
        runtime.commit(retry) ?? panic("commit")
    }
}
"#;

const CANCELLED_RESTART_SOURCE: &str = r#"
use core.sys as env
use core.service as services
use core.time as time

fn worker() {}

fn run() {
    store :: env.get("JET_SERVICE_AUTH_STORE") ?? panic("store")
    phase :: env.get("JET_SERVICE_AUTH_PHASE") ?? panic("phase")
    retention :: Duration.seconds(86400) ?? panic("retention")
    runtime := services.runtime(store, retention: retention)
    tree := services.tree("cancel-restart")
    endpoint :: tree.worker("worker", worker, capacity: 1) ?? panic("worker")
    tree.start() ?? panic("start")
    if phase == "send" {
        accepted :: runtime.send(endpoint, "cancelled", key: "cancelled") ?? panic("send")
        cancelled :: accepted.cancel() ?? panic("cancel")
        state :: cancelled.status() ?? panic("cancel status")
        print("send:{state.show()}")
    } else {
        recovered :: runtime.send(endpoint, "cancelled", key: "cancelled") ?? panic("recover")
        state :: recovered.status() ?? panic("recover status")
        history :: recovered.events() ?? panic("recover events")
        print("recover:{state.show()}:{history.len()}")
        tree.handoff_generation() ?? panic("handoff")
        print("receipt:{tree.upgrade_receipt() ?? panic("receipt")}")
    }
}
"#;

fn compile_restart_binary(source: &str) -> (PathBuf, PathBuf) {
    let serial = RESTART_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "jet_service_restart_{}_{}",
        std::process::id(),
        serial
    ));
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
    rustc.args([
        "--edition",
        "2021",
        rs.to_str().unwrap(),
        "-o",
        bin.to_str().unwrap(),
    ]);
    if let Some(link) = &out.ffi {
        rustc
            .arg("--extern")
            .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        for deps_dir in link.dependency_dirs().filter(|dir| dir.is_dir()) {
            rustc
                .arg("-L")
                .arg(format!("dependency={}", deps_dir.display()));
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
        "restarted:1\naccepted\norder\n"
    );

    let default_store = dir.join("authority-default.log");
    let id = run_restart_default_process(&dir, &default_store, "send", None);
    let id = id.trim();
    assert!(
        !id.is_empty(),
        "default send process did not return a receipt id"
    );
    assert_eq!(
        run_restart_process(&bin, &default_store, "recover", Some(id)),
        "restarted:1\naccepted\norder\n"
    );

    let aot_store = dir.join("authority-aot-to-default.log");
    let id = run_restart_process(&bin, &aot_store, "send", None);
    let id = id.trim();
    assert!(
        !id.is_empty(),
        "AOT send process did not return a receipt id"
    );
    assert_eq!(
        run_restart_default_process(&dir, &aot_store, "recover", Some(id)),
        "restarted:1\naccepted\norder\n"
    );

    // A validly framed but altered immutable acceptance fact must not become a
    // restart alias. The receipt id is the existing length-framed SHA-256
    // identity for the logical store, route, message, and key; the acceptance
    // HMAC also covers the deadline and all persisted route facts.
    let corrupt_store = dir.join("authority-corrupt.log");
    let corrupt_id = run_restart_process(&bin, &corrupt_store, "send", None);
    let mut corrupt = fs::read(&corrupt_store).unwrap();
    let pipes: Vec<usize> = corrupt
        .iter()
        .enumerate()
        .filter_map(|(index, byte)| (*byte == b'|').then_some(index))
        .collect();
    assert!(
        pipes.len() >= 10,
        "receipt record did not contain its framed fields"
    );
    let expires_start = pipes[8] + 1;
    let expires_end = pipes[9];
    let expires_colon = corrupt[expires_start..expires_end]
        .iter()
        .position(|byte| *byte == b':')
        .expect("receipt expiry field was not length framed");
    let expiry_hex = expires_start + expires_colon + 1;
    corrupt[expiry_hex] = if corrupt[expiry_hex] == b'0' {
        b'1'
    } else {
        b'0'
    };
    fs::write(&corrupt_store, corrupt).unwrap();
    assert!(
        !restart_status(&bin, &corrupt_store, "recover", corrupt_id.trim()).success(),
        "a receipt with altered immutable facts was accepted"
    );

    // Removing only the final newline leaves a complete framed record without
    // its commit terminator. Recovery must reject that truncated history.
    let truncated_store = dir.join("authority-truncated.log");
    let truncated_id = run_restart_process(&bin, &truncated_store, "send", None);
    let mut truncated = fs::read(&truncated_store).unwrap();
    assert_eq!(truncated.pop(), Some(b'\n'));
    fs::write(&truncated_store, truncated).unwrap();
    assert!(
        !restart_status(
            &bin,
            &truncated_store,
            "recover",
            truncated_id.trim()
        )
        .success(),
        "a truncated authority log was accepted"
    );
}

#[test]
fn service_authority_replays_cancelled_delivery_without_requeue_or_pin() {
    if !have_rustc() {
        return;
    }
    let (dir, bin) = compile_restart_binary(CANCELLED_RESTART_SOURCE);
    let store = dir.join("cancelled.log");
    assert_eq!(
        run_restart_process(&bin, &store, "send", None),
        "send:Cancelled\n"
    );
    assert_eq!(
        run_restart_process(&bin, &store, "recover", None),
        "recover:Cancelled:2\nreceipt:ServiceUpgradeReceipt(from=1, to=2, migration=none, rollback_available=false, pinned=)\n"
    );
}

const ROLLOUT_RESTART_SOURCE: &str = r#"
use core.sys as env
use core.service as services

fn rollout_worker() {}

fn run() {
    store_path :: env.get("JET_SERVICE_AUTH_STORE") ?? panic("store")
    phase :: env.get("JET_SERVICE_AUTH_PHASE") ?? panic("phase")
    tree := services.tree("rollout-restart")
    store :: services.state_store(store_path) ?? panic("state store")
    tree.set_state_event_log(store, "rollout-events", 1, "reversible") ?? panic("state")
    endpoint :: tree.worker("api", rollout_worker, capacity: 1) ?? panic("worker")
    tree.start() ?? panic("start")
    if phase == "write" {
        tree.directory_register("api", endpoint) ?? panic("directory")
        tree.drain_worker(endpoint) ?? panic("drain")
        print("handoff:{tree.handoff_generation() ?? panic("handoff")}")
    } else {
        current :: tree.directory_resolve("api") ?? panic("resolve")
        print("generation:{tree.directory_generation()}")
        print("current:{current.show()}")
        print("receipt:{tree.upgrade_receipt() ?? panic("receipt")}")
        stale :: tree.send(endpoint, "stale")
        if stale == {
            .Ok(_) -> { print("stale:accepted") }
            .Err(_) -> { print("stale:rejected") }
        }
        current.send("after") ?? panic("after send")
        print("after:{tree.receive(current) ?? panic("after receive")}")
    }
    tree.stop() ?? panic("stop")
}
"#;

const ROLLOUT_RESTART_OUTPUT: &str =
    "generation:2\ncurrent:Endpoint(rollout-restart/api@g2)\nreceipt:ServiceUpgradeReceipt(from=1, to=2, migration=reversible, rollback_available=true, pinned=)\nstale:rejected\nafter:after\n";

#[test]
fn rollout_identity_and_receipt_survive_process_restart() {
    if !have_rustc() {
        return;
    }
    let (dir, bin) = compile_restart_binary(ROLLOUT_RESTART_SOURCE);
    let store = dir.join("rollout.log");
    assert_eq!(
        run_restart_process(&bin, &store, "write", None),
        "handoff:2\n"
    );
    assert_eq!(
        run_restart_process(&bin, &store, "recover", None),
        ROLLOUT_RESTART_OUTPUT
    );

    let default_store = dir.join("rollout-default.log");
    assert_eq!(
        run_restart_default_process(&dir, &default_store, "write", None),
        "handoff:2\n"
    );
    assert_eq!(
        run_restart_process(&bin, &default_store, "recover", None),
        ROLLOUT_RESTART_OUTPUT
    );

    let aot_to_default_store = dir.join("rollout-aot-to-default.log");
    assert_eq!(
        run_restart_process(&bin, &aot_to_default_store, "write", None),
        "handoff:2\n"
    );
    assert_eq!(
        run_restart_default_process(&dir, &aot_to_default_store, "recover", None),
        ROLLOUT_RESTART_OUTPUT
    );
}

#[test]
fn forged_rollout_generation_is_rejected_on_restart() {
    if !have_rustc() {
        return;
    }
    let (dir, bin) = compile_restart_binary(ROLLOUT_RESTART_SOURCE);
    let store = dir.join("rollout-forged.log");
    assert_eq!(
        run_restart_process(&bin, &store, "write", None),
        "handoff:2\n"
    );

    let rollout = PathBuf::from(format!("{}.rollout", store.display()));
    let journal = fs::read_to_string(&rollout).unwrap();
    let forged = journal
        .replace("generation:2\n", "generation:99\n")
        .replace("upgrade_to:2\n", "upgrade_to:99\n")
        .replace("worker_generation:2\n", "worker_generation:99\n")
        .replace("directory_generation:2\n", "directory_generation:99\n");
    assert_ne!(journal, forged, "rollout journal fixture was not changed");
    fs::write(&rollout, forged).unwrap();

    assert!(
        !restart_status(&bin, &store, "recover", "").success(),
        "a coherently forged rollout generation was accepted"
    );
}

const STATE_RESTART_SOURCE: &str = r#"
use core.sys as env
use core.service as services

fn state_worker() {}

fn run() {
    store_path :: env.get("JET_SERVICE_AUTH_STORE") ?? panic("store")
    phase :: env.get("JET_SERVICE_AUTH_PHASE") ?? panic("phase")
    adapter :: env.get("JET_SERVICE_AUTH_ID") ?? panic("adapter")
    tree := services.tree("state")
    store :: services.state_store(store_path) ?? panic("state store")
    if adapter == "snapshot" {
        tree.set_state_snapshot(store, "app-state", 1, "reversible") ?? panic("snapshot state")
    } else {
        if adapter == "schema-drift" {
            tree.set_state_event_log(store, "other-events", 1, "reversible") ?? panic("event state")
        } else {
            if adapter == "version-drift" {
                tree.set_state_event_log(store, "app-state", 2, "reversible") ?? panic("event state")
            } else {
                tree.set_state_event_log(store, "app-state", 1, "reversible") ?? panic("event state")
            }
        }
    }
    tree.worker("worker", state_worker, capacity: 1) ?? panic("worker")
    tree.start() ?? panic("start")
    if adapter == "snapshot" {
        if phase == "write" {
            tree.commit_snapshot("state-v1") ?? panic("commit")
            print("wrote:{tree.restore_snapshot() ?? panic("restore")}")
        } else {
            print("restored:{tree.restore_snapshot() ?? panic("restore")}")
            tree.commit_snapshot("state-v2") ?? panic("recommit")
            print("recommitted:{tree.restore_snapshot() ?? panic("restore")}")
        }
    } else {
        if phase == "write" {
            tree.append_event("first") ?? panic("first")
            tree.append_event("second") ?? panic("second")
            print("wrote:{tree.event_count()}")
        } else {
            print("count:{tree.event_count()}")
            print("replay:{tree.replay_events()}")
            tree.append_event("third") ?? panic("third")
            print("appended:{tree.replay_events()}")
        }
    }
}
"#;

/// A durable state adapter that cannot be read by a later process is not
/// durable.  Restart is the only check that separates a store from a cache.
#[test]
fn state_adapters_survive_process_restart() {
    if !have_rustc() {
        return;
    }
    let (dir, bin) = compile_restart_binary(STATE_RESTART_SOURCE);

    let events = dir.join("events.log");
    assert_eq!(
        run_restart_process(&bin, &events, "write", Some("event-log")),
        "wrote:2\n"
    );
    assert_eq!(
        run_restart_default_process(&dir, &events, "read", Some("event-log")),
        "count:2\nreplay:first|second\nappended:first|second|third\n"
    );

    let events_reversed = dir.join("events-default-to-aot.log");
    assert_eq!(
        run_restart_default_process(&dir, &events_reversed, "write", Some("event-log")),
        "wrote:2\n"
    );
    assert_eq!(
        run_restart_process(&bin, &events_reversed, "read", Some("event-log")),
        "count:2\nreplay:first|second\nappended:first|second|third\n"
    );

    let snapshot = dir.join("snapshot.log");
    assert_eq!(
        run_restart_process(&bin, &snapshot, "write", Some("snapshot")),
        "wrote:state-v1\n"
    );
    assert_eq!(
        run_restart_default_process(&dir, &snapshot, "read", Some("snapshot")),
        "restored:state-v1\nrecommitted:state-v2\n"
    );

    let snapshot_reversed = dir.join("snapshot-default-to-aot.log");
    assert_eq!(
        run_restart_default_process(&dir, &snapshot_reversed, "write", Some("snapshot")),
        "wrote:state-v1\n"
    );
    assert_eq!(
        run_restart_process(&bin, &snapshot_reversed, "read", Some("snapshot")),
        "restored:state-v1\nrecommitted:state-v2\n"
    );

    // A store written under one schema or version must not open under another,
    // and a tail lost to a crash mid-append must fail closed rather than read
    // as a shorter history.
    let drift = dir.join("events-drift.log");
    assert_eq!(
        run_restart_process(&bin, &drift, "write", Some("event-log")),
        "wrote:2\n"
    );
    for adapter in ["schema-drift", "version-drift"] {
        assert!(
            !restart_status(&bin, &drift, "read", adapter).success(),
            "a store opened under a mismatched {adapter}"
        );
    }

    let torn = dir.join("events-torn.log");
    assert_eq!(
        run_restart_process(&bin, &torn, "write", Some("event-log")),
        "wrote:2\n"
    );
    let bytes = fs::read(&torn).unwrap();
    fs::write(&torn, &bytes[..bytes.len() - 3]).unwrap();
    assert!(
        !restart_status(&bin, &torn, "read", "event-log").success(),
        "a truncated state store was accepted"
    );
}

fn restart_status(bin: &Path, store: &Path, phase: &str, id: &str) -> std::process::ExitStatus {
    Command::new(bin)
        .env("JET_SERVICE_AUTH_STORE", store)
        .env("JET_SERVICE_AUTH_PHASE", phase)
        .env("JET_SERVICE_AUTH_ID", id)
        .output()
        .unwrap()
        .status
}

const WORKFLOW_RESTART_SOURCE: &str = r#"
use core.sys as env
use core.service as services

fn workflow_worker() {}

fn run() {
    store_path :: env.get("JET_SERVICE_AUTH_STORE") ?? panic("store")
    phase :: env.get("JET_SERVICE_AUTH_PHASE") ?? panic("phase")
    tree := services.tree("workflows")
    store :: services.state_store(store_path) ?? panic("state store")
    tree.set_state_event_log(store, "wf-events", 1, "reversible") ?? panic("state")
    tree.worker("worker", workflow_worker, capacity: 1) ?? panic("worker")
    tree.start() ?? panic("start")
    if phase == "write" {
        run_id :: tree.workflow_start("checkout", 1) ?? panic("workflow start")
        tree.workflow_step(run_id, "charge") ?? panic("charge step")
        tree.workflow_step(run_id, "ship:express") ?? panic("ship step")
        print("run:{run_id}")
    } else {
        history :: tree.workflow_history(1) ?? panic("history")
        print("history:{history}")
        replay :: tree.workflow_start("checkout", 1) ?? panic("replay")
        print("replay:{replay}")
        versioned :: tree.workflow_start("checkout", 2)
        if versioned == {
            .Ok(_) -> { print("version:accepted") }
            .Err(_) -> { print("version:rejected") }
        }
        if phase == "extend" {
            refund :: tree.workflow_start("refund", 1) ?? panic("refund")
            tree.workflow_step(refund, "credit") ?? panic("credit step")
            print("refund:{refund}")
        }
        if phase == "final" {
            print("refund_history:{tree.workflow_history(2) ?? panic("refund history")}")
        }
    }
}
"#;

const WORKFLOW_RESTART_HISTORY: &str =
    "history:start@v1|step:charge|step:ship:express\nreplay:1\nversion:rejected\n";

const WORKFLOW_REPLAY_SOURCE: &str = r#"
use core.sys as env
use core.service as services

fn replay_worker() {}

fn run() {
    store_path :: env.get("JET_SERVICE_AUTH_STORE") ?? panic("store")
    phase :: env.get("JET_SERVICE_AUTH_PHASE") ?? panic("phase")
    tree := services.tree("workflow-replay")
    store :: services.state_store(store_path) ?? panic("state store")
    tree.set_state_event_log(store, "workflow-events", 1, "reversible") ?? panic("state")
    tree.worker("worker", replay_worker, capacity: 1) ?? panic("worker")
    tree.start() ?? panic("start")
    if phase == "mismatch" {
        run_id :: tree.workflow_start("checkout", 1) ?? panic("workflow")
        tree.workflow_step(run_id, "ship") ?? panic("replay mismatch accepted")
    } else if phase == "activity-mismatch" {
        run_id :: tree.workflow_start("checkout", 1) ?? panic("workflow")
        tree.workflow_activity(run_id, "charge", "charge-1", 2) ?? panic("activity mismatch accepted")
    } else if phase == "completion-mismatch" {
        run_id :: tree.workflow_start("checkout", 1) ?? panic("workflow")
        tree.workflow_activity_complete(run_id, "charge-1", TaskOutcome.Finished) ?? panic("completion mismatch accepted")
    } else if phase == "write" {
        run_id :: tree.workflow_start("checkout", 1) ?? panic("workflow")
        tree.workflow_step(run_id, "charge") ?? panic("charge")
        tree.workflow_step(run_id, "ship") ?? panic("ship")
        tree.workflow_activity(run_id, "charge", "charge-1", 2) ?? panic("activity")
        tree.workflow_activity_retry(run_id, "charge-1", TaskOutcome.Panicked("timeout")) ?? panic("retry")
        tree.workflow_activity_complete(run_id, "charge-1", TaskOutcome.Finished) ?? panic("complete")
        print("written:{tree.workflow_history(run_id) ?? panic("history")}")
    } else {
        run_id :: tree.workflow_start("checkout", 1) ?? panic("replay")
        tree.workflow_step(run_id, "charge") ?? panic("charge replay")
        tree.workflow_step(run_id, "ship") ?? panic("ship replay")
        tree.workflow_activity(run_id, "charge", "charge-1", 2) ?? panic("activity replay")
        tree.workflow_activity_retry(run_id, "charge-1", TaskOutcome.Panicked("timeout")) ?? panic("retry replay")
        tree.workflow_activity_complete(run_id, "charge-1", TaskOutcome.Finished) ?? panic("complete replay")
        print("replayed:{tree.workflow_history(run_id) ?? panic("history")}")
    }
}
"#;

const WORKFLOW_REPLAY_HISTORY: &str = "start@v1|step:charge|step:ship|activity:charge:charge-1@1/2|activity-retry:charge-1@2/2:Panicked(timeout)|activity-done:charge-1|activity-result:Finished\n";

/// A versioned workflow history is only durable if a later process reads back
/// the same runs, steps, and version conflicts the writer recorded.
#[test]
fn workflow_history_survives_process_restart() {
    if !have_rustc() {
        return;
    }
    let (dir, bin) = compile_restart_binary(WORKFLOW_RESTART_SOURCE);

    let store = dir.join("workflow.log");
    assert_eq!(run_restart_process(&bin, &store, "write", None), "run:1\n");
    assert_eq!(
        run_restart_process(&bin, &store, "read", None),
        WORKFLOW_RESTART_HISTORY
    );

    let default_store = dir.join("workflow-default.log");
    assert_eq!(
        run_restart_default_process(&dir, &default_store, "write", None),
        "run:1\n"
    );
    assert_eq!(
        run_restart_default_process(&dir, &default_store, "read", None),
        WORKFLOW_RESTART_HISTORY
    );

    let crossed = dir.join("workflow-aot-to-default.log");
    assert_eq!(
        run_restart_process(&bin, &crossed, "write", None),
        "run:1\n"
    );
    assert_eq!(
        run_restart_default_process(&dir, &crossed, "read", None),
        WORKFLOW_RESTART_HISTORY
    );

    let reversed = dir.join("workflow-default-to-aot.log");
    assert_eq!(
        run_restart_default_process(&dir, &reversed, "write", None),
        "run:1\n"
    );
    assert_eq!(
        run_restart_process(&bin, &reversed, "read", None),
        WORKFLOW_RESTART_HISTORY
    );

    // A truncated tail is the shape a crash mid-append leaves behind.  The
    // next process must say so, not silently drop the run it cannot read.
    let corrupt = dir.join("workflow-corrupt.log");
    assert_eq!(
        run_restart_process(&bin, &corrupt, "write", None),
        "run:1\n"
    );
    let log = corrupt.with_extension("log.workflows");
    let bytes = fs::read(&log).unwrap();
    fs::write(&log, &bytes[..bytes.len() - 4]).unwrap();
    let mut failed = Command::new(&bin);
    failed
        .env("JET_SERVICE_AUTH_STORE", &corrupt)
        .env("JET_SERVICE_AUTH_PHASE", "read");
    let output = failed.output().unwrap();
    assert!(
        !output.status.success(),
        "a truncated workflow log was accepted: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    // A replayed history has to be writable, not just readable: the third
    // process only sees run 2 if the second process numbered and recorded it
    // from replayed state.
    let extended = dir.join("workflow-extended.log");
    assert_eq!(
        run_restart_process(&bin, &extended, "write", None),
        "run:1\n"
    );
    assert_eq!(
        run_restart_process(&bin, &extended, "extend", None),
        format!("{WORKFLOW_RESTART_HISTORY}refund:2\n")
    );
    assert_eq!(
        run_restart_default_process(&dir, &extended, "final", None),
        format!("{WORKFLOW_RESTART_HISTORY}refund_history:start@v1|step:credit\n")
    );
}

#[test]
fn workflow_body_replay_reuses_recorded_steps() {
    if !have_rustc() {
        return;
    }
    let (dir, bin) = compile_restart_binary(WORKFLOW_REPLAY_SOURCE);
    let store = dir.join("workflow-replay.log");
    assert_eq!(
        run_restart_process(&bin, &store, "write", None),
        format!("written:{WORKFLOW_REPLAY_HISTORY}")
    );
    assert_eq!(
        run_restart_process(&bin, &store, "read", None),
        format!("replayed:{WORKFLOW_REPLAY_HISTORY}")
    );

    assert!(!restart_status(&bin, &store, "mismatch", "").success());
    assert!(!restart_status(&bin, &store, "activity-mismatch", "").success());
    assert!(!restart_status(&bin, &store, "completion-mismatch", "").success());

    let default_store = dir.join("workflow-replay-default.log");
    assert_eq!(
        run_restart_default_process(&dir, &default_store, "write", None),
        format!("written:{WORKFLOW_REPLAY_HISTORY}")
    );
    assert_eq!(
        run_restart_default_process(&dir, &default_store, "read", None),
        format!("replayed:{WORKFLOW_REPLAY_HISTORY}")
    );

    let crossed = dir.join("workflow-replay-crossed.log");
    assert_eq!(
        run_restart_process(&bin, &crossed, "write", None),
        format!("written:{WORKFLOW_REPLAY_HISTORY}")
    );
    assert_eq!(
        run_restart_default_process(&dir, &crossed, "read", None),
        format!("replayed:{WORKFLOW_REPLAY_HISTORY}")
    );
}

const WORKFLOW_OUTCOME_SOURCE: &str = r#"
use core.sys as env
use core.service as services

fn outcome_worker() {}

fn run() {
    store_path :: env.get("JET_SERVICE_AUTH_STORE") ?? panic("store")
    phase :: env.get("JET_SERVICE_AUTH_PHASE") ?? panic("phase")
    tree := services.tree("workflow-outcome")
    store :: services.state_store(store_path) ?? panic("state store")
    tree.set_state_event_log(store, "workflow-events", 1, "reversible") ?? panic("state")
    tree.worker("activities", outcome_worker, capacity: 1) ?? panic("worker")
    tree.start() ?? panic("start")
    if phase == "write" {
        run_id :: tree.workflow_start("checkout", 1) ?? panic("workflow")
        scheduled :: tree.workflow_activity(run_id, "charge", "charge-1", 2) ?? panic("schedule")
        print("scheduled:{scheduled}")
        paused :: tree.workflow_activity_retry(run_id, "charge-1", TaskOutcome.Panicked("timeout")) ?? panic("retry")
        print("retry:{paused}")
        result :: tree.workflow_activity_complete(run_id, "charge-1", TaskOutcome.Finished) ?? panic("complete")
        print("completed:{result}")
        print("run:{tree.workflow_outcome(run_id) ?? panic("outcome")}")
        print("observe:{tree.observe()}")
        print("history:{tree.workflow_history(run_id) ?? panic("history")}")
    } else {
        result :: tree.workflow_outcome(1) ?? panic("replay outcome")
        print("replay:{result}")
        print("observe:{tree.observe()}")
        same :: tree.workflow_activity_complete(1, "charge-1", TaskOutcome.Finished) ?? panic("idempotent complete")
        print("same:{same}")
        versioned :: tree.workflow_start("checkout", 2) ?? panic("terminal version")
        print("version_after_terminal:{versioned}")
        print("version_history:{tree.workflow_history(versioned) ?? panic("version history")}")
        print("history:{tree.workflow_history(1) ?? panic("history")}")
    }
}
"#;

const WORKFLOW_OUTCOME_HISTORY: &str =
    "start@v1|activity:charge:charge-1@1/2|activity-retry:charge-1@2/2:Panicked(timeout)|activity-done:charge-1|activity-result:Finished\n";

const WORKFLOW_WAIT_SOURCE: &str = r#"
use core.service as service
use core.testing as testing

fn worker() {}

fn run() {
    temp := testing.temp_dir("service-workflow-wait")
    path :: Path.from(temp).join("workflow.log").to_string()
    tree := service.tree("checkout")
    store :: service.state_store(path) ?? panic("state store")
    tree.set_state_event_log(store, "workflow-events", 1, "reversible") ?? panic("state")
    tree.worker("activities", worker, capacity: 1) ?? panic("worker")
    tree.start() ?? panic("start")

    w :: tree.workflow_start("checkout", 1) ?? panic("workflow")
    duration :: Duration.milliseconds(0) ?? panic("duration")
    w.sleep(duration) ?? panic("sleep")
    activity :: w.activity("charge", "charge-1") ?? panic("activity")
    w.all([activity]) ?? panic("all")
    print("history:{tree.workflow_history(w) ?? panic("history")}")

    replay :: tree.workflow_start("checkout", 1) ?? panic("replay")
    replay.sleep(duration) ?? panic("replay sleep")
    replay_activity :: replay.activity("charge", "charge-1") ?? panic("replay activity")
    replay.all([replay_activity]) ?? panic("replay all")
    print("replay:{tree.workflow_history(replay) ?? panic("replay history")}")
}
"#;

const WORKFLOW_WAIT_HISTORY: &str = "start@v1|sleep:0|activity:charge:charge-1|all:charge-1\n";

/// The typed wait methods publish their effects through the ordinary sema
/// effect graph. No workflow source-text marker is needed for this check.
#[test]
fn workflow_wait_methods_publish_typed_effects() {
    let root = common::unique_tmp("jet_service_workflow_wait_effects");
    fs::create_dir_all(&root).unwrap();
    let entry = root.join("main.jet");
    fs::write(
        &entry,
        r#"
fn time_wait(workflow: ServiceWorkflow, duration: Duration) -[Time]> {
    workflow.sleep(duration) ?? return
}

fn activity_wait(workflow: ServiceWorkflow) -[IO]> {
    workflow.activity("charge", "charge-1") ?? return
    workflow.all(["charge-1"]) ?? return
}

fn run() {}
"#,
    )
    .unwrap();
    let (diagnostics, _, facts) =
        jet::Driver::check_file_with_effect_facts(entry.to_str().unwrap(), None, false);
    assert!(
        !diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error),
        "typed workflow effects should satisfy their declared rows: {diagnostics:#?}"
    );
    assert!(facts.summaries["time_wait"].direct.contains("Time"));
    assert!(facts.summaries["activity_wait"].direct.contains("IO"));
}

/// The workflow handle records each wait decision once and reuses it on a
/// second body pass instead of sleeping or redelivering the activity.
#[test]
fn workflow_wait_methods_replay_their_history() {
    if !have_rustc() {
        return;
    }
    let (code, stdout) = build_and_run("services_workflow_wait", WORKFLOW_WAIT_SOURCE);
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        format!("history:{WORKFLOW_WAIT_HISTORY}replay:{WORKFLOW_WAIT_HISTORY}")
    );

    let (jit_code, jit_stdout, jit_stderr) = run_default_multi(
        "services_workflow_wait_jit",
        "main.jet",
        &[("main.jet", WORKFLOW_WAIT_SOURCE)],
    );
    assert_eq!(
        jit_code, 0,
        "default workflow wait run failed: {jit_stderr}"
    );
    assert_eq!(jit_stdout, stdout, "default workflow wait replay diverged");

    let (interpreter_code, interpreter_stdout, interpreter_stderr) =
        interpreter_run("services_workflow_wait_interpreter", WORKFLOW_WAIT_SOURCE);
    assert_eq!(
        interpreter_code, 0,
        "interpreter workflow wait run failed: {interpreter_stderr}"
    );
    assert_eq!(
        interpreter_stdout, stdout,
        "interpreter workflow wait replay diverged"
    );
}

const WORKFLOW_SLEEP_CANCEL_SOURCE: &str = r#"
use core.service as service
use core.testing as testing

fn worker() {}

fn sleeping_workflow(store_path: String, ready: Sender<Int>) {
    tree := service.tree("checkout")
    store :: service.state_store(store_path) ?? panic("state store")
    tree.set_state_event_log(store, "workflow-events", 1, "reversible") ?? panic("state")
    tree.worker("activities", worker, capacity: 1) ?? panic("worker")
    tree.start() ?? panic("start")
    workflow :: tree.workflow_start("checkout", 1) ?? panic("workflow")
    ready.send(1)
    workflow.sleep(Duration.seconds(10) ?? panic("duration")) ?? panic("sleep completed")
}

fn run() {
    temp :: testing.temp_dir("workflow-sleep-cancel")
    store_path :: Path.from(temp).join("workflow.log").to_string()
    (ready, started) :: channel<Int>()
    task_handle :: task sleeping_workflow(~store_path, ready)
    started.receive() ?? panic("workflow did not start")
    task_handle.cancel()
    result :: task_handle.join()
    if result == {
        .Err(_) -> print("cancelled")
        .Ok(_) -> print("completed")
    }

    tree := service.tree("checkout")
    store :: service.state_store(store_path) ?? panic("state store")
    tree.set_state_event_log(store, "workflow-events", 1, "reversible") ?? panic("state")
    tree.worker("activities", worker, capacity: 1) ?? panic("worker")
    tree.start() ?? panic("restart")
    print("history:{tree.workflow_history(1) ?? panic("history")}")
}
"#;

/// Cancellation must stop the timer and leave a durable cancellation result
/// for the next workflow body pass.
#[test]
fn workflow_sleep_cancellation_is_recorded() {
    if !have_rustc() {
        return;
    }
    let expected = "cancelled\nhistory:start@v1|sleep:10000000000|sleep-cancelled\n";

    let (code, stdout) = build_and_run(
        "services_workflow_sleep_cancel",
        WORKFLOW_SLEEP_CANCEL_SOURCE,
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, expected);

    let (jit_code, jit_stdout, jit_stderr) = run_default_multi(
        "services_workflow_sleep_cancel_jit",
        "main.jet",
        &[("main.jet", WORKFLOW_SLEEP_CANCEL_SOURCE)],
    );
    assert_eq!(
        jit_code, 0,
        "default workflow cancellation failed: {jit_stderr}"
    );
    assert_eq!(jit_stdout, stdout, "default workflow cancellation diverged");

    let (interpreter_code, interpreter_stdout, interpreter_stderr) = interpreter_run(
        "services_workflow_sleep_cancel_interpreter",
        WORKFLOW_SLEEP_CANCEL_SOURCE,
    );
    assert_eq!(
        interpreter_code, 0,
        "interpreter workflow cancellation failed: {interpreter_stderr}"
    );
    assert_eq!(
        interpreter_stdout, stdout,
        "interpreter workflow cancellation diverged"
    );
}

/// Activity retry and terminal workflow results must remain typed after a
/// process restart, including an AOT/default cross-tier replay.
#[test]
fn workflow_activity_outcome_survives_process_restart() {
    if !have_rustc() {
        return;
    }
    let (dir, bin) = compile_restart_binary(WORKFLOW_OUTCOME_SOURCE);
    let store = dir.join("workflow-outcome.log");
    assert_eq!(
        run_restart_process(&bin, &store, "write", None),
        format!(
            "scheduled:Running\nretry:Paused\ncompleted:Finished\nrun:Finished\nobserve:Observe(workers=1, started=true, generation=1, dead_letters=0, events=0, chaos=0, draining=0, partitions=0, rollback=false, workflows=1,statuses={{running:0,paused:1,cancel_requested:0}},outcomes={{pending:0,finished:1,panicked:0,cancelled:0,deadline_blown:0}})\nhistory:{WORKFLOW_OUTCOME_HISTORY}"
        )
    );
    assert_eq!(
        run_restart_process(&bin, &store, "read", None),
        format!("replay:Finished\nobserve:Observe(workers=1, started=true, generation=1, dead_letters=0, events=0, chaos=0, draining=0, partitions=0, rollback=false, workflows=1,statuses={{running:0,paused:1,cancel_requested:0}},outcomes={{pending:0,finished:1,panicked:0,cancelled:0,deadline_blown:0}})\nsame:Finished\nversion_after_terminal:2\nversion_history:start@v2\nhistory:{WORKFLOW_OUTCOME_HISTORY}")
    );

    let default_store = dir.join("workflow-outcome-default.log");
    assert_eq!(
        run_restart_default_process(&dir, &default_store, "write", None),
        format!(
            "scheduled:Running\nretry:Paused\ncompleted:Finished\nrun:Finished\nobserve:Observe(workers=1, started=true, generation=1, dead_letters=0, events=0, chaos=0, draining=0, partitions=0, rollback=false, workflows=1,statuses={{running:0,paused:1,cancel_requested:0}},outcomes={{pending:0,finished:1,panicked:0,cancelled:0,deadline_blown:0}})\nhistory:{WORKFLOW_OUTCOME_HISTORY}"
        )
    );
    assert_eq!(
        run_restart_default_process(&dir, &default_store, "read", None),
        format!("replay:Finished\nobserve:Observe(workers=1, started=true, generation=1, dead_letters=0, events=0, chaos=0, draining=0, partitions=0, rollback=false, workflows=1,statuses={{running:0,paused:1,cancel_requested:0}},outcomes={{pending:0,finished:1,panicked:0,cancelled:0,deadline_blown:0}})\nsame:Finished\nversion_after_terminal:2\nversion_history:start@v2\nhistory:{WORKFLOW_OUTCOME_HISTORY}")
    );
}

const SOURCE: &str = r#"
use core.service as services
use core.testing as testing

fn failure_worker() {}

fn run() {
    tree := services.tree("delivery")
    tree.set_delivery(services.delivery_durable()) ?? panic("delivery")
    temp := testing.temp_dir("service-failure")
    store_path :: Path.from(temp).join("delivery.state").to_string()
    store :: services.state_store(store_path) ?? panic("state store")
    tree.set_state_event_log(store, "delivery-events", 1, "reversible") ?? panic("state")
    worker :: tree.worker("worker", failure_worker, capacity: 1) ?? panic("worker")
    tree.start() ?? panic("start")

    first :: tree.send_durable(worker, "first", key: "k1") ?? panic("first")
    first.status() ?? panic("first status")
    duplicate :: tree.send_durable(worker, "first", key: "k1") ?? panic("duplicate")
    duplicate.status() ?? panic("duplicate status")
    conflicting :: tree.send_durable(worker, "different", key: "k1")
    if conflicting == {
        .Ok(delivery) -> {
            cancelled :: delivery.cancel() ?? panic("unexpected conflict delivery")
            cancelled.status() ?? panic("unexpected conflict status")
            print("conflict:accepted")
        }
        .Err(_) -> { print("conflict:rejected") }
    }
    full :: tree.send_durable(worker, "second", key: "k2")
    if full == {
        .Ok(delivery) -> {
            cancelled :: delivery.cancel() ?? panic("unexpected full delivery")
            cancelled.status() ?? panic("unexpected full status")
            print("full:accepted")
        }
        .Err(_) -> { print("full:rejected") }
    }
    print("dead_letters:{tree.dead_letter_count()}")

    tree.receive(worker) ?? panic("receive")
    tree.drain_worker(worker) ?? panic("drain")
    stopped_receive :: tree.receive(worker)
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
    assert_eq!(
        stdout,
        "conflict:rejected\nfull:rejected\ndead_letters:1\ndrained_receive:rejected\n"
    );
}

#[test]
fn services_failure_paths_match_default_run() {
    let (code, stdout, stderr) = run_default_multi(
        "services_failure_paths_jit",
        "main.jet",
        &[("main.jet", SOURCE)],
    );
    assert_eq!(code, 0, "default jet run failed: {stderr}");
    assert_eq!(
        stdout,
        "conflict:rejected\nfull:rejected\ndead_letters:1\ndrained_receive:rejected\n"
    );
}

const DRAIN_HANDOFF_SOURCE: &str = r#"
use core.service as service

fn worker() {}

fn run() {
    tree := service.tree("drain-handoff")
    endpoint :: tree.worker("api", worker, capacity: 2) ?? panic("worker")
    tree.start() ?? panic("start")
    tree.send(endpoint, "queued") ?? panic("queued")
    tree.directory_register("api", endpoint) ?? panic("directory")
    tree.drain_worker(endpoint) ?? panic("drain")

    late :: endpoint.send("late")
    if late == {
        .Ok(_) -> { print("late:accepted") }
        .Err(_) -> { print("late:rejected") }
    }
    print("drained:{tree.receive(endpoint) ?? panic("receive")}")

    tree.stop() ?? panic("restart stop")
    tree.start() ?? panic("restart start")
    tree.directory_register("api", endpoint) ?? panic("restart directory")
    generation :: tree.handoff_generation() ?? panic("handoff")
    current :: tree.directory_resolve("api") ?? panic("resolve")
    current.send("after") ?? panic("after send")
    print("handoff:{generation}:{current.show()}")
    print("after:{tree.receive(current) ?? panic("after receive")}")
    tree.stop() ?? panic("stop")
}
"#;

const DRAIN_HANDOFF_OUTPUT: &str =
    "late:rejected\ndrained:queued\nhandoff:2:Endpoint(drain-handoff/api@g2)\nafter:after\n";

#[test]
fn service_rollout_drain_handoff_orders_endpoint_gate_and_new_generation_aot() {
    if !have_rustc() {
        return;
    }
    let (code, stdout) = build_and_run("services_drain_handoff", DRAIN_HANDOFF_SOURCE);
    assert_eq!(code, 0);
    assert_eq!(stdout, DRAIN_HANDOFF_OUTPUT);
}

#[test]
fn service_rollout_drain_handoff_orders_endpoint_gate_and_new_generation_default_run() {
    let (code, stdout, stderr) = run_default_multi(
        "services_drain_handoff_jit",
        "main.jet",
        &[("main.jet", DRAIN_HANDOFF_SOURCE)],
    );
    assert_eq!(code, 0, "default jet run failed: {stderr}");
    assert_eq!(stdout, DRAIN_HANDOFF_OUTPUT);
}

#[test]
fn service_rollout_drain_handoff_orders_endpoint_gate_and_new_generation_interpreter() {
    let (code, stdout, stderr) =
        interpreter_run("services_drain_handoff_interpreter", DRAIN_HANDOFF_SOURCE);
    assert_eq!(code, 0, "interpreter run failed: {stderr}");
    assert_eq!(stdout, DRAIN_HANDOFF_OUTPUT);
}

const DRAIN_DURABLE_SOURCE: &str = r#"
use core.service as service
use core.testing as testing
use core.time as time

fn worker() {}

fn receipt_id(receipt: ^Delivery) Delivery -> {
    return receipt
}

fn run() {
    temp := testing.temp_dir("drain-durable")
    store :: Path.from(temp).join("authority.log").to_string()
    retention :: Duration.seconds(86400) ?? panic("retention")
    runtime := service.runtime(store, retention: retention)
    tree := service.tree("drain-durable")
    endpoint :: tree.worker("api", worker, capacity: 2) ?? panic("worker")
    tree.start() ?? panic("start")
    tree.send(endpoint, "before") ?? panic("before")
    tree.drain_worker(endpoint) ?? panic("drain")
    receipt :: runtime.send(endpoint, "after", key: "after") ?? panic("send")
    id :: receipt_id(receipt)
    print("accepted")
    print("first:{tree.receive(endpoint) ?? panic("first")}")
    print("second:{tree.receive(endpoint) ?? panic("second")}")
    runtime.commit(id) ?? panic("commit")
    print("generation:{tree.handoff_generation() ?? panic("handoff")}")
}
"#;

const DRAIN_DURABLE_OUTPUT: &str = "accepted\nfirst:before\nsecond:after\ngeneration:2\n";

#[test]
fn service_drain_consumes_in_flight_durable_receipts_before_handoff_aot() {
    if !have_rustc() {
        return;
    }
    let (code, stdout) = build_and_run("services_drain_durable", DRAIN_DURABLE_SOURCE);
    assert_eq!(code, 0);
    assert_eq!(stdout, DRAIN_DURABLE_OUTPUT);
}

#[test]
fn service_drain_consumes_in_flight_durable_receipts_before_handoff_default_run() {
    let (code, stdout, stderr) = run_default_multi(
        "services_drain_durable_jit",
        "main.jet",
        &[("main.jet", DRAIN_DURABLE_SOURCE)],
    );
    assert_eq!(code, 0, "default jet run failed: {stderr}");
    assert_eq!(stdout, DRAIN_DURABLE_OUTPUT);
}

const RUNTIME_HANDOFF_PENDING_SOURCE: &str = r#"
use core.service as service
use core.testing as testing
use core.time as time

fn worker() {}

fn receipt_id(receipt: ^Delivery) Delivery -> {
    return receipt
}

fn receipt_kind(receipt: ^Delivery) String -> {
    state :: receipt.status() ?? panic("status")
    if state == {
        .Pending -> { return "pending" }
        .Accepted -> { return "accepted" }
        .Delivering -> { return "delivering" }
        .Delivered -> { return "delivered" }
        .DeadLettered -> { return "dead" }
        .Cancelled -> { return "cancelled" }
    }
    return "unknown"
}

fn run() {
    temp := testing.temp_dir("runtime-handoff-pending")
    store :: Path.from(temp).join("authority.log").to_string()
    retention :: Duration.seconds(86400) ?? panic("retention")
    runtime := service.runtime(store, retention: retention)
    tree := service.tree("runtime-handoff-pending")
    endpoint :: tree.worker("api", worker, capacity: 1) ?? panic("worker")
    tree.start() ?? panic("start")

    receipt :: runtime.send(endpoint, "pending", key: "pending") ?? panic("send")
    id :: receipt_id(~receipt)
    print("sent:{receipt_kind(^receipt)}")
    print("handoff:{tree.handoff_generation() ?? panic("handoff")}")
    print("receipt:{tree.upgrade_receipt() ?? panic("receipt")}")
    print("received:{tree.receive(endpoint) ?? panic("receive")}")
    runtime.commit(id) ?? panic("commit")
    print("next:{tree.handoff_generation() ?? panic("next handoff")}")
    tree.stop() ?? panic("stop")
}
"#;

const RUNTIME_HANDOFF_PENDING_OUTPUT: &str =
    "sent:accepted\nhandoff:2\nreceipt:ServiceUpgradeReceipt(from=1, to=2, migration=none, rollback_available=false, pinned=api)\nreceived:pending\nnext:3\n";

#[test]
fn runtime_receipt_pins_pending_shard_across_handoff_aot() {
    if !have_rustc() {
        return;
    }
    let (code, stdout) = build_and_run(
        "services_runtime_handoff_pending",
        RUNTIME_HANDOFF_PENDING_SOURCE,
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, RUNTIME_HANDOFF_PENDING_OUTPUT);
}

#[test]
fn runtime_receipt_pins_pending_shard_across_handoff_default_run() {
    let (code, stdout, stderr) = run_default_multi(
        "services_runtime_handoff_pending_jit",
        "main.jet",
        &[("main.jet", RUNTIME_HANDOFF_PENDING_SOURCE)],
    );
    assert_eq!(code, 0, "default jet run failed: {stderr}");
    assert_eq!(stdout, RUNTIME_HANDOFF_PENDING_OUTPUT);
}

#[test]
fn runtime_receipt_pins_pending_shard_across_handoff_interpreter() {
    let (code, stdout, stderr) = interpreter_run(
        "services_runtime_handoff_pending_interpreter",
        RUNTIME_HANDOFF_PENDING_SOURCE,
    );
    assert_eq!(code, 0, "interpreter run failed: {stderr}");
    assert_eq!(stdout, RUNTIME_HANDOFF_PENDING_OUTPUT);
}

const DRAIN_EMPTY_DURABLE_SOURCE: &str = r#"
use core.service as service
use core.testing as testing
use core.time as time

fn worker() {}

fn receipt_id(receipt: ^Delivery) Delivery -> {
    return receipt
}

fn run() {
    temp := testing.temp_dir("drain-empty-durable")
    store :: Path.from(temp).join("authority.log").to_string()
    retention :: Duration.seconds(86400) ?? panic("retention")
    runtime := service.runtime(store, retention: retention)
    tree := service.tree("drain-empty-durable")
    endpoint :: tree.worker("api", worker, capacity: 1) ?? panic("worker")
    tree.start() ?? panic("start")
    tree.drain_worker(endpoint) ?? panic("drain")
    receipt :: runtime.send(endpoint, "after", key: "after") ?? panic("send")
    id :: receipt_id(receipt)
    print("accepted")
    print("received:{tree.receive(endpoint) ?? panic("receive")}")
    runtime.commit(id) ?? panic("commit")
    print("committed")
    tree.stop() ?? panic("stop")
}
"#;

const DRAIN_EMPTY_DURABLE_OUTPUT: &str = "accepted\nreceived:after\ncommitted\n";

#[test]
fn service_empty_drain_preserves_durable_receipts_aot() {
    if !have_rustc() {
        return;
    }
    let (code, stdout) = build_and_run("services_drain_empty_durable", DRAIN_EMPTY_DURABLE_SOURCE);
    assert_eq!(code, 0);
    assert_eq!(stdout, DRAIN_EMPTY_DURABLE_OUTPUT);
}

#[test]
fn service_empty_drain_preserves_durable_receipts_default_run() {
    let (code, stdout, stderr) = run_default_multi(
        "services_drain_empty_durable_jit",
        "main.jet",
        &[("main.jet", DRAIN_EMPTY_DURABLE_SOURCE)],
    );
    assert_eq!(code, 0, "default jet run failed: {stderr}");
    assert_eq!(stdout, DRAIN_EMPTY_DURABLE_OUTPUT);
}

const DURABLE_RETRY_SOURCE: &str = r#"
use core.service as service
use core.testing as testing
use core.time as time

fn worker() {}

fn receipt_id(receipt: ^Delivery) Delivery -> {
    return receipt
}

fn run() {
    temp := testing.temp_dir("durable-retry")
    state_path :: Path.from(temp).join("state.log").to_string()
    delivery_path :: Path.from(temp).join("state.log.delivery").to_string()
    retention :: Duration.seconds(86400) ?? panic("retention")
    runtime := service.runtime(delivery_path, retention: retention)
    tree := service.tree("durable-retry")
    tree.set_delivery(service.delivery_durable()) ?? panic("delivery")
    state :: service.state_store(state_path) ?? panic("state")
    tree.set_state_event_log(state, "events", 1, "reversible") ?? panic("state adapter")
    endpoint :: tree.worker("api", worker, capacity: 1) ?? panic("worker")
    tree.start() ?? panic("start")

    receipt :: tree.send_durable(endpoint, "once", key: "once") ?? panic("send")
    id :: receipt_id(receipt)
    print("first:{tree.receive(endpoint) ?? panic("first")}")
    duplicate :: tree.receive(endpoint)
    if duplicate == {
        .Ok(_) -> { print("duplicate:accepted") }
        .Err(_) -> { print("duplicate:rejected") }
    }
    retry :: runtime.retry(id) ?? panic("retry")
    print("retry:{tree.receive(endpoint) ?? panic("retry receive")}")
    runtime.commit(retry) ?? panic("commit")
    tree.stop() ?? panic("stop")
}
"#;

const DURABLE_RETRY_OUTPUT: &str = "first:once\nduplicate:rejected\nretry:once\n";

#[test]
fn durable_retry_is_the_only_explicit_redelivery_aot() {
    if !have_rustc() {
        return;
    }
    let (code, stdout) = build_and_run("services_durable_retry", DURABLE_RETRY_SOURCE);
    assert_eq!(code, 0);
    assert_eq!(stdout, DURABLE_RETRY_OUTPUT);
}

#[test]
fn durable_retry_is_the_only_explicit_redelivery_default_run() {
    let (code, stdout, stderr) = run_default_multi(
        "services_durable_retry_jit",
        "main.jet",
        &[("main.jet", DURABLE_RETRY_SOURCE)],
    );
    assert_eq!(code, 0, "default jet run failed: {stderr}");
    assert_eq!(stdout, DURABLE_RETRY_OUTPUT);
}

#[test]
fn durable_retry_is_the_only_explicit_redelivery_interpreter() {
    let (code, stdout, stderr) =
        interpreter_run("services_durable_retry_interpreter", DURABLE_RETRY_SOURCE);
    assert_eq!(code, 0, "interpreter run failed: {stderr}");
    assert_eq!(stdout, DURABLE_RETRY_OUTPUT);
}

const TREE_DRAIN_REJECT_SOURCE: &str = r#"
use core.service as service
use core.testing as testing

fn worker() {}

fn run() {
    temp := testing.temp_dir("tree-drain-reject")
    store :: Path.from(temp).join("authority.log").to_string()
    tree := service.tree("tree-drain-reject")
    tree.set_delivery(service.delivery_durable()) ?? panic("delivery")
    state :: service.state_store(store) ?? panic("state")
    tree.set_state_event_log(state, "events", 1, "reversible") ?? panic("state adapter")
    endpoint :: tree.worker("api", worker, capacity: 1) ?? panic("worker")
    tree.start() ?? panic("start")
    tree.directory_register("api", endpoint) ?? panic("directory")
    tree.drain_worker(endpoint) ?? panic("drain")
    rejected :: tree.send_durable(endpoint, "after", key: "after")
    if rejected == {
        .Ok(_) -> { print("direct:accepted") }
        .Err(_) -> { print("direct:rejected") }
    }
    print("handoff:{tree.handoff_generation() ?? panic("handoff")}")
    current :: tree.directory_resolve("api") ?? panic("resolve")
    pending :: tree.receive(current)
    if pending == {
        .Ok(_) -> { print("pending:accepted") }
        .Err(_) -> { print("pending:rejected") }
    }
    tree.stop() ?? panic("stop")
}
"#;

const TREE_DRAIN_REJECT_OUTPUT: &str = "direct:rejected\nhandoff:2\npending:rejected\n";

#[test]
fn tree_durable_send_during_drain_is_rejected_before_admission_aot() {
    if !have_rustc() {
        return;
    }
    let (code, stdout) = build_and_run("services_tree_drain_reject", TREE_DRAIN_REJECT_SOURCE);
    assert_eq!(code, 0);
    assert_eq!(stdout, TREE_DRAIN_REJECT_OUTPUT);
}

#[test]
fn tree_durable_send_during_drain_is_rejected_before_admission_default_run() {
    let (code, stdout, stderr) = run_default_multi(
        "services_tree_drain_reject_jit",
        "main.jet",
        &[("main.jet", TREE_DRAIN_REJECT_SOURCE)],
    );
    assert_eq!(code, 0, "default jet run failed: {stderr}");
    assert_eq!(stdout, TREE_DRAIN_REJECT_OUTPUT);
}

#[test]
fn tree_durable_send_during_drain_is_rejected_before_admission_interpreter() {
    let (code, stdout, stderr) = interpreter_run(
        "services_tree_drain_reject_interpreter",
        TREE_DRAIN_REJECT_SOURCE,
    );
    assert_eq!(code, 0, "interpreter run failed: {stderr}");
    assert_eq!(stdout, TREE_DRAIN_REJECT_OUTPUT);
}

const ROLLOUT_ROLLBACK_SOURCE: &str = r#"
use core.service as service
use core.testing as testing

fn worker() {}

fn run() {
    temp := testing.temp_dir("rollout-rollback")
    path :: Path.from(temp).join("events.log").to_string()
    tree := service.tree("rollback")
    store :: service.state_store(path) ?? panic("store")
    tree.set_state_event_log(store, "events", 1, "reversible") ?? panic("state")
    endpoint :: tree.worker("api", worker, capacity: 1) ?? panic("worker")
    tree.start() ?? panic("start")
    tree.append_event("before") ?? panic("before")
    tree.drain_worker(endpoint) ?? panic("drain")
    tree.handoff_generation() ?? panic("handoff")
    receipt :: tree.upgrade_receipt() ?? panic("receipt")
    print("receipt:{receipt}")
    tree.append_event("after") ?? panic("after")
    rolled :: tree.rollback_generation()
    if rolled == {
        .Ok(generation) -> { print("rolled:{generation}:{tree.replay_events()}") }
        .Err(_) -> { print("rolled:refused") }
    }
    tree.stop() ?? panic("stop")
}
"#;

const ROLLOUT_ROLLBACK_OUTPUT: &str =
    "receipt:ServiceUpgradeReceipt(from=1, to=2, migration=reversible, rollback_available=true, pinned=)\nrolled:1:before\n";

#[test]
fn service_rollout_rollback_restores_pre_handoff_state_aot() {
    if !have_rustc() {
        return;
    }
    let (code, stdout) = build_and_run("services_rollout_rollback", ROLLOUT_ROLLBACK_SOURCE);
    assert_eq!(code, 0);
    assert_eq!(stdout, ROLLOUT_ROLLBACK_OUTPUT);
}

#[test]
fn service_rollout_rollback_restores_pre_handoff_state_default_run() {
    let (code, stdout, stderr) = run_default_multi(
        "services_rollout_rollback_jit",
        "main.jet",
        &[("main.jet", ROLLOUT_ROLLBACK_SOURCE)],
    );
    assert_eq!(code, 0, "default jet run failed: {stderr}");
    assert_eq!(stdout, ROLLOUT_ROLLBACK_OUTPUT);
}

const ROLLOUT_PINNED_ROLLBACK_SOURCE: &str = r#"
use core.service as service

fn worker() {}

fn run() {
    tree := service.tree("pinned-rollback")
    endpoint :: tree.worker("api", worker, capacity: 1) ?? panic("worker")
    tree.start() ?? panic("start")
    tree.send(endpoint, "queued") ?? panic("send")
    tree.drain_worker(endpoint) ?? panic("drain")
    generation :: tree.handoff_generation() ?? panic("handoff")
    print("handoff:{generation}")
    print("rolled:{tree.rollback_generation() ?? panic("rollback")}")
    print("message:{tree.receive(endpoint) ?? panic("receive")}")
    tree.stop() ?? panic("stop")
}
"#;

const ROLLOUT_PINNED_ROLLBACK_OUTPUT: &str = "handoff:2\nrolled:1\nmessage:queued\n";

#[test]
fn service_rollout_rollback_restores_pinned_shard_aot() {
    if !have_rustc() {
        return;
    }
    let (code, stdout) = build_and_run(
        "services_rollout_pinned_rollback",
        ROLLOUT_PINNED_ROLLBACK_SOURCE,
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, ROLLOUT_PINNED_ROLLBACK_OUTPUT);
}

#[test]
fn service_rollout_rollback_restores_pinned_shard_default_run() {
    let (code, stdout, stderr) = run_default_multi(
        "services_rollout_pinned_rollback_jit",
        "main.jet",
        &[("main.jet", ROLLOUT_PINNED_ROLLBACK_SOURCE)],
    );
    assert_eq!(code, 0, "default jet run failed: {stderr}");
    assert_eq!(stdout, ROLLOUT_PINNED_ROLLBACK_OUTPUT);
}

const STATE_AND_LIFECYCLE_SOURCE: &str = r#"
use core.service as services
use core.testing as testing

fn lifecycle_worker() {}

fn run() {
    temp := testing.temp_dir("service-state")
    snapshot := services.tree("snapshot")
    snapshot_store :: Path.from(temp).join("snapshot.state").to_string()
    snapshot_authority :: services.state_store(snapshot_store) ?? panic("snapshot store")
    snapshot.set_state_snapshot(snapshot_authority, "snapshot", 1, "reversible") ?? panic("snapshot state")
    snapshot_worker :: snapshot.worker("worker", lifecycle_worker, capacity: 2) ?? panic("snapshot worker")
    snapshot.start() ?? panic("snapshot start")
    snapshot.commit_snapshot("state-v1") ?? panic("snapshot commit")
    restored :: snapshot.restore_snapshot() ?? panic("snapshot restore")
    print("snapshot:{restored}")
    snapshot.stop() ?? panic("snapshot stop")

    events := services.tree("events")
    event_store :: Path.from(temp).join("events.state").to_string()
    event_authority :: services.state_store(event_store) ?? panic("event store")
    events.set_state_event_log(event_authority, "events", 1, "reversible") ?? panic("event state")
    event_worker :: events.worker("worker", lifecycle_worker, capacity: 2) ?? panic("event worker")
    events.start() ?? panic("event start")
    events.append_event("first") ?? panic("event one")
    events.append_event("second") ?? panic("event two")
    print("events:{events.replay_events()}")
    print("event_count:{events.event_count()}")
    events.stop() ?? panic("event stop")

    workflow := services.tree("workflow")
    workflow_store_path :: Path.from(temp).join("workflow.state").to_string()
    workflow_store :: services.state_store(workflow_store_path) ?? panic("workflow store")
    workflow.set_state_event_log(workflow_store, "workflow-events", 1, "reversible") ?? panic("workflow state")
    workflow_worker :: workflow.worker("worker", lifecycle_worker, capacity: 2) ?? panic("workflow worker")
    workflow.start() ?? panic("workflow start")
    run_id :: workflow.workflow_start("checkout", 1) ?? panic("workflow id")
    same_run :: workflow.workflow_start("checkout", 1) ?? panic("workflow duplicate")
    workflow.workflow_step(run_id, "charge") ?? panic("workflow step")
    history :: workflow.workflow_history(run_id) ?? panic("workflow history")
    print("workflow:{run_id}:{same_run}:{history}")
    versioned :: workflow.workflow_start("checkout", 2)
    if versioned == {
        .Ok(_) -> { print("workflow_version:accepted") }
        .Err(_) -> { print("workflow_version:rejected") }
    }
    workflow.stop() ?? panic("workflow stop")

    cluster := services.tree("cluster")
    endpoint :: cluster.worker("api", lifecycle_worker, capacity: 2) ?? panic("cluster worker")
    cluster.start() ?? panic("cluster start")
    cluster.directory_register("api", endpoint) ?? panic("directory register")
    cluster.drain_worker(endpoint) ?? panic("cluster drain")
    handed :: cluster.handoff_generation() ?? panic("handoff")
    receipt :: cluster.upgrade_receipt() ?? panic("upgrade receipt")
    current :: cluster.directory_resolve("api") ?? panic("directory resolve")
    print("generation:{handed}:{cluster.directory_generation()}:{current.show()}:{receipt}")
    stale :: cluster.send(endpoint, "late")
    if stale == {
        .Ok(_) -> { print("stale:accepted") }
        .Err(_) -> { print("stale:rejected") }
    }
    rolled :: cluster.rollback_generation() ?? panic("rollback")
    print("rollback:{rolled}:{cluster.directory_generation()}")
    cluster.chaos_fail() ?? panic("chaos")
    print(cluster.observe())
    cluster.stop() ?? panic("cluster stop")
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
        "snapshot:state-v1\nevents:first|second\nevent_count:2\nworkflow:1:1:start@v1|step:charge\nworkflow_version:rejected\ngeneration:2:2:Endpoint(cluster/api@g2):ServiceUpgradeReceipt(from=1, to=2, migration=none, rollback_available=false, pinned=)\nstale:rejected\nrollback:1:1\nObserve(workers=1, started=true, generation=1, dead_letters=0, events=0, chaos=1, draining=0, partitions=0, rollback=false, workflows=0,statuses={running:0,paused:0,cancel_requested:0},outcomes={pending:0,finished:0,panicked:0,cancelled:0,deadline_blown:0})\n"
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
        "snapshot:state-v1\nevents:first|second\nevent_count:2\nworkflow:1:1:start@v1|step:charge\nworkflow_version:rejected\ngeneration:2:2:Endpoint(cluster/api@g2):ServiceUpgradeReceipt(from=1, to=2, migration=none, rollback_available=false, pinned=)\nstale:rejected\nrollback:1:1\nObserve(workers=1, started=true, generation=1, dead_letters=0, events=0, chaos=1, draining=0, partitions=0, rollback=false, workflows=0,statuses={running:0,paused:0,cancel_requested:0},outcomes={pending:0,finished:0,panicked:0,cancelled:0,deadline_blown:0})\n"
    );
}

/// Durable delivery and an event log share one service tree but must not share
/// one file. The delivery log and the state store use different framing, and
/// the state store is read back with the adapter its header declares, so a
/// durable send used to leave records the typed read could not parse: a later
/// `append_event` failed on any tree that had accepted a `send_durable`.
///
/// No existing check combined the two surfaces on one tree, so the collision
/// only showed up in an example.
const DURABLE_PLUS_EVENT_LOG_SOURCE: &str = r#"
use core.service as services
use core.testing as testing

fn durable_worker() {}

fn run() {
    tree := services.tree("app")
    tree.set_delivery(services.delivery_durable()) ?? panic("delivery")
    temp := testing.temp_dir("services-delivery-eventlog")
    store_path :: Path.from(temp).join("state.log").to_string()
    store :: services.state_store(store_path) ?? panic("state store")
    tree.set_state_event_log(store, "app-events", 1, "reversible") ?? panic("state")
    worker :: tree.worker("a", durable_worker, capacity: 4) ?? panic("worker")
    tree.start() ?? panic("start")

    first :: tree.send_durable(worker, "ping", key: "k1") ?? panic("durable send")
    first.status() ?? panic("first status")
    tree.append_event("after-durable-send") ?? panic("append after durable send")
    second :: tree.send_durable(worker, "pong", key: "k2") ?? panic("second durable send")
    second.status() ?? panic("second status")
    tree.append_event("after-second-send") ?? panic("append after second send")

    print("events:{tree.event_count()}")
}
"#;

#[test]
fn durable_delivery_does_not_corrupt_the_event_log_aot() {
    if !have_rustc() {
        return;
    }
    let (code, stdout) = build_and_run("services_durable_eventlog", DURABLE_PLUS_EVENT_LOG_SOURCE);
    assert_eq!(code, 0, "durable send beside an event log must not fail");
    assert_eq!(stdout, "events:2\n");
}

#[test]
fn durable_delivery_does_not_corrupt_the_event_log_default_run() {
    let (code, stdout, stderr) = run_default_multi(
        "services_durable_eventlog_jit",
        "main.jet",
        &[("main.jet", DURABLE_PLUS_EVENT_LOG_SOURCE)],
    );
    assert_eq!(code, 0, "default jet run failed: {stderr}");
    assert_eq!(stdout, "events:2\n");
}

/// #1149: the snapshot and event-log adapters must survive a reopen and refuse
/// a store they cannot trust. The state store is framed with a magic line, an
/// adapter line and a schema line, so garbage in the file has to surface as a
/// typed error rather than a panic or a silently empty restore.
const STATE_ADAPTER_SOURCE: &str = r#"
use core.files as files
use core.service as services
use core.testing as testing

fn adapter_worker() {}

fn run() {
    temp := testing.temp_dir("services-state-adapters")

    snapshot_path :: Path.from(temp).join("snapshot.log").to_string()
    snap_tree := services.tree("snap")
    snap_store :: services.state_store(snapshot_path) ?? panic("snapshot store")
    snap_tree.set_state_snapshot(snap_store, "app-state", 1, "reversible") ?? panic("snapshot adapter")
    _snap_worker :: snap_tree.worker("a", adapter_worker, capacity: 2) ?? panic("snapshot worker")
    snap_tree.start() ?? panic("snapshot start")
    snap_tree.commit_snapshot("state-v1") ?? panic("commit")
    print("restored:{snap_tree.restore_snapshot() ?? panic("restore")}")

    event_path :: Path.from(temp).join("events.log").to_string()
    log_tree := services.tree("log")
    log_store :: services.state_store(event_path) ?? panic("event store")
    log_tree.set_state_event_log(log_store, "app-events", 1, "reversible") ?? panic("event adapter")
    _log_worker :: log_tree.worker("a", adapter_worker, capacity: 2) ?? panic("event worker")
    log_tree.start() ?? panic("event start")
    log_tree.append_event("first") ?? panic("first event")
    log_tree.append_event("second") ?? panic("second event")
    print("replay:{log_tree.replay_events()}")

    // A store the runtime cannot trust must fail closed, not restore nothing.
    files.write(snapshot_path, "not a service state store") ?? panic("corrupt write")
    corrupted :: snap_tree.restore_snapshot()
    if corrupted == {
        .Ok(_) -> { print("corrupt:accepted") }
        .Err(_) -> { print("corrupt:rejected") }
    }
}
"#;

#[test]
fn state_adapters_reopen_and_reject_corrupt_stores_aot() {
    if !have_rustc() {
        return;
    }
    let (code, stdout) = build_and_run("services_state_adapters", STATE_ADAPTER_SOURCE);
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "restored:state-v1\nreplay:first|second\ncorrupt:rejected\n"
    );
}

#[test]
fn state_adapters_reopen_and_reject_corrupt_stores_default_run() {
    let (code, stdout, stderr) = run_default_multi(
        "services_state_adapters_jit",
        "main.jet",
        &[("main.jet", STATE_ADAPTER_SOURCE)],
    );
    assert_eq!(code, 0, "default jet run failed: {stderr}");
    assert_eq!(
        stdout,
        "restored:state-v1\nreplay:first|second\ncorrupt:rejected\n"
    );
}

/// The rollback path only does real work when the tree has a state adapter.
/// With no adapter, `prepare_rollback` returns early and `restore_rollback`
/// never touches the store, so a rollback test on a bare tree proves nothing
/// about the durable body. This sets an adapter first, so the upgrade really
/// copies the store aside and the rollback reads it back.
const ROLLBACK_WITH_STATE_SOURCE: &str = r#"
use core.sys as env
use core.service as services

fn rollback_worker() {}

fn run() {
    store_path :: env.get("JET_SERVICE_ROLLBACK_STORE") ?? panic("store")
    policy :: env.get("JET_SERVICE_ROLLBACK_POLICY") ?? panic("policy")
    tree := services.tree("cluster")
    store :: services.state_store(store_path) ?? panic("state store")
    tree.set_state_event_log(store, "cluster-events", 1, policy) ?? panic("state")
    endpoint :: tree.worker("api", rollback_worker, capacity: 1) ?? panic("worker")
    tree.start() ?? panic("start")
    tree.append_event("before-upgrade") ?? panic("append")
    tree.directory_register("api", endpoint) ?? panic("register")
    tree.drain_worker(endpoint) ?? panic("drain")
    tree.handoff_generation() ?? panic("handoff")
    receipt :: tree.upgrade_receipt() ?? panic("receipt")
    print("receipt:{receipt}")
    tree.append_event("after-upgrade") ?? panic("append after")
    rolled :: tree.rollback_generation()
    if rolled == {
        .Ok(generation) -> {
            events :: tree.replay_events()
            print("rolled:{generation}:{events}")
        }
        .Err(_) -> { print("rolled:refused") }
    }
    tree.stop() ?? panic("stop")
}
"#;

fn run_rollback_process(bin: &Path, store: &Path, policy: &str) -> (bool, String, String) {
    let output = Command::new(bin)
        .env("JET_SERVICE_ROLLBACK_STORE", store)
        .env("JET_SERVICE_ROLLBACK_POLICY", policy)
        .output()
        .unwrap();
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// A reversible migration copies the store aside on upgrade, so rolling back
/// restores the events as they stood before it.
#[test]
fn a_reversible_migration_rolls_the_state_store_back() {
    if !have_rustc() {
        return;
    }
    let (dir, bin) = compile_restart_binary(ROLLBACK_WITH_STATE_SOURCE);
    let store = dir.join("cluster.log");
    let (ok, stdout, stderr) = run_rollback_process(&bin, &store, "reversible");
    assert!(ok, "reversible rollback run failed: {stderr}");
    assert!(
        stdout.contains("rollback_available=true"),
        "a reversible migration must offer a rollback: {stdout}"
    );
    assert!(
        stdout.contains("rolled:1:before-upgrade"),
        "rolling back must restore the store as it stood before the upgrade: {stdout}"
    );
    assert!(
        !stdout.contains("after-upgrade"),
        "the event written after the upgrade must not survive the rollback: {stdout}"
    );
}

/// A forward-only migration refuses to roll back, and says so through the
/// receipt rather than by silently doing nothing.
#[test]
fn a_forward_only_migration_refuses_to_roll_back() {
    if !have_rustc() {
        return;
    }
    let (dir, bin) = compile_restart_binary(ROLLBACK_WITH_STATE_SOURCE);
    let store = dir.join("cluster.log");
    let (ok, stdout, stderr) = run_rollback_process(&bin, &store, "forward_only");
    assert!(ok, "forward-only run failed: {stderr}");
    assert!(
        stdout.contains("migration=forward_only"),
        "the receipt must name the policy the adapter was opened under: {stdout}"
    );
    assert!(
        stdout.contains("rollback_available=false"),
        "a forward-only migration must not offer a rollback: {stdout}"
    );
}

const PARTITION_RECONCILE_SOURCE: &str =
    include_str!("../examples/features/tooling/service_partition_reconcile.jet");

#[test]
fn grouped_durable_partition_reconciles_after_handoff() {
    if !have_rustc() {
        return;
    }
    let (code, stdout) = build_and_run("services_partition_reconcile", PARTITION_RECONCILE_SOURCE);
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "foreign:rejected\ndrained:ok\nexpired:rejected\npartition_route:rejected\npartitioned:ok\nhandoff:2\nServiceUpgradeReceipt(from=1, to=2, migration=reversible, rollback_available=true, pinned=api)\nreconciled:g2\nServiceUpgradeReceipt(from=1, to=2, migration=reversible, rollback_available=true, pinned=)\nrejoined:rejoined\n"
    );
}

#[test]
fn grouped_durable_partition_reconciles_under_default_run() {
    let (code, stdout, stderr) = run_default_multi(
        "services_partition_reconcile_jit",
        "main.jet",
        &[("main.jet", PARTITION_RECONCILE_SOURCE)],
    );
    assert_eq!(code, 0, "default jet run failed: {stderr}");
    assert_eq!(
        stdout,
        "foreign:rejected\ndrained:ok\nexpired:rejected\npartition_route:rejected\npartitioned:ok\nhandoff:2\nServiceUpgradeReceipt(from=1, to=2, migration=reversible, rollback_available=true, pinned=api)\nreconciled:g2\nServiceUpgradeReceipt(from=1, to=2, migration=reversible, rollback_available=true, pinned=)\nrejoined:rejoined\n"
    );
}

#[test]
fn grouped_durable_partition_reconciles_in_interpreter() {
    let (code, stdout, stderr) = interpreter_run(
        "services_partition_reconcile_interpreter",
        PARTITION_RECONCILE_SOURCE,
    );
    assert_eq!(
        code, 0,
        "interpreter service partition run failed: {stderr}"
    );
    assert_eq!(
        stdout,
        "foreign:rejected\ndrained:ok\nexpired:rejected\npartition_route:rejected\npartitioned:ok\nhandoff:2\nServiceUpgradeReceipt(from=1, to=2, migration=reversible, rollback_available=true, pinned=api)\nreconciled:g2\nServiceUpgradeReceipt(from=1, to=2, migration=reversible, rollback_available=true, pinned=)\nrejoined:rejoined\n"
    );
}

/// An unknown migration policy is a named error, not a silent default. The
/// policy decides whether a rollback is possible at all, so guessing it is
/// worse than refusing.
#[test]
fn an_unknown_migration_policy_is_refused() {
    if !have_rustc() {
        return;
    }
    let (dir, bin) = compile_restart_binary(ROLLBACK_WITH_STATE_SOURCE);
    let store = dir.join("cluster.log");
    let (ok, stdout, stderr) = run_rollback_process(&bin, &store, "whenever");
    assert!(
        !ok,
        "an unknown migration policy must stop the program: {stdout}{stderr}"
    );
}

const DURABLE_LIFECYCLE_MATRIX_SOURCE: &str = r#"
use core.service as service
use core.testing as testing
use core.time as time

fn worker() -[]> {}

fn state_name(delivery: ^Delivery) String -> {
    state :: delivery.status() ?? panic("status")
    if state == {
        .Pending -> { return "Pending" }
        .Accepted -> { return "Accepted" }
        .Delivering -> { return "Delivering" }
        .Delivered -> { return "Delivered" }
        .DeadLettered -> { return "DeadLettered" }
        .Cancelled -> { return "Cancelled" }
    }
    return "unknown"
}

fn run() {
    temp := testing.temp_dir("durable-lifecycle-matrix")
    delivery_path :: Path.from(temp).join("delivery.log").to_string()
    state_path :: Path.from(temp).join("state.log").to_string()
    retention :: Duration.seconds(60) ?? panic("retention")
    runtime := service.runtime(delivery_path, retention: retention)

    tree := service.tree("lifecycle-matrix")
    tree.set_delivery(service.delivery_durable()) ?? panic("delivery")
    state :: service.state_store(state_path) ?? panic("state store")
    tree.set_state_event_log(state, "lifecycle-events", 1, "reversible") ?? panic("state")
    endpoint :: tree.worker("api", worker, capacity: 2) ?? panic("worker")
    tree.start() ?? panic("start")

    accepted :: runtime.send(endpoint, "accepted", key: "same") ?? panic("accept")
    accepted_state :: state_name(~accepted)
    print("accept:{accepted_state}")
    duplicate :: runtime.send(endpoint, "accepted", key: "same") ?? panic("duplicate")
    duplicate_id :: (~duplicate).show()
    accepted_id :: (~accepted).show()
    duplicate_state :: state_name(~duplicate)
    print("duplicate:{duplicate_id == accepted_id}:{duplicate_state}")
    conflict :: runtime.send(endpoint, "different", key: "same")
    if conflict == {
        .Ok(delivery) -> {
            delivery.cancel() ?? panic("unexpected conflict")
            print("conflict:accepted")
        }
        .Err(_) -> { print("conflict:Policy") }
    }
    delivered :: tree.receive(endpoint) ?? panic("deliver")
    print("delivered:{delivered}")
    runtime.commit(accepted) ?? panic("commit")
    retained :: runtime.retain(duplicate) ?? panic("retain")
    retained_receipt :: (~retained).receipt() ?? panic("retained receipt")
    retained_text :: "{retained_receipt}"
    retained_events :: (~retained).events() ?? panic("retained events")
    retention_kept := retained_text.contains("retention_until=-1") == false
    retained_state :: state_name(^retained)
    print("retention:{retention_kept}:{retained_events.len()}:{retained_state}")

    retry_source :: runtime.send(endpoint, "retry", key: "retry") ?? panic("retry source")
    retry_first :: tree.receive(endpoint) ?? panic("retry first")
    print("retry_first:{retry_first}")
    retry :: runtime.retry(retry_source) ?? panic("retry")
    retry_receipt :: (~retry).receipt() ?? panic("retry receipt")
    retry_text :: "{retry_receipt}"
    retry_events :: (~retry).events() ?? panic("retry events")
    retried := retry_text.contains("attempts=1")
    retry_state :: state_name(~retry)
    print("retry_state:{retry_state}:{retried}:{retry_events.len()}")
    retry_second :: tree.receive(endpoint) ?? panic("retry second")
    print("retry_second:{retry_second}")
    retry_copy :: ~retry
    runtime.commit(retry) ?? panic("retry commit")
    retry_done :: state_name(^retry_copy)
    print("retry_done:{retry_done}")

    cancelled_handle :: runtime.send(endpoint, "cancel", key: "cancel") ?? panic("cancel source")
    cancelled :: cancelled_handle.cancel() ?? panic("cancel")
    cancel_state :: state_name(^cancelled)
    print("cancel:{cancel_state}")

    dead_handle :: runtime.send(endpoint, "dead", key: "dead") ?? panic("dead source")
    dead :: runtime.dead_letter(dead_handle) ?? panic("dead letter")
    dead_state :: state_name(^dead)
    print("dead:{dead_state}")

    short_runtime := service.runtime(delivery_path, retention: Duration.milliseconds(1) ?? panic("short retention"))
    expiring :: short_runtime.send(endpoint, "expire", key: "expire") ?? panic("expire source")
    time.sleep(Duration.milliseconds(10) ?? panic("sleep"))
    expiry_events :: (~expiring).events() ?? panic("expiry events")
    expiry_state :: state_name(^expiring)
    print("expiry:{expiry_state}:{expiry_events.len()}")

    partition_source :: runtime.send(endpoint, "partition", key: "partition") ?? panic("partition source")
    tree.partition_worker(endpoint) ?? panic("partition")
    partition_retry :: runtime.retry(partition_source)
    if partition_retry == {
        .Ok(delivery) -> {
            delivery.cancel() ?? panic("unexpected partition retry")
            print("partition:accepted")
        }
        .Err(_) -> { print("partition:Partitioned") }
    }
    tree.reconcile_worker(endpoint) ?? panic("reconcile")
    recovered :: runtime.send(endpoint, "partition", key: "partition") ?? panic("partition recovery")
    recovered_state :: state_name(~recovered)
    print("partition_recovery:{recovered_state}")
    partition_delivery :: tree.receive(endpoint) ?? panic("partition delivery")
    print("partition_delivery:{partition_delivery}")
    recovered_copy :: ~recovered
    runtime.commit(recovered) ?? panic("partition commit")
    recovered_events :: (~recovered_copy).events() ?? panic("recovered events")
    recovered_state_done :: state_name(^recovered_copy)
    print("partition_done:{recovered_state_done}:{recovered_events.len()}")

    foreign := service.tree("foreign")
    foreign_endpoint :: foreign.worker("api", worker, capacity: 1) ?? panic("foreign worker")
    foreign.start() ?? panic("foreign start")
    revoked :: tree.send_durable(foreign_endpoint, "revoked", key: "revoked")
    if revoked == {
        .Ok(delivery) -> {
            delivery.cancel() ?? panic("unexpected revoked delivery")
            print("revoked:accepted")
        }
        .Err(_) -> { print("revoked:Revoked") }
    }
    foreign.stop() ?? panic("foreign stop")
    tree.stop() ?? panic("stop")
}
"#;

#[test]
fn durable_lifecycle_matrix_covers_all_delivery_scenarios_on_all_tiers() {
    tir_support::assert_tiers_agree(
        "durable_lifecycle_matrix",
        DURABLE_LIFECYCLE_MATRIX_SOURCE,
        "accept:Accepted\nduplicate:true:Accepted\nconflict:Policy\ndelivered:accepted\nretention:true:3:Delivered\nretry_first:retry\nretry_state:Accepted:true:3\nretry_second:retry\nretry_done:Delivered\ncancel:Cancelled\ndead:DeadLettered\nexpiry:DeadLettered:2\npartition:Partitioned\npartition_recovery:Accepted\npartition_delivery:partition\npartition_done:Delivered:3\nrevoked:Revoked\n",
    );
}

const DURABLE_CRASH_REPLAY_SOURCE: &str = r#"
use core.service as service
use core.sys as env

fn worker() -[]> {}

fn state_name(delivery: ^Delivery) String -> {
    state :: delivery.status() ?? panic("status")
    if state == {
        .Pending -> { return "Pending" }
        .Accepted -> { return "Accepted" }
        .Delivering -> { return "Delivering" }
        .Delivered -> { return "Delivered" }
        .DeadLettered -> { return "DeadLettered" }
        .Cancelled -> { return "Cancelled" }
    }
    return "unknown"
}

fn run() {
    store :: env.get("JET_SERVICE_AUTH_STORE") ?? panic("store")
    phase :: env.get("JET_SERVICE_AUTH_PHASE") ?? panic("phase")
    retention :: Duration.seconds(60) ?? panic("retention")
    runtime := service.runtime(store, retention: retention)
    tree := service.tree("durable-crash")
    tree.set_delivery(service.delivery_durable()) ?? panic("delivery")
    endpoint :: tree.worker("api", worker, capacity: 1) ?? panic("worker")
    tree.start() ?? panic("start")

    if phase == "accept" {
        accepted :: runtime.send(endpoint, "crash", key: "crash") ?? panic("accept")
        id :: accepted.show()
        print("accepted:{id}")
    } else if phase == "deliver" {
        delivery :: runtime.send(endpoint, "crash", key: "crash") ?? panic("deliver lookup")
        id :: (~delivery).show()
        state :: state_name(^delivery)
        print("before:{id}:{state}")
        received :: tree.receive(endpoint) ?? panic("receive")
        print("received:{received}")
        print("crash_window:open")
    } else {
        recovered :: runtime.send(endpoint, "crash", key: "crash") ?? panic("recover lookup")
        recovered_id :: (~recovered).show()
        recovered_state :: state_name(~recovered)
        print("recovered:{recovered_id}:{recovered_state}")
        retry :: runtime.retry(recovered) ?? panic("recover retry")
        retry_id :: (~retry).show()
        retry_state :: state_name(~retry)
        retry_copy :: ~retry
        print("retry:{retry_id}:{retry_state}")
        received :: tree.receive(endpoint) ?? panic("replay receive")
        print("received:{received}")
        runtime.commit(retry) ?? panic("replay commit")
        history :: (~retry_copy).events() ?? panic("replay history")
        replayed_state :: state_name(^retry_copy)
        print("replayed:{replayed_state}:{history.len()}")
        print("same_identity:{recovered_id == retry_id}")
    }
}
"#;

#[test]
fn durable_lifecycle_crash_window_recovers_and_replays_one_identity() {
    if !have_rustc() {
        return;
    }
    let (dir, bin) = compile_restart_binary(DURABLE_CRASH_REPLAY_SOURCE);
    let store = dir.join("crash.log");
    let accepted = run_restart_process(&bin, &store, "accept", None);
    assert!(accepted.starts_with("accepted:Delivery(svc-"), "{accepted}");
    let delivered = run_restart_default_process(&dir, &store, "deliver", None);
    assert!(delivered.starts_with("before:Delivery(svc-"), "{delivered}");
    assert!(delivered.contains(":Accepted\nreceived:crash\ncrash_window:open\n"));
    let recovered = run_restart_process(&bin, &store, "recover", None);
    assert_eq!(
        recovered.lines().last(),
        Some("same_identity:true"),
        "recovery changed the durable identity: {recovered}"
    );
    assert!(
        recovered.contains("recovered:Delivery(svc-")
            && recovered.contains(":Delivering\nretry:Accepted\nreceived:crash\nreplayed:Delivered:5\n"),
        "crash-window replay did not rebuild the signed history: {recovered}"
    );
}
