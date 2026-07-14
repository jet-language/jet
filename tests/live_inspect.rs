//! D-OBSERVE-LIVE1=A executable proof: attach to a running generated program,
//! not a replay, fixture snapshot, or completed trace.

use std::fs;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

mod common;

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn live_inspector_reads_running_tasks_channels_and_resources_without_payloads() {
    if !common::have_rustc() {
        return;
    }
    let dir = common::unique_tmp("jet_live_inspect");
    fs::create_dir_all(&dir).unwrap();
    let source = dir.join("live.jet");
    fs::write(
        &source,
        r#"use core.tasks as tasks
use core.time as time

fn run() {
    (secret_sender, _secrets) :: tasks.channel<String>(1)
    secret_sender.send("TOP_SECRET_CHANNEL_PAYLOAD")
    (ready_sender, ready) :: tasks.channel<Int>()
    (_sender, blocked) :: tasks.channel<Int>(1)
    child :: tasks.spawn(take(ready_sender, blocked) () => {
        ready_sender.send(1)
        blocked.receive() ?? panic("closed")
    })
    child.detach()
    ready.receive() ?? panic("closed")
    time.sleep(30000)
}
"#,
    )
    .unwrap();

    let jet = env!("CARGO_BIN_EXE_jet");
    let build = Command::new(jet)
        .args(["build", source.to_str().unwrap()])
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
        .join(format!("live{}", std::env::consts::EXE_SUFFIX));
    let child = Command::new(&binary)
        .env("JET_OBSERVE", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    let guard = ChildGuard(child);

    let deadline = Instant::now() + Duration::from_secs(5);
    let snapshot = loop {
        match jet::DevServer::LiveInspect::read(pid) {
            Ok(snapshot)
                if snapshot.contains("\"wait\":\"channel receive\"")
                    && snapshot.contains("\"depth\":1,\"capacity\":1") =>
            {
                break snapshot;
            }
            _ if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            other => panic!("live scheduler snapshot never became observable: {other:?}"),
        }
    };
    assert!(snapshot.contains("\"parent\":1"));
    assert!(snapshot.contains("\"effects\":"));
    assert!(snapshot.contains("\"arena_bytes\":"));
    assert!(!snapshot.contains("TOP_SECRET_CHANNEL_PAYLOAD"));
    assert!(!snapshot.contains("payload"));
    assert!(!snapshot.contains("locals"));

    let cli = Command::new(jet)
        .args([
            "inspect",
            "live",
            "--attach",
            &pid.to_string(),
            "--once",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(cli.status.success(), "{}", String::from_utf8_lossy(&cli.stderr));
    let cli_snapshot = String::from_utf8(cli.stdout).unwrap();
    assert!(cli_snapshot.contains(&format!("\"pid\":{pid}")));
    assert!(cli_snapshot.contains("\"wait\":\"channel receive\""));
    assert!(!cli_snapshot.contains("TOP_SECRET_CHANNEL_PAYLOAD"));

    drop(guard);
    let _ = fs::remove_file(jet::DevServer::LiveInspect::snapshot_path(pid));
    let _ = fs::remove_dir_all(dir);
}
