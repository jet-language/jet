//! #286: real Event/AsyncEvent/DecisionHook runtime observations consumed by
//! the debugger. These tests attach to generated programs; no source scan can
//! satisfy them.

use std::fs;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

mod common;

struct Running {
    child: Child,
    dir: std::path::PathBuf,
}

impl Drop for Running {
    fn drop(&mut self) {
        let pid = self.child.id();
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_file(jet::DevServer::LiveInspect::snapshot_path(pid));
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn spawn_observed(tag: &str, source: &str) -> Option<Running> {
    if !common::have_rustc() {
        return None;
    }
    let dir = common::unique_tmp(&format!("jet_event_observe_{tag}"));
    fs::create_dir_all(&dir).unwrap();
    let source_path = dir.join("main.jet");
    fs::write(&source_path, source).unwrap();
    let build = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["build", source_path.to_str().unwrap()])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "build failed:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let binary = dir
        .join("build")
        .join(format!("main{}", std::env::consts::EXE_SUFFIX));
    let mut child = Command::new(binary)
        .env("JET_OBSERVE", "1")
        .env("JET_SCHEDULER_THREADS", "2")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut ready = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    if ready.trim() != "READY" {
        let stderr = child
            .stderr
            .take()
            .map(|stream| {
                let mut text = String::new();
                let _ = BufReader::new(stream).read_line(&mut text);
                text
            })
            .unwrap_or_default();
        panic!("observed program did not become ready: {ready:?} {stderr}");
    }
    Some(Running { child, dir })
}

fn debugger_events(pid: u32, terminal: &str) -> (String, String) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut last = String::new();
    loop {
        if let Ok(snapshot) = jet::DevServer::LiveInspect::read(pid) {
            if snapshot.contains(terminal) {
                let rendered = jet::Debug::render_event_observations(&snapshot)
                    .expect("debugger should consume the runtime event schema");
                return (snapshot, rendered);
            }
            last = snapshot;
        }
        if Instant::now() >= deadline {
            let rendered = jet::Debug::render_event_observations(&last).unwrap_or_default();
            panic!("runtime event observation never became visible:\n{rendered}");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn debugger_sees_only_executed_redacted_sync_and_hook_facts() {
    let Some(running) = spawn_observed(
        "sync_hook",
        r#"
use core.event as event
use core.time as time

fn run() {
    scope :: event.scope()
    executed :: event.new<String>()
    executed.on_priority(scope, 17, (secret: String) => {})
    never_called :: event.new<String>()
    if false { never_called.emit("UNEXECUTED_SECRET") }
    executed.emit("SYNC_PAYLOAD_SECRET")

    hook :: event.decision_hook<String, String>(HookPolicy.FirstCancelElseTransform)
    hook.on_priority(scope, 9, (secret: String) => HookDecision.Fail("HOOK_FAILURE_SECRET"))
    hook.run("HOOK_PAYLOAD_SECRET")
    print("READY")
    time.sleep(30000)
}
"#,
    ) else {
        return;
    };
    let (snapshot, rendered) = debugger_events(running.child.id(), "\"terminal\":\"Fail\"");
    for secret in [
        "UNEXECUTED_SECRET",
        "SYNC_PAYLOAD_SECRET",
        "HOOK_FAILURE_SECRET",
        "HOOK_PAYLOAD_SECRET",
    ] {
        assert!(!snapshot.contains(secret), "runtime snapshot leaked {secret}");
        assert!(!rendered.contains(secret), "debugger leaked {secret}");
    }
    assert!(rendered.contains("source=Event"));
    assert!(rendered.contains("priority=17"));
    assert!(rendered.contains("lifecycle=HandlerStarted"));
    assert!(rendered.contains("source=DecisionHook"));
    assert!(rendered.contains("priority=9"));
    assert!(rendered.contains("failure=Handler"));
    assert!(rendered.contains("terminal=Fail"));
    assert_eq!(rendered.matches("lifecycle=DispatchStarted").count(), 2);
}

#[test]
fn debugger_sees_pressure_drop_failure_close_and_single_terminals() {
    let Some(running) = spawn_observed(
        "async",
        r#"
use core.event as event
use core.tasks as tasks
use core.time as time

fn run() {
    print("READY")
    drop_scope :: event.scope()
    dropped :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 1, overflow: .DropNewest }, .Collect) ?? panic("policy")
    (started_tx, started_rx) :: tasks.channel<Int>()
    (release_tx, release_rx) :: tasks.channel<Int>()
    dropped.on_priority(drop_scope, 23, (n: Int) => {
        started_tx.send(~n)
        released :: release_rx.receive() ?? panic("release")
    })
    first :: dropped.emit_async(1)
    started :: started_rx.receive() ?? panic("started")
    second :: dropped.emit_async(2)
    newest :: dropped.emit_async(3)
    release_tx.send(1)
    started_second :: started_rx.receive() ?? panic("started second")
    release_tx.send(2)
    first.join()
    second.join()
    newest.join()

    fail_scope :: event.scope()
    failing :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 1, overflow: .Block }, .Collect) ?? panic("policy")
    failing.on(fail_scope, (n: Int) => Err("ASYNC_FAILURE_SECRET"))
    failing.emit_async(4).join()

    close_scope :: event.scope()
    closing :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 1, overflow: .Block }, .Collect) ?? panic("policy")
    (close_started_tx, close_started_rx) :: tasks.channel<Int>()
    (close_release_tx, close_release_rx) :: tasks.channel<Int>()
    closing.on(close_scope, (n: Int) => {
        close_started_tx.send(~n)
        released :: close_release_rx.receive() ?? panic("release")
    })
    close_running :: closing.emit_async(5)
    close_started :: close_started_rx.receive() ?? panic("started")
    close_queued :: closing.emit_async(6)
    close_blocked :: closing.emit_async(7)
    closing.close()
    close_scope.cancel()
    close_running.join()
    close_queued.join()
    close_blocked.join()

    time.sleep(30000)
}
"#,
    ) else {
        return;
    };
    let (snapshot, rendered) = debugger_events(running.child.id(), "\"terminal\":\"Closed\"");
    assert!(!snapshot.contains("ASYNC_FAILURE_SECRET"));
    assert!(!rendered.contains("ASYNC_FAILURE_SECRET"));
    assert!(rendered.contains("capacity=1 overflow=DropNewest"));
    assert!(rendered.contains("queued=1"));
    assert!(rendered.contains("blocked=1"));
    assert!(rendered.contains("priority=23"));
    assert!(rendered.contains("failure=Handler terminal=None"));
    assert!(rendered.contains("terminal=DroppedNewest"));
    assert!(rendered.contains("terminal=HandlerFailed"));
    assert!(rendered.contains("terminal=Closed"));
    assert!(rendered.contains("terminal=Cancelled"), "{rendered}");

    let dropped_line = rendered
        .lines()
        .find(|line| line.contains("terminal=DroppedNewest"))
        .expect("dropped dispatch terminal");
    let dispatch = dropped_line
        .split_whitespace()
        .find(|field| field.starts_with("dispatch="))
        .unwrap();
    assert_eq!(
        rendered
            .lines()
            .filter(|line| line.contains(dispatch) && line.contains("terminal=DroppedNewest"))
            .count(),
        1,
        "drop terminal must publish once"
    );
}

#[test]
fn runtime_event_sequence_is_bounded_and_debugger_preserves_exact_sequence() {
    let Some(running) = spawn_observed(
        "bounded",
        r#"
use core.event as event
use core.time as time

fn run() {
    scope :: event.scope()
    many :: event.new<Int>()
    many.on(scope, (n: Int) => {})
    loop i := 0; i < 300; i++ { many.emit(i) }
    print("READY")
    time.sleep(30000)
}
"#,
    ) else {
        return;
    };
    let (snapshot, rendered) = debugger_events(running.child.id(), "\"terminal\":\"Delivered\"");
    assert_eq!(snapshot.matches("\"sequence\":").count(), 256);
    assert_eq!(rendered.lines().count(), 256);
    let first = rendered.lines().next().unwrap();
    let last = rendered.lines().last().unwrap();
    assert!(!first.contains("sequence=1 "), "old records were not evicted: {first}");
    assert!(last.contains("terminal=Delivered"));
}
