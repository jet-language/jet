#[test]
fn canonical_core_import_resolves() {
    let out = compile_temp(
        "core_imports.jet",
        r#"
use core.files as fs

fn run() {
    print(fs.exists("/tmp"))
}
"#,
    );
    assert!(out.rust.contains("jet_std_fs_exists"));
}

#[test]
fn importing_core_without_calls_keeps_optional_helpers_unemitted() {
    let out = compile_temp(
        "core_import_only.jet",
        r#"
use core.files as fs
use core.term as io
use core.sys as env
use core.process as process
use core.math as math
use core.math.random as random
use core.time as time
use core.encoding.json as json

fn run() {
}
"#,
    );
    assert!(!out.rust.contains("mod jet_std"));
    assert!(!out.rust.contains("jet_std_fs_read"));
    assert!(out.rust.contains("fn main()"));
}

#[test]
fn core_data_import_and_codegen_resolve() {
    let out = compile_temp(
        "core_data_import.jet",
        r#"
use core.data as data

#Codable
struct Ticket {
    team: String
    minutes: Float
}

fn run() {
    rows :: data.csv<Ticket>("team,minutes\nCore,4.0") ?? panic("bad csv")
    print(data.count(rows))
}
"#,
    );
    assert!(out.rust.contains("jet_enc_csv_decode"));
    assert!(
        out.rust.contains("jet_enc_csv_decode::<__jet_Ticket>"),
        "core.data.csv must lower its sema-owned list element type exactly:\n{}",
        out.rust
    );
    assert!(
        !out.rust.contains("jet_enc_csv_decode::<Vec<__jet_Ticket>>"),
        "core.data.csv nested its list result at the runtime boundary:\n{}",
        out.rust
    );
    assert!(out.rust.contains("jet_data_count"));
}

#[test]
fn core_files_depth_example_runs() {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }
    let out = Command::new(&jet)
        .args(["run", "examples/features/io/files_depth.jet"])
        .output()
        .expect("run files_depth");
    assert!(
        out.status.success(),
        "files_depth failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let expected = fs::read_to_string("examples/features/expected/io/files_depth.out").unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), expected);
}

#[test]
fn core_watcher_example_runs() {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }
    let out = Command::new(&jet)
        .args(["run", "examples/features/io/watcher.jet"])
        .output()
        .expect("run watcher");
    assert!(
        out.status.success(),
        "watcher failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let expected = fs::read_to_string("examples/features/expected/io/watcher.out").unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), expected);
}

#[cfg(unix)]
#[test]
fn core_process_builder_pipeline_and_spawn_run() {
    let dir = std::env::temp_dir().join(format!("jet_corelib_process_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let work = dir.join("work");
    fs::create_dir_all(&work).unwrap();

    let probe = dir.join("probe.sh");
    let emit = dir.join("emit.sh");
    let cat = dir.join("cat.sh");
    let lines = dir.join("lines.sh");
    write_executable(
        &probe,
        "#!/bin/sh\nprintf 'env=%s\\n' \"$JET_PROCESS_TEST\"\nprintf 'cwd=%s\\n' \"$(pwd)\"\nread line\nprintf 'stdin=%s\\n' \"$line\"\n",
    );
    write_executable(&emit, "#!/bin/sh\nprintf 'pipe-ok\\n'\n");
    write_executable(&cat, "#!/bin/sh\ncat\n");
    write_executable(&lines, "#!/bin/sh\nprintf 'line-one\\nline-two\\n'\n");

    let src = format!(
        r#"
use core.process as process
use core.time as time

fn run() {{
    timeout :: Duration.seconds(2) ?? panic("duration")
    spec :: process.cmd(["{probe}"]).cwd("{work}").env_clear().env("JET_PROCESS_TEST", "ok").stdin(.Capture).stdout(.Capture).stderr(.Capture).timeout(timeout).output_limit(10000)
    probe_child :: spec.spawn() ?? panic("spawn failed")
    probe_child.stdin.write("from-stdin\n") ?? panic("write failed")
    result :: probe_child.wait() ?? panic("wait failed")
    print(result.success)
    print(result.code)
    print(result.timed_out)
    print(result.output)

    piped :: process.pipeline([process.cmd(["{emit}"]), process.cmd(["{cat}"])]) ?? panic("pipeline failed")
    print(piped.success)
    print(piped.output)

    child :: process.cmd(["{lines}"]).stdout(.Stream).spawn() ?? panic("spawn failed")
    loop line in child.stdout.lines() {{
        print(line)
    }}
    waited :: child.wait() ?? panic("wait failed")
    print(waited.success)
}}
"#,
        probe = jet_string_path(&probe),
        work = jet_string_path(&work),
        emit = jet_string_path(&emit),
        cat = jet_string_path(&cat),
        lines = jet_string_path(&lines)
    );

    let (code, stdout, stderr) = build_and_run(&dir, "process_api", &src, &[], None);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert!(stdout.contains("true\n0\nfalse\n"), "{stdout}");
    assert!(stdout.contains("env=ok\n"), "{stdout}");
    assert!(
        stdout.contains(&format!("cwd={}\n", work.display())),
        "{stdout}"
    );
    assert!(stdout.contains("stdin=from-stdin\n"), "{stdout}");
    assert!(stdout.contains("pipe-ok\n"), "{stdout}");
    assert!(stdout.contains("line-one\n"), "{stdout}");
}

#[cfg(unix)]
#[test]
fn core_process_pipeline_honors_final_redirection_modes() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_process_pipeline_redirect_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let first = dir.join("first.sh");
    let last = dir.join("last.sh");
    write_executable(
        &first,
        "#!/bin/sh\nprintf 'edge\\n'\nprintf 'first-err\\n' >&2\n",
    );
    write_executable(
        &last,
        "#!/bin/sh\ncat\nprintf 'final-err\\n' >&2\n",
    );
    let src = format!(
        r#"
use core.process as process

fn run() {{
    result :: process.pipeline([
        process.cmd(["{first}"]),
        process.cmd(["{last}"]).stdout(.Inherit).stderr(.Inherit)
    ]) ?? panic("pipeline failed")
    print(result.success)
    print(result.output)
    print(result.errors)
}}
"#,
        first = jet_string_path(&first),
        last = jet_string_path(&last),
    );
    let (code, stdout, stderr) =
        build_and_run(&dir, "process_pipeline_redirect", &src, &[], None);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "edge\ntrue\n\nfirst-err\n\n");
    assert_eq!(stderr, "final-err\n");
}

