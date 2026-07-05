//! c134 Phase 7: `jet dev <file>.jet --target=web` — the std-only watch +
//! rebuild-on-save + live-reload dev server (Source/CmdDevWeb.rs). Starts the
//! real `jet` binary as a child process, talks to it over a plain TCP socket
//! (no new dependency — matches the server's own std-only HTTP), edits a
//! throwaway temp copy of a fixture, and confirms the rebuild lands.
//!
//! Also covers c-devserver (owner-directed 2026-07-01): `jet dev <file>`
//! when the file defines a top-level `fn dev()` — the file configures and
//! starts its OWN `core.devserver` value instead of the CLI hardcoding
//! `--target=web --port=<N>`. Same spawn/TCP/rebuild harness, driven through
//! zero CLI flags — `jet dev app.jet`, no `--target=web`.

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn have_tool(name: &str) -> bool {
    Command::new(name).arg("--version").output().is_ok()
}

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

fn unused_local_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .expect("bind an ephemeral localhost port")
        .local_addr()
        .expect("read local addr")
        .port()
}

/// Minimal blocking HTTP/1.1 GET over a raw `TcpStream` — the same shape of
/// client `jet dev --target=web`'s own std-only server (Source/CmdDevWeb.rs)
/// expects, so the test doesn't need `curl` or any HTTP crate.
fn http_get(port: u16, path: &str) -> Option<(u16, Vec<u8>)> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let req = format!(
        "GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        path
    );
    stream.write_all(req.as_bytes()).ok()?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).ok()?;
    let sep = b"\r\n\r\n";
    let split = raw
        .windows(sep.len())
        .position(|w| w == sep)
        .map(|i| i + sep.len())?;
    let header_text = String::from_utf8_lossy(&raw[..split]);
    let status: u16 = header_text
        .lines()
        .next()?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;
    Some((status, raw[split..].to_vec()))
}

