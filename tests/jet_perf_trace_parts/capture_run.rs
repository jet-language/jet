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
    (ready_sender, ready) :: channel<Int>()
    (hold_sender, blocked) :: channel<Int>(1)
    child :: task {{
        ready_sender.send(1)
        time.sleep(700ms)
        blocked.receive() ?? panic("closed")
    }}
    child.detach()
    // Both waiters have finite completion paths. The channel waiter wakes after
    // the observation window; the accept waiter gets a real client after it.
    listener :: net.tcp_listen("127.0.0.1:{port}") ?? panic("bind")
    io_child :: task {{
        _ :: &listener.accept() ?? panic("accept")
    }}
    io_child.detach()
    ready.receive() ?? panic("closed")
    // hold_sender stays live so the blocked receive keeps real waiters.
    arena :: mem.Arena.new()
    x :: arena.alloc(42)
    probe_work()
    digest := crypto.sha256("trace-work".bytes())
    loop i in 0..16384 {{
        digest = crypto.sha256(digest.hex().bytes())
    }}
    print("READY {{digest.hex().len()}}")
    time.sleep(1200ms)
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

/// Clock, symbol, and source range of each profile row, in row order.
fn profile_row_identity(text: &str) -> (String, String, Vec<(String, String, [i64; 4])>) {
    use jet_foundation::JSON::{json_get, json_int, json_str, parse_json};
    let trace = parse_json(text).unwrap_or_else(|()| panic!("trace is not JSON: {text}"));
    let profile = json_get(&trace, "content")
        .and_then(|content| json_get(content, "profile"))
        .unwrap_or_else(|| panic!("missing profile: {text}"));
    let field = |key: &str| json_get(profile, key).and_then(json_str).unwrap().to_string();
    let jet_foundation::DataTree::DataTree::Array(rows) = json_get(profile, "rows").unwrap() else {
        panic!("profile rows are not an array: {text}");
    };
    let rows = rows
        .iter()
        .map(|row| {
            let source = json_get(row, "source").unwrap();
            let position = |key: &str| json_get(source, key).and_then(json_int).unwrap();
            (
                json_get(row, "clock").and_then(json_str).unwrap().to_string(),
                json_get(json_get(row, "symbol").unwrap(), "name")
                    .and_then(json_str)
                    .unwrap()
                    .to_string(),
                [
                    position("start_line"),
                    position("start_column"),
                    position("end_line"),
                    position("end_column"),
                ],
            )
        })
        .collect();
    (field("status"), field("window_status"), rows)
}

#[test]
fn perf_run_short_program_has_repeatable_source_attribution() {
    // A completed run publishes its exit snapshot before the unreaped child is
    // observed, so even a run far shorter than one poll is never `no_samples`.
    if !cfg!(any(target_os = "linux", target_os = "android")) {
        return;
    }
    let root = temp_workspace();
    // The comment spells `fn run`; only the parsed declaration is attributable.
    fs::write(
        root.join("short.jet"),
        "// Short program: `fn run` starts here.\nfn run() {\n    print(\"short\")\n}\n",
    )
    .unwrap();
    let mut identities = Vec::new();
    for attempt in 0..3 {
        let out = root.join(format!("short-{attempt}.jettrace"));
        let output = run_jet(&root, &["perf", "run", "short.jet", "--out", out.to_str().unwrap()]);
        assert!(
            output.status.success(),
            "perf run failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), "short\n");
        identities.push(profile_row_identity(&fs::read_to_string(&out).unwrap()));
    }
    let range = [2, 1, 4, 2];
    let expected = (
        "captured".to_string(),
        "exit".to_string(),
        vec![
            ("wall".to_string(), "run".to_string(), range),
            ("cpu".to_string(), "run".to_string(), range),
        ],
    );
    for identity in &identities {
        assert_eq!(identity, &expected);
    }
    let _ = fs::remove_dir_all(&root);
}