#[cfg(unix)]
#[test]
fn core_process_pipeline_honors_stage_timeout() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_process_pipeline_timeout_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let slow = dir.join("slow.sh");
    write_executable(&slow, "#!/bin/sh\nsleep 1\n");
    let src = format!(
        r#"
use core.process as process

fn run() {{
    timeout :: Duration.milliseconds(100) ?? panic("duration failed")
    result :: process.pipeline([
        process.cmd(["{slow}"]).timeout(timeout),
        process.cmd(["cat"])
    ]) ?? panic("pipeline failed")
    print(result.timed_out)
}}
"#,
        slow = jet_string_path(&slow),
    );
    let (code, stdout, stderr) = build_and_run(&dir, "process_pipeline_timeout", &src, &[], None);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "true\n");

    let started = std::time::Instant::now();
    let rerun = Command::new(dir.join("process_pipeline_timeout"))
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        started.elapsed() < std::time::Duration::from_millis(800),
        "pipeline ignored stage timeout: {:?}",
        started.elapsed()
    );
    assert_eq!(rerun.status.code(), Some(0));
    assert_eq!(rerun.stdout, b"true\n");
    let _ = fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn core_process_live_stream_does_not_block_on_sibling_output() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_process_live_backpressure_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let slow = dir.join("slow.sh");
    write_executable(
        &slow,
        "#!/bin/sh\n\
dd if=/dev/zero bs=131072 count=1 1>&2 2>/dev/null &\n\
writer=$!\n\
(sleep 2; kill -9 \"$writer\" 2>/dev/null) >/dev/null 2>&1 &\n\
killer=$!\n\
wait \"$writer\"\n\
kill \"$killer\" 2>/dev/null || true\n\
printf 'done\\n'\n",
    );
    let src = format!(
        r#"
use core.process as process

fn run() {{
    child :: process.cmd(["{slow}"]).stdout(.Stream).stderr(.Stream).spawn() ?? panic("spawn failed")
    loop line in child.stdout.lines() {{
        print(line)
    }}
    child.wait() ?? panic("wait failed")
    print("finished")
}}
"#,
        slow = jet_string_path(&slow),
    );
    let (code, stdout, stderr) =
        build_and_run(&dir, "process_live_backpressure", &src, &[], None);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "done\nfinished\n");
    assert!(stderr.is_empty(), "unexpected child stderr: {stderr:?}");

    let started = std::time::Instant::now();
    let rerun = Command::new(dir.join("process_live_backpressure"))
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        started.elapsed() < std::time::Duration::from_millis(1500),
        "live stream waited for the sibling pipe instead of draining it: {:?}",
        started.elapsed()
    );
    assert_eq!(rerun.status.code(), Some(0));
    assert_eq!(rerun.stdout, b"done\nfinished\n");
    assert!(
        rerun.stderr.is_empty(),
        "unexpected child stderr: {:?}",
        rerun.stderr
    );
}