/// Polls `/__jet_dev_version` until it differs from `baseline`, or panics
/// after a generous timeout — the rebuild-on-save proof point.
fn wait_for_version_change(port: u16, baseline: &str, timeout: Duration) -> String {
    let start = Instant::now();
    loop {
        if let Some((200, body)) = http_get(port, "/__jet_dev_version") {
            let v = String::from_utf8_lossy(&body).trim().to_string();
            if v != baseline && !v.is_empty() {
                return v;
            }
        }
        if start.elapsed() > timeout {
            panic!(
                "/__jet_dev_version never changed from {:?} within {:?}",
                baseline, timeout
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

// ============================================================================
// Section: `--target=web` entry mode (was tests/web_dev.rs)
// ============================================================================

const FIXTURE_SRC: &str = r#"use core.ui as ui

fn run() {
    backend :: ui.null_backend()
    node :: ui.node("hello", 100.0, 20.0)
    constraint :: ui.constraint(0.0, 0.0, 200.0, 100.0)
    size :: backend.measure(node, constraint)
    print(size.width)
    print(size.height)
}
"#;

/// Same source with one literal changed, so the compiled `app.js` differs
/// byte-for-byte from `FIXTURE_SRC`'s — proof a rebuild actually re-ran the
/// compiler rather than re-serving a cached response.
const FIXTURE_SRC_EDITED: &str = r#"use core.ui as ui

fn run() {
    backend :: ui.null_backend()
    node :: ui.node("hello there", 100.0, 20.0)
    constraint :: ui.constraint(0.0, 0.0, 200.0, 100.0)
    size :: backend.measure(node, constraint)
    print(size.width)
    print(size.height)
}
"#;

/// Reads the child's stdout for the `serving http://localhost:<port>` line
/// `run_dev_web` prints on startup (Source/CmdDevWeb.rs), with a bounded
/// wait — never a blind `sleep`.
fn wait_for_port(child_stdout: std::process::ChildStdout) -> u16 {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        use std::io::{BufRead, BufReader};
        let reader = BufReader::new(child_stdout);
        for line in reader.lines().map_while(Result::ok) {
            if let Some(rest) = line.split("http://localhost:").nth(1) {
                if let Some(port_str) = rest.split(|c: char| !c.is_ascii_digit()).next() {
                    if let Ok(port) = port_str.parse::<u16>() {
                        let _ = tx.send(port);
                        return;
                    }
                }
            }
        }
    });
    rx.recv_timeout(Duration::from_secs(20))
        .expect("jet dev --target=web never printed its \"serving http://localhost:<port>\" line")
}

#[test]
fn jet_dev_web_serves_and_rebuilds_on_save() {
    if !have_tool("rustc") {
        eprintln!("note: skipping jet_dev_web_serves_and_rebuilds_on_save (need rustc)");
        return;
    }

    // Isolated scratch dir so this test never touches a real example file or
    // races other tests' `build/` output (D-style: temp dirs under
    // std::env::temp_dir(), matching tests/web_build.rs's convention).
    let dir = std::env::temp_dir().join(format!("jet_dev_web_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src_path = dir.join("app.jet");
    fs::write(&src_path, FIXTURE_SRC).unwrap();

    let mut child = Command::new(jet_bin())
        .args(["dev", "app.jet", "--target=web"])
        .current_dir(&dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start `jet dev --target=web`");

    // Make sure the child dies even if an assertion below panics.
    struct KillOnDrop(std::process::Child);
    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let stdout = child.stdout.take().unwrap();
    let guard = KillOnDrop(child);

    let port = wait_for_port(stdout);

    // The initial build must be live and match what the real compile
    // pipeline produces for the same source (in-process, same as
    // tests/web_build.rs's fixtures use).
    let expected_initial = jet::compile_web_with_path(FIXTURE_SRC, "app.jet")
        .expect("front end rejected fixture")
        .web
        .expect("web target compile must produce web artifacts")
        .js_app;
    let (status, body) = http_get(port, "/app.js").expect("GET /app.js failed");
    assert_eq!(status, 200);
    assert_eq!(
        String::from_utf8_lossy(&body),
        expected_initial,
        "served app.js should match a plain compile of the same source"
    );

    let (status, version_body) =
        http_get(port, "/__jet_dev_version").expect("GET /__jet_dev_version failed");
    assert_eq!(status, 200);
    let baseline_version = String::from_utf8_lossy(&version_body).trim().to_string();

    // index.html should carry the injected live-reload poller.
    let (status, html_body) = http_get(port, "/").expect("GET / failed");
    assert_eq!(status, 200);
    assert!(
        String::from_utf8_lossy(&html_body).contains("__jet_dev_version"),
        "served index.html should have the live-reload script injected"
    );

    // Edit the SAME file the dev server is watching (never a real example
    // file — this is the throwaway temp copy) and wait for the rebuild.
    fs::write(&src_path, FIXTURE_SRC_EDITED).unwrap();
    let new_version = wait_for_version_change(port, &baseline_version, Duration::from_secs(15));
    assert_ne!(new_version, baseline_version);

    let expected_edited = jet::compile_web_with_path(FIXTURE_SRC_EDITED, "app.jet")
        .expect("front end rejected edited fixture")
        .web
        .expect("web target compile must produce web artifacts")
        .js_app;
    let (status, body) = http_get(port, "/app.js").expect("GET /app.js (after edit) failed");
    assert_eq!(status, 200);
    assert_eq!(
        String::from_utf8_lossy(&body),
        expected_edited,
        "served app.js should reflect the rebuilt (edited) source"
    );
    assert_ne!(
        expected_initial, expected_edited,
        "sanity: the edit should actually change generated JS, or this test proves nothing"
    );

    // `guard`'s `Drop` kills and reaps the child process on scope exit.
    drop(guard);
    let _ = fs::remove_dir_all(&dir);
}

// ============================================================================
// Section: user-defined `fn dev()` entry mode (was tests/web_dev_fn.rs)
// ============================================================================

fn fn_fixture_src(port: u16, greeting: &str) -> String {
    format!(
        r#"use core.devserver as devserver

fn dev() {{
    server :: devserver.for_app("app.jet")
    server.port({port})
    server.serve()
}}

fn run() {{
    print("{greeting}")
}}
"#
    )
}

/// Polls for the server to come up on the port the fixture's own `dev()` passes
/// to `.port(...)` — never a blind `sleep`.
fn wait_for_server_up(port: u16, timeout: Duration) {
    let start = Instant::now();
    loop {
        if matches!(http_get(port, "/__jet_dev_version"), Some((200, _))) {
            return;
        }
        if start.elapsed() > timeout {
            panic!(
                "jet dev's own core.devserver never came up on port {}",
                port
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn jet_dev_runs_user_defined_dev_fn_and_rebuilds_on_save() {
    if !have_tool("rustc") {
        eprintln!(
            "note: skipping jet_dev_runs_user_defined_dev_fn_and_rebuilds_on_save (need rustc)"
        );
        return;
    }
    // The fixture's `dev()` shells out to `jet build --target=web` — it needs
    // to find `jet` itself on PATH, exactly like a real end-user run.
    let jet_dir = jet_bin().parent().unwrap().to_path_buf();
    let existing_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", jet_dir.display(), existing_path);

    let dir = std::env::temp_dir().join(format!("jet_dev_fn_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src_path = dir.join("app.jet");
    let port = unused_local_port();
    fs::write(&src_path, fn_fixture_src(port, "hello, world")).unwrap();

    let mut child = Command::new(jet_bin())
        .args(["dev", "app.jet"])
        .current_dir(&dir)
        .env("PATH", &new_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start `jet dev`");

    struct KillOnDrop(std::process::Child);
    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    // Drain stdout/stderr on background threads so a full pipe can never
    // wedge the child (dropping a `Stdio::piped()` handle instead of reading
    // it closes our end of the pipe and makes the child's next write fail
    // with a broken-pipe panic).
    let stderr = child.stderr.take().unwrap();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let reader = std::io::BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
            eprintln!("[child stderr] {}", line);
        }
    });
    let stdout = child.stdout.take().unwrap();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let reader = std::io::BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            eprintln!("[child stdout] {}", line);
        }
    });
    let guard = KillOnDrop(child);

    wait_for_server_up(port, Duration::from_secs(30));

    let (status, version_body) =
        http_get(port, "/__jet_dev_version").expect("GET /__jet_dev_version failed");
    assert_eq!(status, 200);
    let baseline_version = String::from_utf8_lossy(&version_body).trim().to_string();

    let (status, js_body) = http_get(port, "/app.js").expect("GET /app.js failed");
    assert_eq!(status, 200);
    assert!(
        !js_body.is_empty(),
        "served app.js from the user's own dev() server should be non-empty"
    );

    fs::write(&src_path, fn_fixture_src(port, "hello there")).unwrap();
    let new_version = wait_for_version_change(port, &baseline_version, Duration::from_secs(20));
    assert_ne!(new_version, baseline_version);

    drop(guard);
    let _ = fs::remove_dir_all(&dir);
}

/// `devserver.app()` — zero-arg "watch the file jet dev launched" — plus the
/// JET_BIN plumbing. Two things distinguish this from the `for_app` test
/// above, both deliberate:
///   1. the fixture names NO path at all (`devserver.app()`), so the watched
///      file must arrive via the JET_DEV_FILE env var `run_dev_entry` sets;
///   2. no PATH doctoring — the rebuild subprocess must find the compiler
///      through JET_BIN (the parent `jet`'s own `current_exe()`), not a PATH
///      lookup, so this also regresses the cwd-sensitive-wrapper failure
///      found during manual verification (nix devshell `jet` breaks under
///      the staging cwd; a bare `jet` may not be on PATH at all here).
#[test]
fn jet_dev_app_zero_arg_watches_launched_file() {
    if !have_tool("rustc") {
        eprintln!("note: skipping jet_dev_app_zero_arg_watches_launched_file (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_dev_app_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src_path = dir.join("app.jet");
    let port = unused_local_port();
    fs::write(
        &src_path,
        format!(
            r#"use core.devserver as devserver

fn dev() {{
    server :: devserver.app()
    server.port({port})
    server.serve()
}}

fn run() {{
    print("hello, world")
}}
"#
        ),
    )
    .unwrap();

    let mut child = Command::new(jet_bin())
        .args(["dev", "app.jet"])
        .current_dir(&dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start `jet dev`");

    struct KillOnDrop(std::process::Child);
    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let stderr = child.stderr.take().unwrap();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let reader = std::io::BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
            eprintln!("[child stderr] {}", line);
        }
    });
    let stdout = child.stdout.take().unwrap();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let reader = std::io::BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            eprintln!("[child stdout] {}", line);
        }
    });
    let guard = KillOnDrop(child);

    wait_for_server_up(port, Duration::from_secs(30));

    let (status, version_body) =
        http_get(port, "/__jet_dev_version").expect("GET /__jet_dev_version failed");
    assert_eq!(status, 200);
    let baseline_version = String::from_utf8_lossy(&version_body).trim().to_string();

    // Editing the launched file must trigger a rebuild — proving app()
    // really resolved to it (there is no other candidate: no path literal
    // exists anywhere in the fixture).
    let edited = fs::read_to_string(&src_path)
        .unwrap()
        .replace("hello, world", "hello there");
    fs::write(&src_path, edited).unwrap();
    let new_version = wait_for_version_change(port, &baseline_version, Duration::from_secs(20));
    assert_ne!(new_version, baseline_version);

    drop(guard);
    let _ = fs::remove_dir_all(&dir);
}
