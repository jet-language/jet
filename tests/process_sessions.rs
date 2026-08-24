#![allow(dead_code, unused_imports)]

mod common;
#[path = "tir_support/mod.rs"]
mod tir_support;
include!("corelib_parts/support.rs");

#[cfg(any(unix, windows))]
fn compile_native_fixture(dir: &PathBuf) -> PathBuf {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/process_sessions/session_fixture.rs");
    let binary = dir.join(if cfg!(windows) {
        "session_fixture.exe"
    } else {
        "session_fixture"
    });
    let built = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(source)
        .arg("-o")
        .arg(&binary)
        .output()
        .expect("compile native process-session fixture");
    assert!(
        built.status.success(),
        "native process-session fixture failed to compile:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );
    binary
}

#[cfg(any(unix, windows))]
#[test]
fn native_fixture_terminal_bytes_resize_and_closed_stream() {
    let dir = std::env::temp_dir().join(format!(
        "jet_process_session_fixture_terminal_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let fixture = compile_native_fixture(&dir);
    let src = format!(
        r#"
use core.process as process

fn run() {{
    policy :: TerminalPolicy{{
        size: TerminalSize{{ cols: 80, rows: 24 }},
        mode: .Cooked
    }}
    child :: process.cmd(["{fixture}", "terminal"]).terminal(policy).spawn() ?? panic("spawn failed")
    session :: child.terminal ?? panic("missing terminal session")
    session.resize(TerminalSize{{ cols: 100, rows: 30 }}) ?? panic("resize failed")
    child.stdin.write("typed\n") ?? panic("write failed")
    result :: child.wait() ?? panic("wait failed")
    print(result.success)
    print(result.output.contains("input:typed"))
    print(result.output.contains("size:30x100"))
    print(result.output)
    if child.stdin.write("late") == {{
        .Ok(_) -> {{ print("closed:accepted") }}
        .Err(_) -> {{ print("closed:error") }}
    }}
}}
"#,
        fixture = jet_string_path(&fixture),
    );
    let (code, stdout, stderr) = build_and_run(&dir, "native_terminal", &src, &[], None);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert!(stdout.starts_with("true\ntrue\ntrue\n"), "{stdout:?}");
    assert!(
        stdout.contains("\x1b[31mcontrol\x1b[0m"),
        "ANSI bytes lost: {stdout:?}"
    );
    assert!(stdout.contains("size:30x100"), "resize lost: {stdout:?}");
    assert!(
        stdout.ends_with("closed:error\n"),
        "closed input was not typed: {stdout:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[cfg(any(unix, windows))]
#[test]
fn terminal_session_behavior_matches_all_execution_tiers() {
    let dir = std::env::temp_dir().join(format!(
        "jet_process_session_fixture_tiers_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let fixture = jet_string_path(&compile_native_fixture(&dir));
    let src = format!(
        r#"
use core.process as process

fn run() {{
    child :: process.cmd(["{fixture}", "terminal"]).terminal().spawn() ?? panic("spawn failed")
    session :: child.terminal ?? panic("missing terminal session")
    session.resize(TerminalSize{{ cols: 100, rows: 30 }}) ?? panic("resize failed")
    exited :: child.exited() ?? panic("poll failed")
    print(!exited)
    child.stdin.write("typed\n") ?? panic("write failed")
    result :: child.wait() ?? panic("wait failed")
    print(result.success)
    print(result.output.contains("input:typed"))
    print(result.output.contains("size:30x100"))
    if child.stdin.write("late") == {{
        .Ok(_) -> {{ print(false) }}
        .Err(_) -> {{ print(true) }}
    }}
}}
"#,
        fixture = fixture,
    );
    tir_support::assert_tiers_agree(
        "process_session_terminal_behavior",
        &src,
        "true\ntrue\ntrue\ntrue\ntrue\n",
    );
    let _ = fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn pipeline_stage_timeout_matches_all_execution_tiers() {
    let dir = std::env::temp_dir().join(format!(
        "jet_process_session_pipeline_timeout_tiers_{}",
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
    tir_support::assert_tiers_agree("process_pipeline_stage_timeout", &src, "true\n");
    let _ = fs::remove_dir_all(&dir);
}

#[cfg(any(unix, windows))]
#[test]
fn process_session_resource_limits_match_all_execution_tiers() {
    let dir = std::env::temp_dir().join(format!(
        "jet_process_session_resource_limits_tiers_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let fixture = jet_string_path(&compile_native_fixture(&dir));
    let resource_builders = if cfg!(target_os = "linux") {
        ".cpu_time_limit(cpu_budget).memory_limit(67108864).open_file_limit(64)"
    } else if cfg!(target_os = "windows") {
        ".cpu_time_limit(cpu_budget).memory_limit(67108864)"
    } else {
        ".cpu_time_limit(cpu_budget)"
    };
    let src = format!(
        r#"
use core.process as process

fn run() {{
    cpu_budget :: Duration.milliseconds(100) ?? panic("duration failed")
    accepted :: process.cmd(["{fixture}", "output", "small"]){resource_builders}.output_limit(16).run() ?? panic("under-limit run failed")
    print(accepted.success)
    print(accepted.output == "ok\n")
    limited :: process.cmd(["{fixture}", "output", "large"]).output_limit(16).run()
    if limited == {{
        .Ok(_) -> {{ print("limit:accepted") }}
        .Err(error) -> {{
            if error == {{
                .ResourceLimit(limit) -> {{
                    if limit == {{
                        .Output -> {{ print("limit:output") }}
                        else -> {{ print("limit:wrong") }}
                    }}
                }}
                else -> {{ print("limit:wrong") }}
            }}
        }}
    }}
}}
"#,
        fixture = fixture,
        resource_builders = resource_builders,
    );
    tir_support::assert_tiers_agree(
        "process_session_resource_limits",
        &src,
        "true\ntrue\nlimit:output\n",
    );
    let _ = fs::remove_dir_all(&dir);
}

#[cfg(target_os = "linux")]
#[test]
fn process_session_resource_limit_exhaustion_names_each_limit_all_tiers() {
    let dir = std::env::temp_dir().join(format!(
        "jet_process_session_resource_exhaustion_tiers_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let fixture = jet_string_path(&compile_native_fixture(&dir));
    let src = format!(
        r#"
use core.process as process

fn run() {{
    cpu :: process.cmd(["{fixture}", "cpu"]).cpu_time_limit(Duration.milliseconds(1000)).run()
    if cpu == {{
        .Ok(_) -> print("cpu:missed")
        .Err(error) -> {{
            if error == {{
                .ResourceLimit(limit) -> {{
                    if limit == {{
                        .CpuTime -> print("cpu:typed")
                        else -> print("cpu:wrong")
                    }}
                }}
                else -> print("cpu:wrong")
            }}
        }}
    }}
    memory :: process.cmd(["{fixture}", "memory"]).memory_limit(33554432).run()
    if memory == {{
        .Ok(_) -> print("memory:missed")
        .Err(error) -> {{
            if error == {{
                .ResourceLimit(limit) -> {{
                    if limit == {{
                        .Memory -> print("memory:typed")
                        else -> print("memory:wrong")
                    }}
                }}
                else -> print("memory:wrong")
            }}
        }}
    }}
    files :: process.cmd(["{fixture}", "files"]).open_file_limit(64).run()
    if files == {{
        .Ok(_) -> print("files:missed")
        .Err(error) -> {{
            if error == {{
                .ResourceLimit(limit) -> {{
                    if limit == {{
                        .OpenFiles -> print("files:typed")
                        else -> print("files:wrong")
                    }}
                }}
                else -> print("files:wrong")
            }}
        }}
    }}
}}
"#,
        fixture = fixture,
    );
    tir_support::assert_tiers_agree(
        "process_session_resource_limit_exhaustion",
        &src,
        "cpu:typed\nmemory:typed\nfiles:typed\n",
    );
    let _ = fs::remove_dir_all(&dir);
}

#[cfg(any(unix, windows))]
#[test]
fn native_fixture_controls_the_full_tree_on_interrupt_timeout_and_drop() {
    let dir = std::env::temp_dir().join(format!(
        "jet_process_session_fixture_tree_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let fixture = compile_native_fixture(&dir);
    let fixture = jet_string_path(&fixture);
    let pid_files = ["interrupt", "terminate", "kill", "timeout", "drop"]
        .map(|name| dir.join(format!("{name}.pid")));
    for path in &pid_files {
        let _ = fs::remove_file(path);
    }
    let src = format!(
        r#"
use core.process as process
use core.time as time

fn dropped(fixture: String, pid: String) {{
    child :: process.cmd([fixture, "tree", pid]).terminal().spawn() ?? panic("drop spawn failed")
    time.sleep(100ms)
}}

fn run() {{
    interrupt :: process.cmd(["{fixture}", "tree", "{interrupt}"]).terminal().spawn() ?? panic("interrupt spawn failed")
    time.sleep(100ms)
    interrupt.interrupt() ?? panic("interrupt failed")
    interrupt_result :: interrupt.wait() ?? panic("interrupt wait failed")
    print(interrupt_result.success)
    if interrupt.stdin.write("late") == {{
        .Ok(_) -> panic("interrupt cleanup left stdin open")
        .Err(error) -> {{
            if error == {{ .Closed(_) -> {{}} else -> panic("interrupt cleanup was not typed") }}
        }}
    }}

    terminate :: process.cmd(["{fixture}", "tree", "{terminate}"]).terminal().spawn() ?? panic("terminate spawn failed")
    time.sleep(100ms)
    terminate.terminate() ?? panic("terminate failed")
    terminate_result :: terminate.wait() ?? panic("terminate wait failed")
    print(terminate_result.success)
    if terminate.stdin.write("late") == {{
        .Ok(_) -> panic("terminate cleanup left stdin open")
        .Err(error) -> {{
            if error == {{ .Closed(_) -> {{}} else -> panic("terminate cleanup was not typed") }}
        }}
    }}

    kill :: process.cmd(["{fixture}", "tree", "{kill}"]).terminal().spawn() ?? panic("kill spawn failed")
    time.sleep(100ms)
    kill.kill() ?? panic("kill failed")
    kill_result :: kill.wait() ?? panic("kill wait failed")
    print(kill_result.success)
    if kill.stdin.write("late") == {{
        .Ok(_) -> panic("kill cleanup left stdin open")
        .Err(error) -> {{
            if error == {{ .Closed(_) -> {{}} else -> panic("kill cleanup was not typed") }}
        }}
    }}

    timeout :: Duration.milliseconds(100) ?? panic("duration failed")
    timed :: process.cmd(["{fixture}", "tree", "{timeout}"]).terminal().timeout(timeout).run() ?? panic("timeout failed")
    print(timed.timed_out)

    dropped("{fixture}", "{drop}")
    print("drop returned")
}}
"#,
        fixture = fixture,
        interrupt = jet_string_path(&pid_files[0]),
        terminate = jet_string_path(&pid_files[1]),
        kill = jet_string_path(&pid_files[2]),
        timeout = jet_string_path(&pid_files[3]),
        drop = jet_string_path(&pid_files[4]),
    );
    let (code, stdout, stderr) = build_and_run(&dir, "native_tree", &src, &[], None);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "false\nfalse\nfalse\ntrue\ndrop returned\n");

    for path in pid_files {
        let pid = fs::read_to_string(&path).expect("native fixture wrote descendant pid");
        let pid = pid.trim();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while native_pid_alive(pid) {
            if std::time::Instant::now() >= deadline {
                stop_native_pid(pid);
                panic!(
                    "native process-session descendant {pid} survived {:?}",
                    path
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    let _ = fs::remove_dir_all(&dir);
}

#[cfg(unix)]
fn native_pid_alive(pid: &str) -> bool {
    Command::new("kill")
        .args(["-0", pid])
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(windows)]
fn native_pid_alive(pid: &str) -> bool {
    use std::ffi::c_void;

    unsafe extern "system" {
        fn CloseHandle(handle: *mut c_void) -> i32;
        fn GetExitCodeProcess(handle: *mut c_void, code: *mut u32) -> i32;
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
    }

    let Ok(pid) = pid.parse::<u32>() else {
        return false;
    };
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const STILL_ACTIVE: u32 = 259;
    // SAFETY: the test only queries the explicitly recorded child PID and
    // closes the returned process handle exactly once.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return false;
    }
    let mut code = 0;
    let live = unsafe { GetExitCodeProcess(handle, &mut code) } != 0 && code == STILL_ACTIVE;
    unsafe { CloseHandle(handle) };
    live
}

#[cfg(unix)]
fn stop_native_pid(pid: &str) {
    let _ = Command::new("kill")
        .args(["-9", pid])
        .stderr(std::process::Stdio::null())
        .status();
}

#[cfg(windows)]
fn stop_native_pid(pid: &str) {
    let _ = Command::new("taskkill")
        .args(["/PID", pid, "/T", "/F"])
        .status();
}

#[test]
fn compatibility_matrix_names_every_session_capability() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let matrix = fs::read_to_string(root.join("tests/fixtures/process_sessions/compatibility.tsv"))
        .expect("read process-session compatibility matrix");
    let mut rows = matrix.lines();
    assert_eq!(
        rows.next(),
        Some("capability\tfixture\tlinux\tmacos\twindows\tother")
    );
    let expected = [
        "terminal-byte-stream",
        "terminal-resize",
        "terminal-input-and-close",
        "process-tree-interrupt",
        "process-tree-terminate",
        "process-tree-kill",
        "process-tree-timeout",
        "process-tree-drop",
    ];
    let rows: Vec<_> = rows.collect();
    assert_eq!(rows.len(), expected.len());
    let host_column = if cfg!(target_os = "linux") {
        2
    } else if cfg!(target_os = "macos") {
        3
    } else if cfg!(target_os = "windows") {
        4
    } else {
        5
    };
    for (row, capability) in rows.into_iter().zip(expected) {
        let fields: Vec<_> = row.split('\t').collect();
        assert_eq!(fields.len(), 6, "malformed matrix row: {row}");
        assert_eq!(fields[0], capability);
        assert_eq!(fields[1], "session_fixture.rs");
        let expected_test = if capability.starts_with("terminal-") {
            "native_fixture_terminal_bytes_resize_and_closed_stream"
        } else {
            "native_fixture_controls_the_full_tree_on_interrupt_timeout_and_drop"
        };
        assert!(fields[2..5].iter().all(|state| *state == expected_test));
        assert_eq!(
            fields[host_column],
            if host_column == 5 {
                "unsupported"
            } else {
                expected_test
            },
            "matrix does not describe this target's fixture evidence: {row}"
        );
        assert_eq!(fields[5], "unsupported");
    }
}