#[cfg(unix)]
#[test]
fn core_process_limits_kill_descendants_and_stop_output_early() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_process_limits_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let timeout_pid = dir.join("timeout.pid");
    let output_pid = dir.join("output.pid");
    let timeout_script = dir.join("timeout.sh");
    let output_script = dir.join("output.sh");
    write_executable(
        &timeout_script,
        &format!(
            "#!/bin/sh\nsleep 30 &\nprintf '%s\\n' \"$!\" > '{}'\nwait\n",
            timeout_pid.display()
        ),
    );
    write_executable(
        &output_script,
        &format!(
            "#!/bin/sh\nsleep 2 &\nprintf '%s\\n' \"$!\" > '{}'\nprintf '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'\nwait\n",
            output_pid.display()
        ),
    );
    let src = format!(
        r#"
use core.process as process
use core.time as time

fn run() {{
    timeout :: Duration.seconds(1) ?? panic("duration")
    timed :: process.cmd(["{timeout_script}"]).timeout(timeout).run() ?? panic("timeout run failed")
    print(timed.timed_out)
    limited :: process.cmd(["{output_script}"]).output_limit(16).run()
    if limited == {{
        .Ok(_) -> {{ print("limit:accepted") }}
        .Err(_) -> {{ print("limit:refused") }}
    }}
}}
"#,
        timeout_script = jet_string_path(&timeout_script),
        output_script = jet_string_path(&output_script),
    );
    // Compile once, then time only the already-built production binary. Measuring
    // `build_and_run` here also measures rustc and makes the enforcement check
    // fail on a cold cache before the child even starts.
    let (code, stdout, stderr) = build_and_run(&dir, "process_limits", &src, &[], None);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert!(stdout.contains("true\nlimit:refused\n"), "{stdout}");
    let started = std::time::Instant::now();
    let rerun = Command::new(dir.join("process_limits"))
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "output limit waited for the child instead of terminating it: {:?}",
        started.elapsed()
    );
    assert_eq!(rerun.status.code(), Some(0));
    assert_eq!(rerun.stdout, b"true\nlimit:refused\n");

    for pid_file in [&timeout_pid, &output_pid] {
        let pid = fs::read_to_string(pid_file).unwrap();
        let pid = pid.trim();
        let alive = Command::new("kill")
            .args(["-0", pid])
            .status()
            .unwrap()
            .success();
        if alive {
            let _ = Command::new("kill").args(["-9", pid]).status();
        }
        assert!(!alive, "process descendant {pid} survived enforcement");
    }
}

#[cfg(unix)]
#[test]
fn core_process_streams_bound_both_pipes_and_close_stdin_after_wait() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_process_streams_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let flood = dir.join("flood.sh");
    write_executable(
        &flood,
        "#!/bin/sh\n\
dd if=/dev/zero bs=1048576 count=2 2>/dev/null | tr '\\000' x &\n\
out=$!\n\
dd if=/dev/zero bs=1048576 count=2 2>/dev/null | tr '\\000' y >&2 &\n\
err=$!\n\
wait \"$out\"\n\
wait \"$err\"\n",
    );
    let src = format!(
        r#"
use core.process as process

fn run() {{
    limited :: process.cmd(["{flood}"]).output_limit(1024).run()
    if limited == {{
        .Ok(_) -> {{ print("limit:accepted") }}
        .Err(_) -> {{ print("limit:refused") }}
    }}
    child :: process.cmd(["sh", "-c", "exit 0"]).stdin(.Capture).spawn() ?? panic("spawn failed")
    child.wait() ?? panic("wait failed")
    if child.stdin.write("late") == {{
        .Ok(_) -> {{ print("closed:accepted") }}
        .Err(_) -> {{ print("closed:typed-error") }}
    }}
}}
"#,
        flood = jet_string_path(&flood),
    );
    let (code, stdout, stderr) = build_and_run(&dir, "process_streams", &src, &[], None);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "limit:refused\nclosed:typed-error\n");
}

