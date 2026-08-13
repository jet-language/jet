#[test]
fn perf_attach_captures_wall_and_alloc_with_jet_symbol_from_observe() {
    if !common::have_rustc() {
        return;
    }
    let root = temp_workspace();
    fs::create_dir_all(&root).unwrap();
    let source = root.join("live.jet");
    fs::write(
        &source,
        r#"use core.mem as mem
use core.net as net
use core.tasks as tasks
use core.time as time

fn probe_work() {
    // Present only so source_identity must parse a non-entry function spelling.
}
fn run() {
    // Channel setup before arena so spawn panic cannot capture the arena view.
    (ready_sender, ready) :: tasks.channel<Int>()
    (hold_sender, blocked) :: tasks.channel<Int>(1)
    child :: task {
        ready_sender.send(1)
        blocked.receive() ?? panic("closed")
    }
    child.detach()
    // Second child blocks on accept with no client — real observe I/O wait.
    listener :: net.tcp_listen("127.0.0.1:0") ?? panic("bind")
    io_child :: task {
        _ :: listener.accept() ?? panic("accept")
    }
    io_child.detach()
    ready.receive() ?? panic("closed")
    // hold_sender stays live so the blocked receive keeps real waiters.
    arena :: mem.Arena.new()
    x :: arena.alloc(42)
    probe_work()
    print("READY")
    time.sleep(30000)
}
"#,
    )
    .unwrap();

    let build = run_jet(&root, &["build", source.to_str().unwrap()]);
    assert!(
        build.status.success(),
        "build failed:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let binary = root
        .join("build")
        .join(format!("live{}", std::env::consts::EXE_SUFFIX));
    let mut child = Command::new(&binary)
        .env("JET_OBSERVE", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut ready = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    assert_eq!(
        ready.trim(),
        "READY",
        "observed program did not become ready: {ready:?}"
    );
    let pid = child.id();
    let _guard = ChildGuard(child);

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match jet::DevServer::LiveInspect::read(pid) {
            Ok(snapshot)
                if snapshot.contains("\"arena_allocations\":")
                    && !snapshot.contains("\"arena_allocations\":0")
                    && snapshot.contains("\"parent\":1")
                    && snapshot.contains("\"recv_waiters\":1")
                    && (snapshot.contains("\"wait\":\"tcp accept\"")
                        || snapshot.contains("\"wait\":\"tcp accept readiness\"")) =>
            {
                break
            }
            _ if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            other => panic!(
                "observe snapshot never became readable with allocs+child+contention+io: {other:?}"
            ),
        }
    }
    // Ensure /proc starttime → wall ticks clear the honest >0 / >=1ms floor.
    std::thread::sleep(Duration::from_millis(50));

    let out = root.join("capture.jettrace");
    let attach = run_jet(
        &root,
        &[
            "perf",
            "attach",
            &pid.to_string(),
            "--source",
            source.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ],
    );
    let stderr = String::from_utf8_lossy(&attach.stderr);
    assert!(
        attach.status.success(),
        "attach capture failed: status={:?} stderr={stderr}",
        attach.status.code()
    );
    assert!(out.is_file(), "missing {}", out.display());
    let text = fs::read_to_string(&out).unwrap();
    assert_honest_wall_and_alloc(&text);
    assert_honest_tasks(&text);
    assert_honest_locks(&text);
    assert_honest_io(&text);
    assert_honest_native(&text, false, false);
    assert_honest_spans(&text, false);
    assert!(text.contains("\"name\":\"probe_work\""), "parsed fn missing: {text}");
    assert!(text.contains("\"name\":\"run\""), "{text}");
    assert!(text.contains("live.jet"), "{text}");
    assert!(
        !text.contains("\"duration_ns\":1,"),
        "fabricated 1ns wall leaked into trace: {text}"
    );
    assert!(
        !text.contains("\"count\":0,"),
        "zero-alloc scrape-success leaked into trace: {text}"
    );

    let view = run_jet(&root, &["perf", "view", out.to_str().unwrap()]);
    let stdout = String::from_utf8_lossy(&view.stdout);
    assert!(view.status.success(), "{}", String::from_utf8_lossy(&view.stderr));
    assert!(stdout.contains("sample wall"), "{stdout}");
    assert!(stdout.contains("alloc count="), "{stdout}");
    assert!(!stdout.contains("alloc count=0 "), "zero alloc in view: {stdout}");
    assert!(stdout.contains("tasks count="), "{stdout}");
    assert!(stdout.contains("children="), "{stdout}");
    assert!(!stdout.contains("tasks count=0 "), "zero tasks in view: {stdout}");
    assert!(stdout.contains("locks count="), "{stdout}");
    assert!(stdout.contains("waiters="), "{stdout}");
    assert!(!stdout.contains("locks count=0 "), "zero locks in view: {stdout}");
    assert!(stdout.contains("io count="), "{stdout}");
    assert!(!stdout.contains("io count=0 "), "zero io in view: {stdout}");
    assert!(stdout.contains("native process_cpu="), "{stdout}");
    assert!(stdout.contains("spans unavailable reason="), "{stdout}");
    assert!(stdout.contains("live.jet#run"), "{stdout}");

    let _ = fs::remove_dir_all(&root);
}
