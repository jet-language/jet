#[test]
fn perf_run_captures_wall_and_alloc_into_jettrace() {
    if !common::have_rustc() {
        return;
    }
    let root = temp_workspace();
    fs::create_dir_all(&root).unwrap();
    let source = root.join("session.jet");
    let port = unused_local_port();
    // Non-entry `probe_work` must appear in source_identity; sample symbol is
    // parsed `fn run` only — never a hardcoded invention. Arena stays live in
    // `run` so observe still sees outstanding allocations during sleep.
    // Child blocks on channel receive so poll sees contention + parent link.
    fs::write(
        &source,
        format!(r#"use core.crypto as crypto
use core.mem as mem
use core.net as net
use core.tasks as tasks
use core.time as time

fn probe_work() {{
    // Present only so source_identity must parse a non-entry function spelling.
}}

fn run() {{
    // Channel setup before arena so spawn panic cannot capture the arena view.
    (ready_sender, ready) :: tasks.channel<Int>()
    (hold_sender, blocked) :: tasks.channel<Int>(1)
    child :: task {{
        ready_sender.send(1)
        time.sleep(700)
        blocked.receive() ?? panic("closed")
    }}
    child.detach()
    // Both waiters have finite completion paths. The channel waiter wakes after
    // the observation window; the accept waiter gets a real client after it.
    listener :: net.tcp_listen("127.0.0.1:{port}") ?? panic("bind")
    io_child :: task {{
        _ :: listener.accept() ?? panic("accept")
    }}
    io_child.detach()
    ready.receive() ?? panic("closed")
    // hold_sender stays live so the blocked receive keeps real waiters.
    arena :: mem.Arena.new()
    x :: arena.alloc(42)
    probe_work()
    digest := crypto.sha256("trace-work".bytes())
    loop i, 0..16384 {{
        digest = crypto.sha256(digest.hex().bytes())
    }}
    print("READY {{digest.hex().len()}}")
    time.sleep(1200)
    hold_sender.send(1)
    _client :: net.tcp_connect("127.0.0.1:{port}") ?? panic("connect")
}}
"#,
        ),
    )
    .unwrap();

    let out = root.join("run-capture.jettrace");
    let output = run_jet(
        &root,
        &[
            "perf",
            "run",
            source.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "perf run failed: status={:?} stderr={stderr}",
        output.status.code()
    );
    assert!(stderr.contains("trace:"), "{stderr}");
    assert!(out.is_file(), "missing {}", out.display());
    let text = fs::read_to_string(&out).unwrap();
    assert_honest_wall_and_alloc(&text);
    assert_honest_tasks(&text);
    assert_honest_locks(&text);
    assert_honest_io(&text);
    assert_honest_native(&text, true, true);
    assert_honest_spans(&text, true);
    assert!(text.contains("\"name\":\"probe_work\""), "parsed fn missing: {text}");
    assert!(text.contains("\"name\":\"run\""), "{text}");
    assert!(text.contains("session.jet"), "{text}");
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
    assert!(stdout.contains("spans count="), "{stdout}");
    assert!(stdout.contains("window="), "{stdout}");
    assert!(stdout.contains("target="), "{stdout}");
    assert!(stdout.contains("session.jet#run"), "{stdout}");
    assert!(stdout.contains("command run"), "{stdout}");

    let _ = fs::remove_dir_all(&root);
}
