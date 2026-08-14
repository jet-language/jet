//! Tower #438 CLI/runtime smoke for D-WEBAPP1 / D-WEBAUTHOR1.
#![allow(non_snake_case)]

mod common;
use common::Scratch;

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn jet_bin() -> PathBuf {
    repo_root().join("target/debug/jet")
}

fn run_jet(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(jet_bin())
        .current_dir(repo_root())
        .args(args)
        .output()
        .expect("spawn jet");
    (
        out.status.code().unwrap_or(1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

struct ServerChild(Child);

impl Drop for ServerChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.local_addr().unwrap().port()
}

fn request(port: u16, method: &str, path: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut stream = loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(stream) => break stream,
            Err(error) if Instant::now() < deadline => {
                let _ = error;
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => panic!("web app did not listen on {port}: {error}"),
        }
    };
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
    )
    .unwrap();
    stream.shutdown(std::net::Shutdown::Write).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

fn spawn_server_with_program(program: &Path, args: &[&str], port: u16) -> ServerChild {
    ServerChild(
        Command::new(program)
            .current_dir(repo_root())
            .args(args)
            .env("JET_APP_PORT", port.to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn web app server"),
    )
}

fn spawn_server(args: &[&str], port: u16) -> ServerChild {
    spawn_server_with_program(&jet_bin(), args, port)
}

#[test]
fn app_graph_facts_json() {
    let (code, stdout, stderr) = run_jet(&[
        "explain",
        "--web-graph",
        "--json",
        "examples/features/web/web_app.jet",
    ]);
    assert_eq!(code, 0, "stderr={stderr}\nstdout={stdout}");
    assert!(stdout.contains("\"shared_tir\": true"), "{stdout}");
    assert!(stdout.contains("\"path\": \"/\""), "{stdout}");
    assert!(stdout.contains("csp-default"), "{stdout}");
    assert!(stdout.contains("hydration"), "{stdout}");
    assert!(stdout.contains("\"prefix\": \"/api\""), "{stdout}");
}

#[test]
fn web_app_expand_facts_web() {
    let (code, stdout, stderr) = run_jet(&[
        "inspect",
        "expand",
        "--facts",
        "web",
        "examples/features/web/web_app.jet",
    ]);
    assert_eq!(code, 0, "stderr={stderr}\nstdout={stdout}");
    assert!(stdout.contains("web application graph") || stdout.contains("shared TIR"), "{stdout}");
}

#[test]
fn web_app_routes_from_exhaustive() {
    let scratch = Scratch::new("webapp-routes");
    let tmp = scratch.path.clone();
    fs::create_dir_all(tmp.join("routes/orders")).unwrap();
    fs::write(
        tmp.join("app.jet"),
        r#"
use core.web as web

fn run() => App {
    return web.app().routes(from: "routes")
}
"#,
    )
    .unwrap();
    fs::write(tmp.join("routes/index.jet"), "fn page() {}\n").unwrap();
    fs::write(tmp.join("routes/orders/[id].jet"), "fn page() {}\n").unwrap();
    fs::write(tmp.join("routes/_helper.jet"), "fn helper() {}\n").unwrap();

    let (code, stdout, stderr) = run_jet(&[
        "explain",
        "--web-graph",
        "--json",
        tmp.join("app.jet").to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "stderr={stderr}\nstdout={stdout}");
    assert!(stdout.contains("\"path\": \"/\""), "{stdout}");
    assert!(stdout.contains("/orders/:id"), "{stdout}");
    assert!(!stdout.contains("_helper"), "{stdout}");
}

#[test]
fn web_app_run_serves_pages_actions_and_assets() {
    let port = free_port();
    let _server = spawn_server(&["run", "examples/features/web/web_app.jet"], port);

    let page = request(port, "GET", "/");
    assert!(page.starts_with("HTTP/1.1 200"), "{page}");
    assert!(page.contains("<title>Home</title>"), "{page}");
    assert!(page.contains("hello from csr"), "{page}");

    let action = request(port, "POST", "/actions/save");
    assert!(action.starts_with("HTTP/1.1 200"), "{action}");
    assert!(action.ends_with("ok"), "{action}");

    let asset = request(port, "GET", "/assets/app.css");
    assert!(asset.starts_with("HTTP/1.1 200"), "{asset}");
    assert!(asset.contains("font-family"), "{asset}");
}

#[test]
fn typed_app_args_serve_in_default_and_aot_modes() {
    let port = free_port();
    let _server = spawn_server(
        &[
            "run",
            "examples/features/web/app_typed_args.jet",
            "--",
            "--port=9000",
        ],
        port,
    );
    let response = request(port, "GET", "/");
    assert!(response.starts_with("HTTP/1.1 404"), "default typed App: {response}");
    drop(_server);

    let (code, stdout, stderr) = run_jet(&[
        "build",
        "examples/features/web/app_typed_args.jet",
    ]);
    assert_eq!(code, 0, "stderr={stderr}\nstdout={stdout}");
    let port = free_port();
    let _server = spawn_server_with_program(
        &repo_root().join("build/app_typed_args"),
        &["--port=9000"],
        port,
    );
    let response = request(port, "GET", "/");
    assert!(response.starts_with("HTTP/1.1 404"), "AOT typed App: {response}");
}

#[test]
fn typed_app_args_report_invalid_port_with_shared_fix() {
    for args in [
        &[
            "run",
            "examples/features/web/app_typed_args.jet",
            "--",
            "--port=nine",
        ][..],
        &[
            "run",
            "--interpret",
            "examples/features/web/app_typed_args.jet",
            "--",
            "--port=nine",
        ][..],
        &[
            "run",
            "--release",
            "examples/features/web/app_typed_args.jet",
            "--",
            "--port=nine",
        ][..],
    ] {
        let (code, stdout, stderr) = run_jet(args);
        assert_eq!(code, 2, "args={args:?}\nstdout={stdout}\nstderr={stderr}");
        assert!(stdout.is_empty(), "args={args:?}\nstdout={stdout}");
        assert!(
            stderr.contains("`--port` expects an Int, got `nine`"),
            "args={args:?}\nstderr={stderr}"
        );
        assert!(
            stderr.contains("fix: pass a whole number to `--port`"),
            "args={args:?}\nstderr={stderr}"
        );
    }
}

#[test]
fn web_app_dev_forced_interpreter_serves_callbacks() {
    let port = free_port();
    let _server = spawn_server(
        &[
            "dev",
            "examples/features/web/web_app.jet",
            "--interpret",
            &format!("--port={port}"),
        ],
        port,
    );

    let page = request(port, "GET", "/");
    assert!(page.starts_with("HTTP/1.1 200"), "{page}");
    assert!(page.contains("<title>Home</title>"), "{page}");
    assert!(page.contains("hello from csr"), "{page}");

    let action = request(port, "POST", "/actions/save");
    assert!(action.starts_with("HTTP/1.1 200"), "{action}");
    assert!(action.ends_with("ok"), "{action}");
}

#[test]
fn web_app_dev_auto_serves_with_reload_without_dev_function() {
    let scratch = Scratch::new("webapp-reload");
    let tmp = scratch.path.clone();
    let source = fs::read_to_string(repo_root().join("examples/features/web/web_app.jet")).unwrap();
    let app = tmp.join("app.jet");
    fs::write(&app, source).unwrap();
    let port = free_port();
    let _server = spawn_server(
        &[
            "dev",
            app.to_str().unwrap(),
            &format!("--port={port}"),
        ],
        port,
    );
    let page = request(port, "GET", "/");
    assert!(page.starts_with("HTTP/1.1 200"), "{page}");
    assert!(page.contains("EventSource(\"/__jet/reload\")"), "{page}");
    let (sent, received) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        sent.send(request(port, "GET", "/__jet/reload")).unwrap();
    });
    assert!(
        received.recv_timeout(Duration::from_millis(250)).is_err(),
        "reload endpoint fired without a source change"
    );
    let mut changed = fs::OpenOptions::new().append(true).open(&app).unwrap();
    writeln!(changed).unwrap();
    let reload = received.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(reload.starts_with("HTTP/1.1 200"), "{reload}");
    assert!(reload.contains("text/event-stream"), "{reload}");
    assert!(reload.contains("data: reload"), "{reload}");
}

#[test]
fn app_hello_graph() {
    let (code, stdout, stderr) = run_jet(&[
        "explain",
        "--web-graph",
        "--json",
        "examples/features/web/app_hello.jet",
    ]);
    assert_eq!(code, 0, "stderr={stderr}\nstdout={stdout}");
    assert!(stdout.contains("\"handler\": \"home\""), "{stdout}");
    assert!(stdout.contains("csp"), "{stdout}");
}
