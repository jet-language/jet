#[test]
fn perf_run_keeps_completed_socket_echo_io_span() {
    if !common::have_rustc() {
        return;
    }
    let root = temp_workspace();
    let source = root.join("socket_echo.jet");
    fs::write(
        &source,
        r#"use core.net as net
use core.tasks as tasks
use core.time as time

fn run() {
    listener :: net.tcp_listen("127.0.0.1:0") ?? panic("bind")
    address :: net.socket_to_string(net.listener_local_socket_addr(listener) ?? panic("address"))
    server :: task {
        stream :: listener.accept() ?? panic("accept")
        message :: stream.read_text(16) ?? panic("read")
        stream.write_all("echo:{message}".bytes()) ?? panic("write")
    }
    // Cross two 100 ms observe publications before completing the wait.
    time.sleep(250)
    client :: net.tcp_connect(address) ?? panic("connect")
    client.write_all("ping".bytes()) ?? panic("write")
    print(client.read_text(16) ?? panic("read"))
    server.join() ?? panic("server failed")
}
"#,
    )
    .unwrap();
    let out = root.join("socket-echo.jettrace");
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
    assert!(
        output.status.success(),
        "socket echo did not complete normally: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "echo:ping");
    let text = fs::read_to_string(&out).unwrap();
    assert_honest_io(&text);
    assert_completed_io_bound_to_tasks(&text);
    let _ = fs::remove_dir_all(&root);
}