/// D-PROCESS-SESSION1=A (#1181): `.terminal()` is the one opt-in for a
/// terminal-backed session, and it lives on the same `ProcessSpec`. Argv
/// execution with no terminal stays the default. Unix run/spawn use a real PTY;
/// pipeline stages reject terminal specs rather than coercing them to pipes.
#[cfg(unix)]
#[test]
fn core_process_terminal_uses_unix_pty_for_run_and_spawn() {
    let dir = std::env::temp_dir().join(format!("jet_core_process_terminal_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.process as process

fn run() {
    plain :: process.cmd(["echo", "plain-ok"]).stdout(.Capture).run() ?? panic("default run failed")
    print(plain.output.trim())

    run_result :: process.cmd(["printf", "run-ok"]).terminal().run() ?? panic("terminal run failed")
    print(run_result.output.contains("run-ok"))

    child :: process.cmd(["printf", "spawn-ok"]).terminal().spawn() ?? panic("terminal spawn failed")
    if child.terminal == {
        .Val(session) -> {
            session.resize(TerminalSize{ cols: 100, rows: 30 }) ?? panic("resize failed")
            print("spawn: session")
        }
        .None -> { print("spawn: no session") }
    }
    waited :: child.wait() ?? panic("terminal wait failed")
    print(waited.output.contains("spawn-ok"))

    if process.pipeline([process.cmd(["echo", "a"]), process.cmd(["cat"]).terminal()]) == {
        .Ok(_) -> { print("pipeline: accepted") }
        .Err(_) -> { print("pipeline: refused") }
    }
    if process.cmd([]).terminal().run() == {
        .Ok(_) -> { print("empty: accepted") }
        .Err(e) -> {
            if e == {
                .InvalidInput(_) -> { print("empty: invalid") }
                else -> { print("empty: wrong error") }
            }
        }
    }
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "process_terminal", src, &[], None);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert!(stdout.starts_with("plain-ok\n"), "{stdout}");
    assert!(stdout.contains("true\nspawn: session\ntrue\npipeline: refused\nempty: invalid\n"), "{stdout}");
    // The production path carries the native PTY primitive into the emitted
    // program; this guards against a test-only or pipe fallback.
    let compiled = compile_temp("process_terminal_text.jet", src);
    assert!(
        compiled.rust.contains("posix_openpt"),
        "the Unix terminal path must include the native PTY backend"
    );
}

/// D-PROCESS-SESSION1=A (#1181): the Unix backend is a real bidirectional
/// terminal, not a pair of pipes. Prove tty identity, input, ANSI bytes, and
/// the requested window size through the shipped ProcessSpec path.
#[cfg(unix)]
#[test]
fn core_process_terminal_is_tty_bidirectional_and_preserves_control_bytes() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_process_terminal_bytes_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let probe = dir.join("terminal_probe.sh");
    write_executable(
        &probe,
        "#!/bin/sh
if [ ! -t 0 ] || [ ! -t 1 ]; then exit 41; fi
printf 'ready\\n'
IFS= read -r line
printf 'input:%s\\n' \"$line\"
printf '\\033[31mcontrol\\033[0m\\n'
stty size
",
    );
    let src = format!(
        r#"
use core.process as process

fn run() {{
    policy :: TerminalPolicy{{
        size: TerminalSize{{ cols: 100, rows: 30 }},
        mode: .Raw
    }}
    child :: process.cmd(["{probe}"]).terminal(policy).spawn() ?? panic("terminal spawn failed")
    session :: child.terminal ?? panic("missing terminal session")
    session.resize(TerminalSize{{ cols: 100, rows: 30 }}) ?? panic("resize failed")
    child.stdin.write("typed\n") ?? panic("terminal write failed")
    result :: child.wait() ?? panic("terminal wait failed")
    print(result.success)
    print(result.output)
    if child.stdin.write("late") == {{
        .Ok(_) -> {{ print("closed:accepted") }}
        .Err(error) -> {{
            if error == {{
                .Closed(_) -> {{ print("closed:typed-error") }}
                else -> {{ print("closed:wrong-error") }}
            }}
        }}
    }}
}}
"#,
        probe = jet_string_path(&probe)
    );
    let (code, stdout, stderr) = build_and_run(&dir, "process_terminal_bytes", &src, &[], None);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert!(stdout.starts_with("true\nready\ninput:typed\n"), "{stdout:?}");
    assert!(stdout.contains("\x1b[31mcontrol\x1b[0m"), "ANSI bytes lost: {stdout:?}");
    assert!(stdout.contains("30 100"), "resize lost: {stdout:?}");
    assert!(stdout.ends_with("closed:typed-error\n"), "{stdout:?}");
}

/// D-PROCESS-SESSION1=A (#1181): terminal lifecycle controls target the Unix
/// session process group. Each child leaves a descendant PID behind so the
/// host test can prove interrupt, terminate, kill, timeout, and drop reap it.
#[cfg(unix)]
#[test]
fn core_process_terminal_controls_reap_the_full_process_tree() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_process_terminal_tree_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let script = dir.join("tree.sh");
    let interrupt_pid = dir.join("interrupt.pid");
    let terminate_pid = dir.join("terminate.pid");
    let kill_pid = dir.join("kill.pid");
    let timeout_pid = dir.join("timeout.pid");
    let drop_pid = dir.join("drop.pid");
    write_executable(
        &script,
        "#!/bin/sh
sleep 30 &
printf '%s\\n' \"$!\" > \"$1\"
trap 'exit 130' INT
trap 'exit 143' TERM
while :; do sleep 1; done
",
    );
    let src = format!(
        r#"
use core.process as process
use core.time as time

fn dropped(path: String) {{
    child :: process.cmd(["{script}", path]).terminal().spawn() ?? panic("drop spawn failed")
    time.sleep(100ms)
}}

fn run() {{
    interrupt :: process.cmd(["{script}", "{interrupt_pid}"]).terminal().spawn() ?? panic("interrupt spawn failed")
    time.sleep(100ms)
    interrupt.interrupt() ?? panic("interrupt failed")
    interrupt_result :: interrupt.wait() ?? panic("interrupt wait failed")
    print(interrupt_result.success)

    terminate :: process.cmd(["{script}", "{terminate_pid}"]).terminal().spawn() ?? panic("terminate spawn failed")
    time.sleep(100ms)
    terminate.terminate() ?? panic("terminate failed")
    terminate_result :: terminate.wait() ?? panic("terminate wait failed")
    print(terminate_result.success)

    kill :: process.cmd(["{script}", "{kill_pid}"]).terminal().spawn() ?? panic("kill spawn failed")
    time.sleep(100ms)
    kill.kill() ?? panic("kill failed")
    kill_result :: kill.wait() ?? panic("kill wait failed")
    print(kill_result.success)

    timeout :: Duration.milliseconds(100) ?? panic("timeout duration failed")
    timed :: process.cmd(["{script}", "{timeout_pid}"]).terminal().timeout(timeout).run() ?? panic("timeout run failed")
    print(timed.timed_out)

    dropped("{drop_pid}")
    print("drop returned")
}}
"#,
        script = jet_string_path(&script),
        interrupt_pid = jet_string_path(&interrupt_pid),
        terminate_pid = jet_string_path(&terminate_pid),
        kill_pid = jet_string_path(&kill_pid),
        timeout_pid = jet_string_path(&timeout_pid),
        drop_pid = jet_string_path(&drop_pid),
    );
    let (code, stdout, stderr) = build_and_run(&dir, "process_terminal_tree", &src, &[], None);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "false\nfalse\nfalse\ntrue\ndrop returned\n");

    for pid_file in [
        interrupt_pid,
        terminate_pid,
        kill_pid,
        timeout_pid,
        drop_pid,
    ] {
        let pid = fs::read_to_string(&pid_file).unwrap();
        let pid = pid.trim();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        loop {
            let alive = Command::new("kill")
                .args(["-0", pid])
                .status()
                .unwrap()
                .success();
            if !alive {
                break;
            }
            if std::time::Instant::now() >= deadline {
                let _ = Command::new("kill").args(["-9", pid]).status();
                panic!("terminal descendant {pid} survived {:?}", pid_file);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

/// Process wait is a cancellation wait point. Cancelling a task that owns a
/// live child must unwind the wait cleanup and terminate the child group,
/// including a deliberately forked descendant.
#[cfg(unix)]
#[test]
fn core_process_wait_cancellation_reaps_the_full_process_tree() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_process_cancel_tree_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let script = dir.join("cancel-tree.sh");
    let descendant_pid = dir.join("descendant.pid");
    write_executable(
        &script,
        &format!(
            "#!/bin/sh\nsleep 30 &\nprintf '%s\\n' \"$!\" > '{}'\nwhile :; do sleep 1; done\n",
            descendant_pid.display()
        ),
    );
    let src = format!(
        r#"
use core.process as process
use core.tasks as tasks
use core.time as time

fn run() {{
    (ready_tx, ready_rx) :: channel<Int>()
    worker :: task {{
        child :: process.cmd(["{script}"]).stdout(.Capture).stderr(.Capture).spawn() ?? panic("spawn failed")
        ready_tx.send(1)
        child.wait() ?? panic("cancelled wait returned")
    }}
    _ready :: ready_rx.receive() ?? panic("ready failed")
    time.sleep(100ms)
    worker.cancel()
    result :: worker.join()
    if result == {{
        .Err(error) -> {{
            if error == {{
                .Cancelled -> {{ print("cancelled") }}
                else -> {{ print("wrong cancellation") }}
            }}
        }}
        .Ok(_) -> {{ print("completed") }}
    }}
}}
"#,
        script = jet_string_path(&script),
    );
    let (code, stdout, stderr) = build_and_run(&dir, "process_cancel_tree", &src, &[], None);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "cancelled\n");

    let pid = fs::read_to_string(&descendant_pid).unwrap();
    let pid = pid.trim();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    loop {
        let alive = Command::new("kill")
            .args(["-0", pid])
            .status()
            .unwrap()
            .success();
        if !alive {
            break;
        }
        if std::time::Instant::now() >= deadline {
            let _ = Command::new("kill").args(["-9", pid]).status();
            panic!("cancelled process descendant {pid} survived task cleanup");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// D-PROCESS-SESSION1=A / D-PROCESS-SESSION2=D (#1181): the beginner and expert
/// forms share one ProcessSpec. Stable host facts advertise the Unix PTY and a
/// policy controls the initial terminal size and mode.
#[cfg(unix)]
#[test]
fn core_process_terminal_policy_and_capabilities_are_typed_and_resizable() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_process_terminal_policy_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.process as process

fn run() {
    policy :: TerminalPolicy{
        size: TerminalSize{ cols: 120, rows: 40 },
        mode: .Raw
    }
    plan :: process.cmd(["echo", "hi"]).terminal(policy)
    facts :: plan.abilities()
    print(facts.has(TerminalFact.terminal))
    print(facts.has(TerminalFact.resize))
    print(facts.has(TerminalFact.raw))
    print(facts.has("preview_x"))
    if plan.run() == {
        .Ok(_) -> { print("terminal:ok") }
        .Err(_) -> { print("terminal:unavailable") }
    }
    child :: process.cmd(["echo", "plain"]).stdout(.Capture).spawn() ?? panic("spawn failed")
    if child.terminal == {
        .Val(session) -> {
            session.resize(TerminalSize{ cols: 80, rows: 24 }) ?? panic("resize failed")
            print("plain child unexpectedly has terminal")
        }
        .None -> { print("plain child has no terminal") }
    }
    waited :: child.wait() ?? panic("wait failed")
    print(waited.output.trim())
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "process_terminal_policy", src, &[], None);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(
        stdout,
        "true\ntrue\ntrue\nfalse\nterminal:ok\nplain child has no terminal\nplain\n"
    );

    let typo = jet::compile(
        r#"use core.process as process
fn run() {
    facts :: process.cmd(["echo", "x"]).abilities()
    print(facts.has(TerminalFact.reszie))
}
"#,
    )
    .expect_err("stable fact typos must fail in sema");
    assert!(
        typo.iter().any(|diag| {
            diag.code == "E0302"
                && diag.what.contains("`TerminalFact` has no key `reszie`")
                && diag.fix.contains("`TerminalFact.resize`")
        }),
        "{typo:?}"
    );

    let preview_typo = jet::compile(
        r#"use core.process as process
fn run() {
    facts :: process.cmd(["echo", "x"]).abilities()
    print(facts.has("reszie"))
}
"#,
    )
    .expect_err("close preview-string typos must suggest the stable fact");
    assert!(
        preview_typo.iter().any(|diag| {
            diag.code == "E0302"
                && diag.what.contains("`reszie` looks like `resize`")
                && diag.fix.contains("`TerminalFact.resize`")
        }),
        "{preview_typo:?}"
    );

    let plain_child_terminal = jet::compile(
        r#"use core.process as process
fn run() {
    child :: process.cmd(["echo", "plain"]).spawn() ?? panic("spawn failed")
    child.terminal.resize(TerminalSize{ cols: 80, rows: 24 })
}
"#,
    )
    .expect_err("a plain child must not expose a TerminalSession");
    assert!(
        plain_child_terminal
            .iter()
            .any(|diag| {
                diag.code == "E0311"
                    && diag.what
                        == "`.resize()` needs `TerminalSession`, not `TerminalSession?`"
                    && diag.fix.contains("session.resize(size)")
            }),
        "{plain_child_terminal:?}"
    );
}

/// D-PROCESS-SESSION1=A / #1182: the Windows terminal path must create a real
/// ConPTY, preserve the combined byte stream, resize the console, and release
/// the pseudoconsole before `wait()` joins its output reader.
#[cfg(windows)]
#[test]
fn core_process_terminal_uses_windows_conpty_for_bytes_resize_and_close() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_process_terminal_conpty_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let script = dir.join("conpty_probe.cmd");
    fs::write(
        &script,
        format!(
            "@echo off\r\nmode con >nul 2>nul || exit /b 41\r\necho ready\r\nset /p line=\r\necho input:%line%\r\necho {red}control{reset}\r\nmode con\r\n",
            red = "\x1b[31m",
            reset = "\x1b[0m"
        ),
    )
    .unwrap();
    let src = format!(
        r#"
use core.process as process

fn run() {{
    policy :: TerminalPolicy{{
        size: TerminalSize{{ cols: 80, rows: 24 }},
        mode: .Cooked
    }}
    child :: process.cmd(["cmd.exe", "/D", "/Q", "/C", "{script}"])
        .cwd("{dir}")
        .terminal(policy)
        .spawn() ?? panic("ConPTY spawn failed")
    session :: child.terminal ?? panic("missing ConPTY session")
    session.resize(TerminalSize{{ cols: 100, rows: 30 }}) ?? panic("ConPTY resize failed")
    child.stdin.write("typed\r\n") ?? panic("ConPTY input failed")
    result :: child.wait() ?? panic("ConPTY wait failed")
    print(result.success)
    print(result.output)
    if child.stdin.write("late") == {{
        .Ok(_) -> {{ print("closed:accepted") }}
        .Err(error) -> {{
            if error == {{
                .Closed(_) -> {{ print("closed:typed") }}
                else -> {{ print("closed:wrong") }}
            }}
        }}
    }}
}}
"#,
        script = jet_string_path(&script),
        dir = jet_string_path(&dir),
    );
    let (code, stdout, stderr) = build_and_run(&dir, "process_terminal_conpty", &src, &[], None);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    let output = stdout.replace("\r\n", "\n");
    assert!(output.starts_with("true\n"), "{stdout:?}");
    assert!(output.contains("ready\n"), "ready marker lost: {stdout:?}");
    assert!(output.contains("input:typed\n"), "input bytes lost: {stdout:?}");
    assert!(output.contains("\x1b[31mcontrol\x1b[0m"), "VT bytes lost: {stdout:?}");
    assert!(output.contains("100") && output.contains("30"), "resize lost: {stdout:?}");
    assert!(output.ends_with("closed:typed\n"), "closed stdin was not typed: {stdout:?}");

    let compiled = compile_temp("process_terminal_conpty_text.jet", &src);
    assert!(compiled.rust.contains("CreatePseudoConsole"));
    assert!(compiled.rust.contains("PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE"));
    assert!(compiled.rust.contains("let native = jet_process_pty::spawn("));
    assert!(compiled.rust.contains("jet_process_pty::resize_console("));
    assert!(compiled.rust.contains("jet_std::ProcessHandle::Native"));
    assert!(compiled.rust.contains("TerminateJobObject"));
    assert!(compiled.rust.contains("ClosePseudoConsole"));
    assert!(compiled.rust.contains("input.write_all(&[3])"));
    assert!(!compiled.rust.contains("GenerateConsoleCtrlEvent"));
}

#[cfg(unix)]
#[test]
fn core_process_sh_typed_text_keeps_each_hole_one_argv_item() {
    let dir = std::env::temp_dir().join(format!("jet_core_process_sh_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "process_sh_typed_text",
        r#"
use core.process as process

fn run() {
    hostile :: "two words;*.jet"
    expected :: Sh{"printf <%s> {hostile}"}
    first :: process.run(expected) ?? panic("typed-head command failed")
    print(first.output)

    second :: process.run(Sh{"printf [%s] {hostile}"}) ?? panic("second typed-head failed")
    print(second.output)

    audited :: Sh.raw("printf raw")
    third :: process.run(audited) ?? panic("raw command failed")
    print(third.output)
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "<two words;*.jet>\n[two words;*.jet]\nraw\n");
}

#[test]
fn core_time_calendar_zone_and_dst_run() {
    let source_zone = std::env::var_os("TZDIR")
        .map(|dir| PathBuf::from(dir).join("America/New_York"))
        .filter(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from("/usr/share/zoneinfo/America/New_York"));
    if !source_zone.exists() {
        return;
    }
    let dir =
        std::env::temp_dir().join(format!("jet_corelib_time_calendar_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let tzdb = dir.join("tzdb");
    fs::create_dir_all(tzdb.join("America")).unwrap();
    fs::copy(&source_zone, tzdb.join("America/New_York")).unwrap();
    let src = r#"
use core.time as time
use core.time as date

fn run() {
    zone :: time.zone("America/New_York") ?? panic("missing zone")
    local :: time.zoned_local(date.new(2024, 3, 10), time.time(1, 30, 0), zone)
    print(local.format("yyyy-MM-dd HH:mm:ss VV XXX"))
    civil :: local.add_period(time.period_days(1))
    day :: Duration.hours(24) ?? panic("duration")
    absolute :: local.add_duration(day)
    print(civil.format("yyyy-MM-dd HH:mm:ss VV XXX"))
    print(absolute.format("yyyy-MM-dd HH:mm:ss VV XXX"))
    print(local.to_datetime().format_rfc3339())
    parsed :: time.parse_rfc3339("2024-03-10T06:30:00Z") ?? panic("bad parse")
    print(parsed.in_zone(zone).format("yyyy-MM-dd HH:mm:ss VV XXX"))
}
"#;
    let tzdb_env = tzdb.to_string_lossy().into_owned();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "time_calendar",
        src,
        &[("JET_TZDB_DIR", &tzdb_env)],
        None,
    );
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(
        stdout,
        "2024-03-10 01:30:00 America/New_York -05:00\n2024-03-11 01:30:00 America/New_York -04:00\n2024-03-11 02:30:00 America/New_York -04:00\n2024-03-10T06:30:00Z\n2024-03-10 01:30:00 America/New_York -05:00\n"
    );

    // I9: default `jet run` must return the same ZonedDateTime the full build
    // does, not a flattened carrier. #2030 — the resident-JIT civil-time host
    // had no arm for a zoned `add_duration`/`add_period` receiver, so dispatch
    // fell through to a null handle and `format`/`to_string` rendered an
    // unrelated heap slot (the source file path) with offset 0 and is_dst false.
    let jet = jet_bin();
    if jet.exists() {
        let quick_path = dir.join("time_calendar.jet");
        fs::write(&quick_path, src).unwrap();
        let quick = Command::new(&jet)
            .arg("run")
            .arg(&quick_path)
            .current_dir(&dir)
            .env("JET_TZDB_DIR", &tzdb_env)
            .output()
            .unwrap();
        assert!(
            quick.status.success(),
            "default `jet run` failed:\n{}",
            String::from_utf8_lossy(&quick.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&quick.stdout),
            stdout,
            "zoned arithmetic must mean the same thing on the default tier and AOT (I9)"
        );
    }
}

#[test]
fn core_url_mime_parse_join_query_and_http_url_run() {
    let dir = std::env::temp_dir().join(format!("jet_corelib_url_mime_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.net.mime as mime
use core.net.url as url

fn run() {
    base :: url.parse("https://Bücher.example:443/a/./b/../c?x=1#frag") ?? panic("bad url")
    print(base.to_string())
    print(base.host() ?? "none")
    print(base.path())
    print(base.query_pairs()[0][0])
    print(base.query_pairs()[0][1])
    rel :: base.join("../notify?user=ada lovelace&user=grace#done") ?? panic("bad join")
    print(rel.to_string())
    print(rel.path_segments().join("|"))
    print(rel.fragment() ?? "none")
    print(url.query([["user", "ada lovelace"], ["user", "grace"], ["empty", ""]]))
    print(url.percent_encode("a b/c"))
    print(url.percent_decode("a%20b%2Fc") ?? "bad")
    html :: mime.parse("Text/HTML; charset=UTF-8") ?? panic("bad mime")
    print(html.essence())
    print(html.param("charset") ?? "none")
    print(mime.from_extension("png") ?? "none")
    print(mime.extension("image/png") ?? "none")
    png :: mime.parse("image/png") ?? panic("bad mime")
    print(url.data(png, "<h1>Hi</h1>").to_string())
    print(url.file("/tmp/a b.txt").to_string())
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "url_mime", src, &[], None);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(
        stdout,
        "https://xn--bcher-kva.example:443/a/c?x=1#frag\nxn--bcher-kva.example\n/a/c\nx\n1\nhttps://xn--bcher-kva.example:443/notify?user=ada%20lovelace&user=grace#done\nnotify\ndone\nuser=ada%20lovelace&user=grace&empty=\na%20b%2Fc\na b/c\ntext/html\nUTF-8\nimage/png\npng\ndata:image/png,%3Ch1%3EHi%3C%2Fh1%3E\nfile:///tmp/a%20b.txt\n"
    );
}
