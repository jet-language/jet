//! c134 Phase 7: `jet dev <file>.jet --target=web` — the std-only watch +
//! rebuild-on-save + live-reload dev server (Source/CmdDevWeb.rs). Starts the
//! real `jet` binary as a child process, talks to it over a plain TCP socket
//! (no new dependency — matches the server's own std-only HTTP), edits a
//! throwaway temp copy of a fixture, and confirms the rebuild lands.
//!
//! Also covers c-devserver (owner-directed 2026-07-01): `jet dev <file>`
//! when the file defines a top-level `fn dev()` — the file configures and
//! starts its OWN `core.web.devserver` value instead of the CLI hardcoding
//! `--target=web --port=<N>`. Same spawn/TCP/rebuild harness, driven through
//! zero CLI flags — `jet dev app.jet`, no `--target=web`.

mod common;

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{mpsc, Mutex, OnceLock};
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

static CANVAS_SESSIONS: OnceLock<Mutex<HashMap<u16, String>>> = OnceLock::new();

fn canvas_session(port: u16) -> Option<String> {
    CANVAS_SESSIONS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap()
        .get(&port)
        .cloned()
}

fn remember_canvas_session(port: u16, session: String) {
    CANVAS_SESSIONS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap()
        .insert(port, session);
}

/// Minimal blocking HTTP/1.1 GET over a raw `TcpStream` — the same shape of
/// client `jet dev --target=web`'s own std-only server (Source/CmdDevWeb.rs)
/// expects, so the test doesn't need `curl` or any HTTP crate.
fn http_get(port: u16, path: &str) -> Option<(u16, Vec<u8>)> {
    http_get_with_session(port, path, canvas_session(port).as_deref())
}

fn http_get_without_session(port: u16, path: &str) -> Option<(u16, Vec<u8>)> {
    http_get_with_session(port, path, None)
}

fn http_get_with_session(
    port: u16,
    path: &str,
    session: Option<&str>,
) -> Option<(u16, Vec<u8>)> {
    let (status, _, body) = http_get_response_with_session(port, path, session)?;
    Some((status, body))
}

fn http_get_with_content_type(port: u16, path: &str) -> Option<(u16, String, Vec<u8>)> {
    http_get_response_with_session(port, path, None)
}

fn http_get_response_with_session(
    port: u16,
    path: &str,
    session: Option<&str>,
) -> Option<(u16, String, Vec<u8>)> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let authorization = session
        .map(|session| format!("Authorization: Bearer {session}\r\n"))
        .unwrap_or_default();
    let req = format!(
        "GET {} HTTP/1.1\r\nHost: localhost:{}\r\n{}Sec-Fetch-Site: same-origin\r\nConnection: close\r\n\r\n",
        path, port, authorization
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
    let content_type = header_text.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-type")
            .then(|| value.trim().to_string())
    }).unwrap_or_default();
    Some((status, content_type, raw[split..].to_vec()))
}

fn http_post(port: u16, path: &str, body: &str) -> Option<(u16, Vec<u8>)> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let authorization = canvas_session(port)
        .map(|session| format!("Authorization: Bearer {session}\r\n"))
        .unwrap_or_default();
    let req = format!(
        "POST {} HTTP/1.1\r\nHost: localhost:{}\r\n{}Origin: http://localhost:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        path,
        port,
        authorization,
        port,
        body.len(),
        body
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

fn json_field(haystack: &str, field: &str) -> String {
    let key = format!("\"{field}\":\"");
    let start = haystack.find(&key).expect("json field") + key.len();
    let rest = &haystack[start..];
    rest[..rest.find('"').expect("json field terminator")].to_string()
}

fn debug_session_id(body: &str) -> String {
    let payload = body
        .split_once("\"protocol\":\"jet.canvas.debug\"")
        .map(|(_, rest)| rest)
        .expect("debug protocol payload");
    json_field(payload, "id")
}

fn json_number_after(haystack: &str, marker: &str) -> u16 {
    let start = haystack.find(marker).expect("json number marker") + marker.len();
    haystack[start..]
        .split(|ch: char| !ch.is_ascii_digit())
        .next()
        .expect("json number")
        .parse()
        .expect("json number value")
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

fn wait_for_status(port: u16, client: &str, state: &str, timeout: Duration) -> String {
    let start = Instant::now();
    let path = format!("/__jet_dev_status?client={client}");
    loop {
        if let Some((200, body)) = http_get(port, &path) {
            let body = String::from_utf8_lossy(&body).into_owned();
            if body.contains(&format!("\"state\":\"{state}\"")) {
                return body;
            }
        }
        if start.elapsed() > timeout {
            panic!("dev status never reached {state:?} within {timeout:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn wait_for_file_text(path: &std::path::Path, needle: &str, timeout: Duration) -> String {
    let start = Instant::now();
    loop {
        let text = fs::read_to_string(path).unwrap_or_default();
        if text.contains(needle) {
            return text;
        }
        if start.elapsed() > timeout {
            panic!("PTY transcript never contained {needle:?} within {timeout:?}:\n{text}");
        }
        std::thread::sleep(Duration::from_millis(50));
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

fn port_from_url_line(line: &str, prefix: &str) -> Option<u16> {
    let url = line.strip_prefix(prefix)?;
    let authority = url.split("://").nth(1)?.split('/').next()?;
    authority.rsplit_once(':')?.1.parse().ok()
}

fn session_from_canvas_url_line(line: &str) -> Option<String> {
    let rest = line.strip_prefix("Canvas: ")?.split("session=").nth(1)?;
    rest.split(|c: char| !c.is_ascii_hexdigit())
        .next()
        .filter(|token| token.len() >= 32)
        .map(str::to_string)
}

/// Reads the app preview and Canvas lines `run_dev_web` prints on startup,
/// with a bounded wait — never a blind `sleep`.
#[derive(Clone, Copy)]
struct DevPorts {
    canvas: u16,
    application: u16,
    app_before_canvas: bool,
}

fn wait_for_ports(child_stdout: std::process::ChildStdout) -> DevPorts {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        use std::io::{BufRead, BufReader};
        let reader = BufReader::new(child_stdout);
        let mut canvas_port = None;
        let mut application_port = None;
        let mut session = None;
        let mut saw_canvas = false;
        let mut app_before_canvas = false;
        for line in reader.lines().map_while(Result::ok) {
            if canvas_port.is_none() {
                if let Some(port) = port_from_url_line(&line, "Canvas: ") {
                    canvas_port = Some(port);
                }
                saw_canvas |= line.starts_with("Canvas: ");
            }
            if application_port.is_none() {
                if let Some(port) = port_from_url_line(&line, "App preview: ") {
                    application_port = Some(port);
                }
                if line.starts_with("App preview: ") && !saw_canvas {
                    app_before_canvas = true;
                }
            }
            if session.is_none() {
                if let Some(token) = session_from_canvas_url_line(&line) {
                    session = Some(token);
                }
            }
            if let (Some(canvas), Some(application), Some(session)) =
                (canvas_port, application_port, session.as_ref())
            {
                remember_canvas_session(canvas, session.clone());
                let _ = tx.send(DevPorts {
                    canvas,
                    application,
                    app_before_canvas,
                });
                return;
            }
        }
    });
    rx.recv_timeout(Duration::from_secs(20))
        .expect("jet dev never printed its serving URL and Canvas session")
}

fn wait_for_app_preview(child_stdout: std::process::ChildStdout) -> u16 {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        use std::io::{BufRead, BufReader};
        let reader = BufReader::new(child_stdout);
        let mut sent = false;
        for line in reader.lines().map_while(Result::ok) {
            if !sent {
                if let Some(port) = port_from_url_line(&line, "App preview: ") {
                    let _ = tx.send(port);
                    sent = true;
                }
            }
        }
    });
    rx.recv_timeout(Duration::from_secs(20))
        .expect("jet dev never printed its static app preview URL")
}

fn wait_for_canvas_port(path: &std::path::Path, timeout: Duration) -> u16 {
    let transcript = wait_for_file_text(path, "Canvas: http://", timeout);
    let (port, session) = transcript
        .lines()
        .find_map(|line| {
            Some((
                port_from_url_line(line, "Canvas: ")?,
                session_from_canvas_url_line(line)?,
            ))
        })
        .expect("PTY startup did not include a Canvas URL");
    remember_canvas_session(port, session);
    port
}

#[test]
fn ui_showcase_uses_builtin_host_and_keeps_companion_page_live() {
    if !have_tool("rustc") {
        eprintln!("note: skipping UI showcase dev host proof (need rustc)");
        return;
    }

    let dir = std::env::temp_dir().join(format!("jet_ui_showcase_dev_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("app.jet"),
        include_str!("../examples/features/web/ui_web_click.jet"),
    )
    .unwrap();
    fs::write(
        dir.join("ui_web_click.html"),
        include_str!("../examples/features/web/ui_web_click.html"),
    )
    .unwrap();

    let port = unused_local_port();
    let child = Command::new(jet_bin())
        .args(["dev", "app.jet", &format!("--port={port}")])
        .current_dir(&dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start UI showcase dev host");

    struct KillOnDrop(std::process::Child);
    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let guard = KillOnDrop(child);
    wait_for_server_up(port, Duration::from_secs(30));

    for request in 0..3 {
        let (status, body) = http_get(port, "/").expect("GET showcase companion page");
        assert_eq!(status, 200, "request {request}");
        let html = String::from_utf8_lossy(&body);
        assert!(
            html.contains("<h1>Reactive counter, rendered live</h1>"),
            "request {request}: {html}"
        );
        assert!(
            html.contains("id=\"jet-app\"") && html.contains("jet_main"),
            "request {request}: showcase host served a generic shell"
        );
    }
    let (status, js) = http_get(port, "/app.js").expect("GET showcase app.js");
    assert_eq!(status, 200);
    assert!(String::from_utf8_lossy(&js).contains("function jet_main()"));

    drop(guard);
    let _ = fs::remove_dir_all(&dir);
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

    let ports = wait_for_ports(stdout);
    assert!(
        (8080..=8089).contains(&ports.application),
        "default app preview port escaped the stable range: {}",
        ports.application
    );
    assert!(
        ports.app_before_canvas,
        "startup must print App preview before Canvas"
    );
    assert_eq!(
        ports.application, ports.canvas,
        "the workbench serves app preview and Canvas on one listener (card #2170)"
    );
    let port = ports.application;

    for path in ["/C:/Windows/win.ini", "/\\Windows\\win.ini", "/\\\\server\\share\\secret"] {
        let (status, _) = http_get(port, path).expect("static-path hostile request failed");
        assert_eq!(
            status, 400,
            "Windows absolute static path escaped the build root: {path}"
        );
    }

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
    let served = String::from_utf8_lossy(&body);
    // The dev host appends a source-map reference for the workbench debugger;
    // everything before that trailer must match a plain compile byte-for-byte.
    let map_trailer = "//# sourceMappingURL=app.js.map\n";
    let stripped = served
        .strip_suffix(map_trailer)
        .unwrap_or_else(|| panic!("served app.js must end with the dev source-map trailer"));
    assert_eq!(
        stripped, expected_initial,
        "served app.js (minus the dev source-map trailer) should match a plain compile of the same source"
    );
    let (status, _map) = http_get(port, "/app.js.map").expect("GET /app.js.map failed");
    assert_eq!(status, 200, "the referenced source map must be served");

    let (status, version_body) =
        http_get(port, "/__jet_dev_version").expect("GET /__jet_dev_version failed");
    assert_eq!(status, 200);
    let baseline_version = String::from_utf8_lossy(&version_body).trim().to_string();
    let (status, status_body) =
        http_get(port, "/__jet_dev_status").expect("GET /__jet_dev_status failed");
    assert_eq!(status, 200);
    let status_text = String::from_utf8_lossy(&status_body);
    assert!(status_text.contains("\"state\":\"ready\""), "{status_text}");
    assert!(status_text.contains("\"clients\":"), "{status_text}");

    // index.html should carry the injected live-reload poller.
    let (status, html_body) = http_get_without_session(port, "/").expect("GET / failed");
    assert_eq!(status, 200);
    let html = String::from_utf8_lossy(&html_body);
    assert!(
        html.contains("__jet_dev_status") && html.contains("Build failed"),
        "served index.html should have the live-reload script injected"
    );
    let (status, _) =
        http_get_without_session(port, "/canvas").expect("GET /canvas without session");
    assert_eq!(
        status, 401,
        "the Canvas workbench route must keep its session gate on the shared listener"
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
    let served_edited = String::from_utf8_lossy(&body);
    let served_edited = served_edited.strip_suffix(map_trailer).unwrap_or(&served_edited);
    assert_eq!(
        served_edited, expected_edited,
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

#[test]
fn jet_dev_web_rejects_windows_absolute_static_paths() {
    if !have_tool("rustc") {
        eprintln!(
            "note: skipping jet_dev_web_rejects_windows_absolute_static_paths (need rustc)"
        );
        return;
    }

    let dir = std::env::temp_dir().join(format!(
        "jet_dev_windows_static_paths_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("app.jet"), FIXTURE_SRC).unwrap();
    let mut child = Command::new(jet_bin())
        .args(["dev", "app.jet", "--target=web"])
        .current_dir(&dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to start `jet dev --target=web`");

    struct KillOnDrop(std::process::Child);
    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let stdout = child.stdout.take().unwrap();
    let guard = KillOnDrop(child);
    let ports = wait_for_ports(stdout);

    for path in [
        "/C:/Windows/win.ini",
        "/C:\\Windows\\win.ini",
        "/\\Windows\\win.ini",
        "/\\\\server\\share\\secret",
    ] {
        let (status, _) = http_get(ports.application, path)
            .expect("Windows absolute static-path request failed");
        assert_eq!(
            status, 400,
            "Windows absolute static path escaped the build root: {path}"
        );
    }

    drop(guard);
    let _ = fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn web_test_static_server_rejects_symlink_escape() {
    if !have_tool("node") {
        eprintln!("note: skipping web_test_static_server_rejects_symlink_escape (need node)");
        return;
    }

    use std::os::unix::fs::symlink;

    let root = std::env::temp_dir().join(format!(
        "jet-web-test-static-symlink-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("index.html"), "safe\n").unwrap();
    let outside = root.with_file_name(format!(
        "jet-web-test-static-outside-{}",
        std::process::id()
    ));
    fs::write(&outside, "secret\n").unwrap();
    symlink(&outside, root.join("escape.js")).unwrap();

    let port = unused_local_port();
    let child = Command::new("node")
        .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/web-test/serve.mjs"))
        .arg("--port")
        .arg(port.to_string())
        .arg("--root")
        .arg(&root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start web-test static server");
    struct KillOnDrop(std::process::Child);
    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let _guard = KillOnDrop(child);

    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        if let Some((200, _)) = http_get(port, "/") {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let (status, _) = http_get(port, "/escape.js").expect("static server request");
    assert_eq!(status, 400, "symlink outside the static root was served");
    assert_eq!(fs::read_to_string(&outside).unwrap(), "secret\n");

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_file(&outside);
}

#[test]
fn jet_dev_web_error_overlay_status_last_good_and_recovery_stay_in_lockstep() {
    if !have_tool("rustc") {
        eprintln!(
            "note: skipping jet_dev_web_error_overlay_status_last_good_and_recovery_stay_in_lockstep (need rustc)"
        );
        return;
    }

    let dir = std::env::temp_dir().join(format!("jet_dev_hybrid_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src_path = dir.join("app.jet");
    fs::write(&src_path, FIXTURE_SRC).unwrap();

    let mut child = Command::new(jet_bin())
        .args(["dev", "app.jet", "--target=web", "--verbose"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start hybrid `jet dev --target=web`");

    struct KillOnDrop(std::process::Child);
    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let (stderr_tx, stderr_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut stderr = stderr;
        let mut bytes = Vec::new();
        let _ = stderr.read_to_end(&mut bytes);
        let _ = stderr_tx.send(String::from_utf8_lossy(&bytes).into_owned());
    });
    let guard = KillOnDrop(child);
    let ports = wait_for_ports(stdout);
    let port = ports.canvas;
    let application_port = ports.application;

    let ready = wait_for_status(port, "tab-a", "ready", Duration::from_secs(10));
    assert!(ready.contains("\"clients\":1"), "{ready}");
    assert!(
        ready.contains(&format!("ready · localhost:{port} · 1 client")),
        "{ready}"
    );
    let second_tab = wait_for_status(port, "tab-b", "ready", Duration::from_secs(2));
    assert!(second_tab.contains("\"clients\":2"), "{second_tab}");

    let (_, baseline_body) = http_get(port, "/__jet_dev_version").expect("baseline version");
    let baseline_version = String::from_utf8_lossy(&baseline_body).trim().to_string();
    let (_, initial_js) =
        http_get(application_port, "/app.js").expect("initial last-good app.js");

    let broken = FIXTURE_SRC.replace("print(size.height)", "missing_hybrid_symbol()");
    fs::write(&src_path, broken).unwrap();
    let error = wait_for_status(port, "tab-a", "error", Duration::from_secs(10));
    assert!(error.contains("\"code\":\"E0102\""), "{error}");
    assert!(error.contains("Error [E0102]"), "{error}");
    assert!(error.contains("missing_hybrid_symbol"), "{error}");
    assert!(error.contains("error · E0102 · "), "{error}");

    let (_, error_version_body) =
        http_get(port, "/__jet_dev_version").expect("version during error");
    assert_eq!(
        String::from_utf8_lossy(&error_version_body).trim(),
        baseline_version,
        "error build must not trigger reload"
    );
    let (_, last_good_js) =
        http_get(application_port, "/app.js").expect("last-good app.js during error");
    assert_eq!(
        last_good_js, initial_js,
        "error build replaced last-good output"
    );

    let (_, html) = http_get(application_port, "/").expect("injected browser shell");
    let html = String::from_utf8_lossy(&html);
    for marker in [
        "overlayBody.textContent = s.diagnostic",
        "dismissedDiagnostic",
        "state === \"building\" || state === \"reconnecting\"",
        "location.reload()",
    ] {
        assert!(
            html.contains(marker),
            "browser hybrid control missing {marker}"
        );
    }

    fs::write(&src_path, FIXTURE_SRC_EDITED).unwrap();
    let recovered_version =
        wait_for_version_change(port, &baseline_version, Duration::from_secs(15));
    assert_ne!(recovered_version, baseline_version);
    let recovered = wait_for_status(port, "tab-a", "ready", Duration::from_secs(5));
    assert!(recovered.contains("\"diagnostic\":\"\""), "{recovered}");
    assert!(recovered.contains("\"code\":\"\""), "{recovered}");
    let expected_edited = jet::compile_web_with_path(FIXTURE_SRC_EDITED, "app.jet")
        .expect("front end rejected recovered fixture")
        .web
        .expect("web output after recovery")
        .js_app;
    let (_, recovered_js) = http_get(application_port, "/app.js").expect("recovered app.js");
    let recovered_js = String::from_utf8_lossy(&recovered_js);
    let recovered_js = recovered_js
        .strip_suffix("//# sourceMappingURL=app.js.map\n")
        .unwrap_or(&recovered_js);
    assert_eq!(recovered_js, expected_edited);

    drop(guard);
    let terminal = stderr_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("collect append-only terminal transcript");
    assert!(
        !terminal.contains('\x1b'),
        "NO_COLOR transcript carried ANSI: {terminal}"
    );
    for marker in [
        "jet dev  [ready]",
        "jet dev  [building]",
        "jet dev  [error] E0102",
        "Error [E0102]",
        "missing_hybrid_symbol",
        "save app.jet  →  error E0102",
        "save app.jet  →  rebuilt",
        "GET  /app.js",
    ] {
        assert!(
            terminal.contains(marker),
            "terminal hybrid transcript missing {marker}:\n{terminal}"
        );
    }

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn jet_dev_web_real_pty_pins_header_toggles_verbose_and_restores_scroll_region() {
    if !have_tool("rustc") || !have_tool("script") {
        eprintln!(
            "note: skipping jet_dev_web_real_pty_pins_header_toggles_verbose_and_restores_scroll_region (need rustc + script)"
        );
        return;
    }

    let dir = std::env::temp_dir().join(format!("jet_dev_hybrid_pty_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src_path = dir.join("app.jet");
    let transcript_path = dir.join("terminal.typescript");
    fs::write(&src_path, FIXTURE_SRC).unwrap();
    let port = unused_local_port();

    // util-linux `script` allocates a real pseudoterminal. `-f` flushes each
    // redraw into the transcript so this test can drive the live process,
    // not inspect a post-exit string facade. NO_COLOR matches the owner's
    // host environment and proves color is not required for pinning/input.
    let command = format!(
        "env NO_COLOR=1 {} dev app.jet --target=web --port={port}",
        jet_bin().display()
    );
    let mut pty = Command::new("script")
        .args(["-qfec", &command])
        .arg(&transcript_path)
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start real PTY wrapper");
    let mut input = pty.stdin.take().expect("PTY stdin");

    struct KillOnDrop(std::process::Child);
    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut guard = KillOnDrop(pty);
    let canvas_port = wait_for_canvas_port(&transcript_path, Duration::from_secs(20));
    wait_for_server_up(canvas_port, Duration::from_secs(20));

    let ready = wait_for_status(canvas_port, "pty-tab", "ready", Duration::from_secs(5));
    assert!(ready.contains("\"clients\":1"), "{ready}");
    let pinned = wait_for_file_text(
        &transcript_path,
        &format!("jet dev  [ready] localhost:{canvas_port} · 1 client"),
        Duration::from_secs(5),
    );
    assert!(
        pinned.contains("watching app.jet · Canvas"),
        "pinned detail row missing:\n{pinned}"
    );
    assert!(
        !pinned.contains("\x1b[32m"),
        "NO_COLOR PTY emitted colored status: {pinned:?}"
    );

    input.write_all(b"v").unwrap();
    input.flush().unwrap();
    let verbose = wait_for_file_text(
        &transcript_path,
        "verbose request/rebuild log enabled · press v to collapse",
        Duration::from_secs(5),
    );
    assert!(
        verbose.contains("\x1b[3r") && verbose.contains("\x1b[3;1H"),
        "verbose mode never pinned rows 1-2 with a row-3 scroll region: {verbose:?}"
    );

    http_get(port, "/app.js").expect("request while verbose");
    wait_for_file_text(&transcript_path, "GET  /app.js", Duration::from_secs(5));

    let broken = FIXTURE_SRC.replace("print(size.height)", "missing_hybrid_symbol()");
    fs::write(&src_path, broken).unwrap();
    wait_for_status(canvas_port, "pty-tab", "error", Duration::from_secs(10));
    let error = wait_for_file_text(
        &transcript_path,
        "missing_hybrid_symbol",
        Duration::from_secs(5),
    );
    for marker in ["jet dev  [error] E0102", "Error [E0102]", "Why:", "Fix:"] {
        assert!(
            error.contains(marker),
            "verbose PTY diagnostic missing {marker}:\n{error}"
        );
    }

    input.write_all(b"v").unwrap();
    input.flush().unwrap();
    let collapsed = wait_for_file_text(
        &transcript_path,
        "\x1b[r\x1b[2J\x1b[H",
        Duration::from_secs(5),
    );
    let scroll_regions_before = collapsed.matches("\x1b[3r").count();

    // Re-enter verbose mode, then send EOF while DECSTBM is still active.
    // The prior version exited only after collapsing, so its cleanup assertion
    // could pass without the input guard doing any restoration.
    input.write_all(b"v").unwrap();
    input.flush().unwrap();
    let start = Instant::now();
    let before_eof = loop {
        let text = fs::read_to_string(&transcript_path).unwrap_or_default();
        if text.matches("\x1b[3r").count() > scroll_regions_before {
            break text.len();
        }
        if start.elapsed() > Duration::from_secs(5) {
            panic!("PTY did not reactivate verbose scroll region: {text:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    input.write_all(&[4]).unwrap();
    input.flush().unwrap();

    let start = Instant::now();
    let after_eof = loop {
        let text = fs::read_to_string(&transcript_path).unwrap_or_default();
        if text[before_eof..].contains("\x1b[r") {
            break text;
        }
        if start.elapsed() > Duration::from_secs(5) {
            panic!("EOF did not restore active scroll region: {text:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    };

    // EOF leaves the server running in cooked mode. A later refresh must not
    // reinstall DECSTBM after its raw-input cleanup guard has gone away.
    let scroll_regions_after_eof = after_eof.matches("\x1b[3r").count();
    wait_for_status(canvas_port, "post-eof-tab", "error", Duration::from_secs(5));
    std::thread::sleep(Duration::from_millis(100));
    let after_refresh = fs::read_to_string(&transcript_path).unwrap();
    assert_eq!(
        after_refresh.matches("\x1b[3r").count(),
        scroll_regions_after_eof,
        "refresh reinstalled DECSTBM without raw controls: {after_refresh:?}"
    );

    // Cooked terminal now owns Ctrl-C; use it only to terminate the fixture.
    input.write_all(&[3]).unwrap();
    input.flush().unwrap();

    let start = Instant::now();
    loop {
        if let Some(status) = guard.0.try_wait().unwrap() {
            assert_eq!(status.code(), Some(130), "PTY wrapper exit: {status}");
            break;
        }
        if start.elapsed() > Duration::from_secs(5) {
            panic!("PTY wrapper did not exit after Ctrl-C");
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn jet_dev_web_browser_runs_hybrid_status_overlay_and_recovery_matrix() {
    if !have_tool("rustc") || !have_tool("node") || !have_tool("chromium") || !have_tool("script") {
        eprintln!(
            "note: skipping jet_dev_web_browser_runs_hybrid_status_overlay_and_recovery_matrix (need rustc + node + chromium + script)"
        );
        return;
    }

    let dir = std::env::temp_dir().join(format!("jet_dev_hybrid_browser_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src_path = dir.join("app.jet");
    let transcript_path = dir.join("browser-terminal.typescript");
    fs::write(&src_path, FIXTURE_SRC).unwrap();
    let port = unused_local_port();

    let command = format!(
        "env NO_COLOR=1 {} dev app.jet --target=web --verbose --port={port}",
        jet_bin().display()
    );
    let mut child = Command::new("script")
        .args(["-qfec", &command])
        .arg(&transcript_path)
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to start browser-matrix dev server in PTY");
    let mut input = child.stdin.take().expect("browser-matrix PTY stdin");

    struct KillOnDrop(std::process::Child);
    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut guard = KillOnDrop(child);
    wait_for_server_up(port, Duration::from_secs(20));

    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("node")
        .arg(repo.join("scripts/web-dev-test/hybrid.mjs"))
        .arg("--port")
        .arg(port.to_string())
        .arg("--source")
        .arg(&src_path)
        .arg("--terminal-transcript")
        .arg(&transcript_path)
        .current_dir(&repo)
        .output()
        .expect("run hybrid browser matrix");
    assert!(
        output.status.success(),
        "hybrid browser matrix failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("PASS hybrid dev-server browser matrix"),
        "browser matrix did not report completion"
    );
    assert_eq!(fs::read_to_string(&src_path).unwrap(), FIXTURE_SRC_EDITED);

    let before_ctrl_c = fs::read_to_string(&transcript_path).unwrap().len();
    input.write_all(&[3]).unwrap();
    input.flush().unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = guard.0.try_wait().unwrap() {
            assert_eq!(
                status.code(),
                Some(130),
                "browser PTY wrapper exit: {status}"
            );
            break;
        }
        if start.elapsed() > Duration::from_secs(5) {
            panic!("browser PTY wrapper did not exit after Ctrl-C");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let final_transcript = fs::read_to_string(&transcript_path).unwrap();
    assert!(
        final_transcript[before_ctrl_c..].contains("\x1b[r"),
        "Ctrl-C did not restore active browser-matrix scroll region: {final_transcript:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn jet_dev_web_exposes_canvas_panel_and_graph() {
    if !have_tool("rustc") {
        eprintln!("note: skipping jet_dev_web_exposes_canvas_panel_and_graph (need rustc)");
        return;
    }

    let dir = std::env::temp_dir().join(format!("jet_dev_canvas_{}", std::process::id()));
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

    struct KillOnDrop(std::process::Child);
    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let stdout = child.stdout.take().unwrap();
    let guard = KillOnDrop(child);
    let port = wait_for_ports(stdout).canvas;

    let (status, html) = http_get(port, "/canvas").expect("GET Canvas panel");
    assert_eq!(status, 200);
    let html = String::from_utf8_lossy(&html);
    assert!(html.contains("<canvas id=\"jet-canvas-view\""));
    assert!(html.contains("<canvas id=\"minimap\""));
    assert!(html.contains("id=\"graph-list\""));
    assert!(html.contains("id=\"project-panel\""));
    assert!(html.contains("id=\"project-rail\""));
    assert!(html.contains("id=\"project-mode\""));
    assert!(html.contains("id=\"output-panel\""));
    assert!(html.contains("id=\"output-list\""));
    assert!(html.contains("id=\"session-identity\""));
    assert!(html.contains("data-session-view=\"custom servers\""));
    assert!(html.contains("id=\"preview-panel\""));
    assert!(html.contains("id=\"variables-panel\""));
    assert!(html.contains("id=\"variables-list\""));
    assert!(html.contains("id=\"variable-count\""));
    assert!(html.contains("id=\"status-panel\""));
    assert!(html.contains("id=\"status-summary\""));
    assert!(html.contains("id=\"status-count\""));
    assert!(html.contains("id=\"package-summary\""));
    assert!(html.contains("id=\"dependency-summary\""));
    assert!(html.contains("id=\"dev-summary\""));
    assert!(html.contains("id=\"diagnostics-summary\""));
    assert!(html.contains("id=\"trust-summary\""));
    assert!(html.contains("id=\"canvas-dock\""));
    assert!(html.contains("id=\"graph-strip\""));
    assert!(html.contains("id=\"wire-status\""));
    assert!(html.contains("id=\"graph-overview\""));
    assert!(html.contains("id=\"left-drawer\""));
    assert!(html.contains("id=\"right-drawer\""));
    assert!(html.contains("id=\"dock-graphs\""));
    assert!(html.contains("id=\"dock-details\""));
    assert!(html.contains("id=\"details\""));
    assert!(html.contains("id=\"undo-edit\""));
    assert!(html.contains("id=\"redo-edit\""));
    assert!(html.contains("id=\"org-align\""));
    assert!(html.contains("id=\"org-tidy\""));
    assert!(html.contains("id=\"bookmark-add\""));
    assert!(html.contains("id=\"bookmark-jump\""));
    assert!(html.contains("id=\"core-catalog\""));
    assert!(html.contains("id=\"favorite-action\""));
    assert!(html.contains("id=\"run-current\""));
    assert!(html.contains("id=\"run-hud\""));
    assert!(html.contains("id=\"first-run-tour\""));
    assert!(html.contains("id=\"tour-dismiss\""));
    assert!(html.contains("id=\"graph-back\""));
    assert!(html.contains("id=\"graph-forward\""));
    assert!(html.contains("id=\"source-diff\""));
    assert!(html.contains("id=\"review-view\""));
    assert!(html.contains("id=\"review-view-button\""));
    assert!(html.contains("id=\"review-file-list\""));
    assert!(html.contains("id=\"review-content\""));
    assert!(html.contains("id=\"review-refresh\""));
    assert!(html.contains("id=\"edit-source\""));
    assert!(html.contains("id=\"apply-source-edit\""));
    assert!(html.contains("id=\"cancel-source-edit\""));
    assert!(html.contains("id=\"view-toggle\""));
    assert!(html.contains("id=\"lens-switch\""));
    assert!(html.contains("id=\"view-code\""));
    assert!(html.contains("id=\"view-split\""));
    assert!(html.contains("id=\"view-graph\""));
    assert!(html.contains("id=\"detail-toggles\""));
    assert!(html.contains("data-detail-toggle=\"types\""));
    assert!(html.contains("data-detail-toggle=\"diagnostics\""));
    assert!(html.contains("data-detail-toggle=\"effects\""));
    assert!(html.contains("data-detail-toggle=\"debug\""));
    assert!(html.contains("data-detail-toggle=\"package\""));
    assert!(html.contains("id=\"developer-mode\""));
    assert!(html.contains("id=\"toolbar-search\""));
    assert!(html.contains("id=\"toolbar-zoom\""));
    assert!(html.contains("class=\"toolbar-group\""));
    assert!(html.contains("class=\"toolbar-menu\""));
    assert!(html.contains("class=\"icon-button\""));
    assert!(html.contains("<svg viewBox=\"0 0 24 24\""));
    assert!(html.contains("id=\"source-view\""));
    assert!(html.contains("id=\"source-editor\""));
    assert!(html.contains("id=\"context-menu\""));
    assert!(html.contains("grid-template-rows: auto minmax(0, 1fr) 28px"));
    assert!(html.contains("flex-wrap: nowrap"));
    assert!(html
        .contains("grid-template-columns: minmax(156px, 15vw) minmax(0, 1fr) minmax(238px, 20vw)"));
    assert!(html.contains("@media (max-width: 900px)"));
    assert!(html.contains(".side.is-drawer-open"));
    assert!(html.contains("grid-template-columns: minmax(0, 1fr)"));
    assert!(html.contains("canvas_ui=blueprint23"));
    assert!(html.contains("rel=\"icon\" href=\"data:image/svg+xml"));
    assert!(html.contains("body:not(.is-dev-mode) #graph-strip { display: none; }"));
    assert!(html.contains("body:not(.is-dev-mode) #wire-status { display: none; }"));
    assert!(html.contains("body:not(.is-dev-mode) .dev-only"));
    assert!(html.contains("body.is-debug-active .debug-controls { display: flex; }"));
    assert!(html.contains(
        "body:not(.is-debug-active) #debug-menu[open] .debug-controls { display: grid; }"
    ));
    assert!(html.contains("id=\"debug-start\""));
    assert!(html.contains("No live session"));
    assert!(html.contains("body:not(.is-dev-mode) #jump { display: none; }"));
    assert!(html.contains("@media (prefers-reduced-motion: reduce)"));
    assert!(html.contains("#run-hud.is-running"));
    assert!(html.contains("#first-run-tour.is-open"));
    assert!(html.contains("id=\"graph-count\""));
    assert!(html.contains("<summary><span>Functions</span>"));
    assert!(html.contains("<h2>My Canvas</h2>"));
    assert!(html.contains("<summary><span>Files</span>"));
    assert!(html.contains(".project-section"));
    assert!(html.contains(".variable-item"));
    assert!(html.contains(".status-card"));
    assert!(html.contains(".lens-switch"));
    assert!(html.contains(".detail-toggles"));
    assert!(html.contains(".type-detail"));
    assert!(html.contains(".pin-port.is-exec"));
    assert!(html.contains(".pin-port.is-fallible"));
    assert!(html.contains("#stage.is-code"));
    assert!(html.contains("#stage.is-split"));
    assert!(html.contains("#graph-overview { display: none; }"));
    assert!(html.contains(".graph-overview-title"));
    assert!(html.contains(".graph-stat"));
    assert!(html.contains(".graph-tab-title"));
    assert!(html.contains(".graph-tab-count"));
    assert!(html.contains(".details-hero"));
    assert!(html.contains(".pin-card"));
    assert!(html.contains("id=\"scm-state\""));
    assert!(html.contains("id=\"canvas-search\""));
    assert!(html.contains("id=\"search-results\""));
    assert!(!html.contains("id=\"palette-list\""));
    assert!(!html.contains("id=\"node-ribbon\""));
    assert!(!html.contains("id=\"node-action-strip\""));
    assert!(!html.contains("id=\"dock-palette\""));
    assert!(!html.contains("id=\"palette-panel\""));
    assert!(html.contains("id=\"proof-panel\""));
    assert!(html.contains("id=\"proof-rail\""));
    assert!(html.contains("id=\"proof-state\""));
    assert!(html.contains("/canvas/app.js"));
    assert!(!html.contains("source-truth"));
    assert!(!html.contains("Source truth"));
    assert!(!html.contains(">Trust<"));

    let (session_status, session_body) =
        http_get(port, "/canvas/session?client_id=canvas-a").expect("GET Canvas session");
    assert_eq!(session_status, 200);
    let session = String::from_utf8_lossy(&session_body);
    assert!(session.contains("\"protocol\":\"jet.canvas.session\""));
    assert!(session.contains("\"accepted_revision\""));
    assert!(session.contains("\"last_good_program\":\"web-build-"));
    assert!(session.contains("\"transport\":\"canvas\""));
    assert!(session.contains("\"transport\":\"application\""));
    let application_port = json_number_after(
        &session,
        "\"application\":{\"host\":\"127.0.0.1\",\"port\":",
    );
    assert_eq!(
        application_port, port,
        "the workbench serves Canvas and app preview on one listener (card #2170)"
    );
    let (app_status, _) = http_get(application_port, "/").expect("GET app preview listener");
    assert_eq!(app_status, 200);
    let (_, second_session_body) =
        http_get(port, "/canvas/session?client_id=canvas-b").expect("GET second Canvas client");
    let second_session = String::from_utf8_lossy(&second_session_body);
    assert!(second_session.contains("\"clients\":2"));

    let (status, js) = http_get(port, "/canvas/app.js").expect("GET Canvas JS");
    assert_eq!(status, 200);
    let js = String::from_utf8_lossy(&js);
    assert!(js.contains("window.__jetCanvasNonblankPixels"));
    assert!(js.contains("window.__jetCanvasHitMap"));
    assert!(js.contains("window.__jetCanvasPinPoints"));
    assert!(js.contains("ctx.fillRect"));
    assert!(js.contains("const graphUrl"));
    assert!(js.contains("fetch(graphRequestUrl"));
    assert!(js.contains("latestProject"));
    assert!(js.contains("function loadProject"));
    assert!(js.contains("function loadCanvasSession"));
    assert!(js.contains("function syncCanvasOutputs"));
    assert!(js.contains("function selectCanvasOutput"));
    assert!(js.contains("canvasSelectedOutput"));
    assert!(js.contains("acceptedRevision"));
    assert!(js.contains("lastGoodProgram"));
    assert!(js.contains("last-good graph retained"));
    assert!(js.contains("__jetCanvasLastGoodGraph"));
    assert!(js.contains("function syncProjectRail"));
    assert!(js.contains("function syncVariablesList"));
    assert!(js.contains("function renderVariableDetails"));
    assert!(js.contains("function signatureWithVariable"));
    assert!(js.contains("__jetCanvasProjectRail"));
    assert!(js.contains("__jetCanvasVariablesSidebar"));
    assert!(js.contains("__jetCanvasWorkspacePanels"));
    assert!(js.contains("data-project-file"));
    assert!(js.contains("function graphRequestUrl"));
    assert!(js.contains("source_id="));
    assert!(js.contains("function fitGraph"));
    assert!(js.contains("Math.min(1.05"));
    assert!(js.contains("const layoutScale = { x: 1.08, y: 1.08 }"));
    assert!(js.contains("recordHit = true"));
    assert!(js.contains("drawNode(graph, node, inlineByNode, false)"));
    assert!(js.contains("function postTransaction"));
    assert!(js.contains("isError ? 10000 : 2200"));
    assert!(js.contains("toast.addEventListener(\"click\""));
    assert!(html.contains("#toast.is-error"));
    assert!(html.contains("white-space: pre-wrap"));
    assert!(js.contains("const queryUrl"));
    assert!(js.contains("sourceControlUrl"));
    assert!(js.contains("loadSourceControl"));
    assert!(js.contains("showSourceDiff"));
    assert!(js.contains("function renderReview"));
    assert!(js.contains("function reviewParseDiff"));
    assert!(js.contains("function drawReviewGraphOverlay"));
    assert!(js.contains("deleted · no current span"));
    assert!(js.contains("unprojectable · no current graph span"));
    assert!(js.contains("proofUrl"));
    assert!(js.contains("loadProofRail"));
    assert!(js.contains("__JET_CANVAS_PROOF__"));
    assert!(js.contains("__jetCanvasProofRail"));
    assert!(js.contains("function postQuery"));
    assert!(js.contains("Search results are stale"));
    assert!(js.contains("searchState.stale = true"));
    assert!(js.contains("Search is offline"));
    assert!(js.contains("Search result is stale; search again for the current revision"));
    assert!(js.contains("loadCanvasActions"));
    assert!(js.contains("openCoreCatalogPalette"));
    assert!(js.contains("__JET_CANVAS_CORE_CATALOG__"));
    assert!(js.contains("preview_canvas_action"));
    assert!(js.contains("jet.canvas.action"));
    assert!(js.contains("checked-tir+jit"));
    assert!(js.contains("source_to_graph"));
    assert!(js.contains("function setSourceHash"));
    assert!(js.contains("window.history.replaceState"));
    assert!(js.contains("preview_rename"));
    assert!(js.contains("find-references"));
    assert!(js.contains("rename_function"));
    assert!(js.contains("edit_function_signature"));
    assert!(js.contains("create_function"));
    assert!(js.contains("Apply signature"));
    assert!(js.contains("signature-board"));
    assert!(js.contains("pin-editor-row"));
    assert!(js.contains("functionParamRow"));
    assert!(js.contains("function pinCardHtml"));
    assert!(js.contains("pinPortHtml"));
    assert!(js.contains("function-return-type"));
    assert!(js.contains("add-function-output"));
    assert!(js.contains("set-function-output"));
    assert!(js.contains("remove-function-output"));
    assert!(js.contains("handleFunctionPinButton"));
    assert!(js.contains("button.id === \"remove-function-output\" ? \"Void\""));
    assert!(js.contains("function applyFunctionPins"));
    assert!(js.contains("function nextParamName"));
    assert!(js.contains("function syncReturnEditorPreview"));
    assert!(js.contains("function-return-type-chip"));
    assert!(js.contains("Output pin ready"));
    assert!(js.contains("<h2>Events</h2>"));
    assert!(js.contains("undoStack"));
    assert!(js.contains("redoStack"));
    assert!(js.contains("const UNDO_DEPTH = 50"));
    assert!(js.contains("function recordUndoEntry"));
    assert!(js.contains("restoreSource(entry.before, entry, null, \"Undo\")"));
    assert!(js.contains("restoreSource(entry.after, null, entry, \"Redo\")"));
    assert!(js.contains("editorState"));
    assert!(js.contains("jet.canvas.editor:"));
    assert!(js.contains("function alignSelectedNodes"));
    assert!(js.contains("function tidyGraphLayout"));
    assert!(js.contains("function addRerouteKnot"));
    assert!(js.contains("function bookmarkCurrentGraph"));
    assert!(js.contains("function jumpBookmark"));
    assert!(js.contains("function toggleFavoriteAction"));
    assert!(js.contains("function runCurrentGraph"));
    assert!(js.contains("function renderCommandAuthority"));
    assert!(js.contains("executeCommandAuthority"));
    assert!(js.contains("commandUrl"));
    assert!(js.contains("__JET_CANVAS_COMMAND__"));
    assert!(js.contains("__jetCanvasRunLoop"));
    assert!(js.contains("authority_required"));
    assert!(!js.contains("last run projected into debug overlay"));
    assert!(js.contains("function visibleGraphNodes"));
    assert!(js.contains("__jetCanvasVirtualizationStats"));
    assert!(js.contains("wireStyle = \"bezier\""));
    assert!(!js.contains("wireStyle === \"straight\""));
    assert!(js.contains("view.zoom < .38"));
    assert!(js.contains("function typeExplanation"));
    assert!(js.contains("Favorite pinned"));
    assert!(js.contains("Bookmark saved"));
    assert!(js.contains("Graph tidied"));
    assert!(js.contains("replace_source"));
    assert!(js.contains("insert_branch"));
    assert!(js.contains("insert_switch"));
    assert!(js.contains("insert_loop"));
    assert!(js.contains("insert_fallible_rail"));
    assert!(js.contains("insert_call"));
    assert!(!js.contains("Insert call transaction"));
    assert!(js.contains("selectedNodeIds"));
    assert!(js.contains("graphBackStack"));
    assert!(js.contains("graphForwardStack"));
    assert!(js.contains("function switchGraph"));
    assert!(js.contains("function updateGraphNav"));
    assert!(js.contains("Back to "));
    assert!(js.contains("Forward to "));
    assert!(js.contains("mode: \"marquee\""));
    assert!(js.contains("mode: \"node\""));
    assert!(js.contains("contextmenu"));
    assert!(js.contains("function renderActionPalette"));
    assert!(js.contains("data-unavailable-reason-code"));
    assert!(js.contains("aria-disabled"));
    assert!(js.contains("Needs a fallible function."));
    assert!(js.contains("action-palette-search"));
    assert!(js.contains("Canvas actions"));
    assert!(js.contains("All nodes · ${matches.length}/${contextMenuState.actions.length}"));
    assert!(!js.contains("right-click built-ins, functions, source actions"));
    assert!(js.contains("ArrowRight"));
    assert!(js.contains("function graphActionItems"));
    assert!(js.contains("function openGraphActionPalette"));
    assert!(js.contains("Align top"));
    assert!(js.contains("Auto tidy"));
    assert!(js.contains("Add reroute knot"));
    assert!(js.contains("Bookmark graph"));
    assert!(js.contains("Run graph"));
    assert!(!js.contains("paletteSearch.focus"));
    assert!(js.contains("__jetCanvasPinAuthoring"));
    assert!(js.contains("compatiblePin"));
    assert!(js.contains("function connectionPlan"));
    assert!(js.contains("function completeConnection"));
    assert!(js.contains("pendingPin"));
    assert!(js.contains("function setPendingPin"));
    assert!(js.contains("__jetCanvasPendingPin"));
    assert!(js.contains("__jetCanvasFrontendFamily"));
    assert!(js.contains("family: \"modified_hybrid\""));
    assert!(js.contains("codeSplitGraphLens: true"));
    assert!(js.contains("workbenchProjectViewer: true"));
    assert!(js.contains("dragPinCompatibleMenu: true"));
    assert!(js.contains("hoverOnlyTypes: true"));
    assert!(js.contains("getterCapsules: true"));
    assert!(js.contains("embeddedVariables: true"));
    assert!(js.contains("graphiteDetailToggles"));
    assert!(js.contains("detailToggles"));
    assert!(js.contains("function syncDetailToggles"));
    assert!(js.contains("data-detail-toggle"));
    assert!(js.contains("data-view-mode"));
    assert!(js.contains("__jetCanvasLensMode"));
    assert!(js.contains("Select destination pin"));
    assert!(js.contains("function drawCompatibleDropTargets"));
    assert!(js.contains("function drawConnectionBadge"));
    assert!(js.contains("function syncWireStatus"));
    assert!(!js.contains("function syncNodeRibbon"));
    assert!(!js.contains("const toolColors"));
    assert!(!js.contains("nodeToolGlyph"));
    assert!(js.contains("__jetCanvasLastConnectionPlan"));
    assert!(js.contains("const hitR = Math.max(12, 18 * view.zoom)"));
    assert!(js.contains("bestDistance"));
    assert!(js.contains("Drop on an input pin"));
    assert!(js.contains("Type mismatch"));
    assert!(js.contains("compatibleActionType"));
    assert!(js.contains("const TYPE_COLOR_MAP"));
    assert!(js.contains("Bool: \"#c0392b\""));
    assert!(js.contains("Int: \"#2ec4b6\""));
    assert!(js.contains("Float: \"#9acd32\""));
    assert!(js.contains("String: \"#c678dd\""));
    assert!(js.contains("Void: \"#6b7280\""));
    assert!(js.contains("const NODE_ARCHETYPE_STYLES"));
    assert!(js.contains("function drawArchetypeHeader"));
    assert!(js.contains("function nodeStyle"));
    assert!(js.contains("function nodeKindLabel"));
    assert!(js.contains("function shouldDrawNodeBadge"));
    assert!(js.contains("function nodeSize"));
    assert!(js.contains("function drawTypeLegend"));
    assert!(js.contains("if (!detailToggles.types || compactCanvasMode()"));
    assert!(js.contains("[\"Bool\", \"Bool\"]"));
    assert!(js.contains("developerMode && !!node && !!node.kind"));
    assert!(!js.contains("nodeKindLabel(node, graph).toUpperCase()"));
    assert!(!js.contains("node.kind === \"constant\" ? \"LIT\" : \"GET\""));
    assert!(!js.contains("style.glyph, x + 33"));
    assert!(js.contains("function reflowGraph"));
    assert!(js.contains("autoNodeOffsets"));
    assert!(js.contains("Math.max(.42"));
    assert!(js.contains(
        "const topInset = compact ? (developerMode ? 154 : 52) : (developerMode ? 108 : 32)"
    ));
    assert!(js.contains("const leftInset = 22"));
    assert!(js.contains("requestAnimationFrame(fitGraph)"));
    assert!(js.contains("function compactCanvasMode"));
    assert!(js.contains("function setDrawer"));
    assert!(!js.contains("dockPalette.addEventListener"));
    assert!(js.contains("is-drawer-open"));
    assert!(js.contains("function syncGraphStrip"));
    assert!(js.contains("function syncGraphOverview"));
    assert!(js.contains("data-sidebar-graph"));
    assert!(js.contains("function nodeContextActions"));
    assert!(js.contains("__jetCanvasGraphOverview"));
    assert!(js.contains("__jetCanvasGraphSwitcherReady"));
    assert!(js.contains("__jetCanvasSelectedGraphId"));
    assert!(js.contains("data-graph-tab"));
    assert!(js.contains("button.type = \"button\""));
    assert!(js.contains("ev.stopPropagation()"));
    assert!(js.contains("graph-tab"));
    assert!(js.contains("function isExecPin"));
    assert!(js.contains("function pinName"));
    assert!(js.contains("function exactPinType"));
    assert!(js.contains("function drawPinLabel"));
    assert!(js.contains("function drawPinHoverTooltip"));
    assert!(js.contains("drawPinHoverTooltip(hoverPin)"));
    assert!(js.contains("function drawSocketRow"));
    assert!(js.contains("function drawPinDefaultEditor"));
    assert!(js.contains("function drawInlineExprChip"));
    assert!(js.contains("function isLiteralDefault"));
    assert!(js.contains("function applyPinDefaultEditor"));
    assert!(js.contains("__jetCanvasPinDefaultEditors"));
    assert!(js.contains("__jetCanvasInlineExprChips"));
    assert!(js.contains("op: \"edit_inline_expr\""));
    assert!(js.contains("connectedPinIds.has(pin.pin_id)"));
    assert!(!js.contains("function drawLaneTag"));
    assert!(js.contains("function drawWireTypeBadge"));
    assert!(js.contains("function drawWireArrow"));
    assert!(js.contains("function drawWire"));
    assert!(js.contains("function bezierPoint"));
    assert!(js.contains("function bezierControls"));
    assert!(js.contains("ctx.lineTo(x + r * 1.25, y)"));
    assert!(js.contains("selectedWire"));
    assert!(js.contains("const NODE_HEADER_H = 26"));
    assert!(js.contains("function measureNodeLayout"));
    assert!(js.contains("ctx.measureText"));
    assert!(js.contains("__jetCanvasMeasuredNodeSizing"));
    assert!(js.contains("pin.pattern_source"));
    assert!(js.contains("contextMenuOpenedAt"));
    assert!(js.contains("drag = { mode: \"pin\", pin: endpoint.pin"));
    assert!(!js.contains("node.kind === \"branch\" || node.kind === \"dispatch\" ? 296 : 232"));
    assert!(js.contains("function graphForFunctionName"));
    assert!(js.contains("function openFunctionGraph"));
    assert!(js.contains("Open function"));
    assert!(js.contains("open-callee-graph"));
    assert!(js.contains("variable_get"));
    assert!(js.contains("constant"));
    assert!(js.contains("Set variable"));
    assert!(js.contains("__jetCanvasBindingTypeAccent"));
    assert!(js.contains("label: \"Value\""));
    assert!(js.contains("label: \"Literal\""));
    assert!(js.contains("function isGetterCapsule"));
    assert!(js.contains("function simpleEmbeddedValue"));
    assert!(js.contains("__jetCanvasGetterCapsules"));
    assert!(js.contains("__jetCanvasEmbeddedVariables"));
    assert!(!js.contains("Refused: E0204"));
    assert!(js.contains("functionsForPin"));
    assert!(js.contains("function actionInsertsNode"));
    assert!(js.contains("openPinMenu"));
    assert!(js.contains("Create function accepting"));
    assert!(js.contains("function paletteCategoryForAction"));
    assert!(js.contains("function paletteActionGlyph"));
    assert!(js.contains("function variableActionsForGraph"));
    assert!(js.contains("title: name"));
    assert!(!js.contains("title: \"get \" + name"));
    assert!(js.contains("function loadCoreCatalogActions"));
    assert!(js.contains("__jetCanvasVariablePalette"));
    assert!(js.contains("__jetCanvasCoreCatalogPalette"));
    assert!(js.contains("category === \"Core\" ? 1000"));
    assert!(js.contains("module_path: action.module_path"));
    assert!(js.contains("kind: \"variable_get\""));
    assert!(js.contains("kind: \"variable_set\""));
    assert!(js.contains("\"Add connected node\""));
    assert!(js.contains("op: \"insert_call\""));
    assert!(js.contains("const callee = String(module.path || \"core\") + \".\" + member.name"));
    assert!(js.contains("default_args: member.default_args || [\"1\"]"));
    assert!(!js.contains("<span class=\"tag\">${escapeHtml(category)}</span>"));
    assert!(js.contains("function defaultArgsForAction"));
    assert!(js.contains("if (!inputs.length) return existing"));
    assert!(js.contains("function fuzzyScoreText"));
    assert!(js.contains("__jetCanvasFuzzyScore"));
    assert!(js.contains("actionFuzzyScore(action, query)"));
    assert!(js.contains("function restoreNodePositions"));
    assert!(js.contains("function rememberSelectedNodePositions"));
    assert!(js.contains("if (hasSavedNodePositions(graph)) return"));
    assert!(js.contains("if (drag && drag.mode === \"node\") return"));
    assert!(js.contains("function hitWireEndpointAt"));
    assert!(js.contains("wireEndpointHit.push"));
    assert!(js.contains("wire_inline_expr_id"));
    assert!(js.contains("wire_origin_pin_id"));
    assert!(js.contains("wire_target_pin"));
    assert!(js.contains("const projectFunctions = (doc.project_functions || [])"));
    assert!(js.contains("project_function"));
    assert!(js.contains("action-category"));
    assert!(js.contains("move_link"));
    assert!(js.contains("viewMode"));
    assert!(js.contains("setViewMode"));
    assert!(js.contains("setSourceEditMode"));
    assert!(js.contains("applySourceEditBuffer"));
    assert!(js.contains("source_edit: true"));
    assert!(js.contains("function setDeveloperMode"));
    assert!(js.contains("storedFlag(\"jet.canvas.developerMode\")"));
    assert!(js.contains("developerModeButton.addEventListener"));
    assert!(js.contains("document.body.classList.toggle(\"is-dev-mode\""));
    assert!(js.contains("toolbarSearch.addEventListener"));
    assert!(js.contains("document.body.classList.toggle(\"is-debug-active\""));
    assert!(js.contains("Apply pins"));
    assert!(
        html.contains("grid-template-rows: minmax(0, 1fr) auto"),
        "{html}"
    );
    assert!(
        html.contains("#details { min-height: 0; overflow: auto;"),
        "{html}"
    );
    assert!(js.contains("Visible conversion function"));
    assert!(js.contains("Wire refused"));
    assert!(js.contains("promote_to_binding"));
    assert!(js.contains("insert_visible_conversion"));
    assert!(js.contains("Promote to binding"));
    assert!(js.contains("__jetCanvasDebugOverlay"));
    assert!(js.contains("debugUrl"));
    assert!(js.contains("session_id"));
    assert!(js.contains("Debug session disconnected; source was kept"));
    assert!(js.contains("breakpoint_spans"));
    assert!(js.contains("active_node_id"));
    assert!(js.contains("Debug overlay stopped"));
    assert!(js.contains("create_comment_region"));
    assert!(js.contains("edit_comment_region"));
    assert!(js.contains("Apply comment"));
    assert!(js.contains("preview_extract_inline_expr"));
    assert!(js.contains("extract_inline_expr"));
    assert!(js.contains("Extract function"));
    assert!(js.contains("function createStagedNodeFromAction"));
    assert!(js.contains("__jetCanvasStagedNodeVisuals"));
    assert!(js.contains("not saved"));
    assert!(js.contains("function materializeStagedConnection"));
    assert!(js.contains("__jetCanvasStagedMaterialization = \"direct-staged-to-real\""));
    assert!(js.contains("Connect staged nodes to a saved pin first"));
    assert!(js.contains("function createCommentBox"));
    assert!(js.contains("COMMENT_TINTS"));
    assert!(js.contains("function graphCommentBoxes"));
    assert!(js.contains("function copySelection"));
    assert!(js.contains("function pasteSelection"));
    assert!(js.contains("function duplicateSelection"));
    assert!(js.contains("paste_clone"));
    assert!(js.contains("source_edit: \"paste_clone\""));
    assert!(js.contains("ev.key.toLowerCase() === \"c\""));
    assert!(js.contains("ev.key.toLowerCase() === \"v\""));
    assert!(js.contains("ev.key.toLowerCase() === \"d\""));
    assert!(!js.contains("debug-only"));
    assert!(!js.contains("dev-only marker"));
    assert!(!html.contains("release a socket"));

    let (status, graph) = http_get(port, "/canvas/graph").expect("GET Canvas graph");
    assert_eq!(status, 200);
    let graph = String::from_utf8_lossy(&graph);
    assert!(graph.contains("\"protocol\":\"jet.canvas.graph\""));
    assert!(graph.contains("\"schema_version\":1"));
    assert!(graph.contains("\"nodes\""));
    assert!(graph.contains("\"pins\""));
    assert!(graph.contains("\"layout_hints\""));
    assert!(graph.contains("\"source_span\""));
    assert!(graph.contains("\"source_text\""));

    let src = fs::read_to_string(&src_path).unwrap();
    let revision = jet::Canvas::source_revision(&src);
    let req = format!(
        "{{\"schema_version\":1,\"op\":\"noop\",\"revision\":\"{}\"}}",
        revision
    );
    let (status, body) = http_post(port, "/canvas/transaction", &req).expect("POST Canvas noop");
    assert_eq!(status, 200);
    assert!(String::from_utf8_lossy(&body).contains("\"protocol\":\"jet.canvas.edit\""));

    let query = format!(
        "{{\"schema_version\":1,\"op\":\"find\",\"revision\":\"{}\",\"query\":\"run\"}}",
        revision
    );
    let (status, body) = http_post(port, "/canvas/query", &query).expect("POST Canvas query");
    assert_eq!(status, 200);
    assert!(String::from_utf8_lossy(&body).contains("\"protocol\":\"jet.canvas.query\""));

    let (status, body) =
        http_get(port, "/canvas/source-control").expect("GET Canvas source control");
    assert_eq!(status, 200);
    assert!(String::from_utf8_lossy(&body).contains("\"protocol\":\"jet.canvas.source_control\""));

    let (status, body) = http_get(port, "/canvas/proof").expect("GET Canvas proof");
    assert_eq!(status, 200);
    let proof = String::from_utf8_lossy(&body);
    assert!(
        proof.contains("\"protocol\":\"jet.canvas.proof\""),
        "{proof}"
    );
    assert!(proof.contains("\"schema_version\":1"), "{proof}");
    assert!(proof.contains("\"check\":{\"state\":\"ok\""), "{proof}");
    assert!(
        proof.contains("\"command_receipts\":{\"state\":\"missing\""),
        "{proof}"
    );

    let command_req = format!(
        "{{\"schema_version\":1,\"action_id\":\"canvas.command:check\",\"revision\":\"{}\"}}",
        revision
    );
    let (status, receipt) =
        http_post(port, "/canvas/command", &command_req).expect("POST Canvas command");
    let receipt = String::from_utf8_lossy(&receipt);
    assert_eq!(status, 200, "command receipt response: {receipt}");
    assert!(
        receipt.contains("\"protocol\":\"jet.canvas.command_receipt\""),
        "{receipt}"
    );
    assert!(
        receipt.contains("\"action_id\":\"canvas.command:check\""),
        "{receipt}"
    );
    assert!(
        receipt.contains("\"command\":[\"jet\",\"check\",\"app.jet\"]"),
        "{receipt}"
    );
    assert!(receipt.contains("\"success\":true"), "{receipt}");

    let (status, body) = http_get(port, "/canvas/proof").expect("GET Canvas proof after command");
    assert_eq!(status, 200);
    let proof = String::from_utf8_lossy(&body);
    assert!(
        proof.contains("\"command_receipts\":{\"state\":\"current\""),
        "{proof}"
    );
    assert!(
        proof.contains("\"protocol\":\"jet.canvas.command_receipt\""),
        "{proof}"
    );
    assert!(
        proof.contains("\"proof\":{\"state\":\"current\",\"stale\":false"),
        "{proof}"
    );

    fs::write(
        dir.join("package.jet"),
        "name: \"canvas_app\"\nversion: \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        dir.join("helper.jet"),
        "fn run() {\n    a := 1\n    b := a + 1\n    c := b + 1\n    print(\"helper\")\n}\n",
    )
    .unwrap();
    let (status, project) = http_get(port, "/canvas/project").expect("GET Canvas project");
    assert_eq!(status, 200);
    let project = String::from_utf8_lossy(&project);
    assert!(project.contains("\"protocol\":\"jet.canvas.project\""));
    assert!(project.contains("\"schema_version\":1"));
    assert!(project.contains("\"capabilities\":{"));
    assert!(project.contains("\"graph\":true"));
    assert!(project.contains("\"code\":true"));
    assert!(project.contains("\"diagnostics\":true"));
    assert!(project.contains("\"runtime_output\":true"));
    assert!(project.contains("\"project_revision\":\"sha256-"));
    assert!(project.contains("\"entry\""));
    assert!(project.contains("\"state_policy\""));

    let (status, helper_graph) =
        http_get(port, "/canvas/graph?source_id=helper.jet").expect("GET helper graph");
    assert_eq!(
        status,
        200,
        "helper graph response: {}",
        String::from_utf8_lossy(&helper_graph)
    );
    let helper_graph = String::from_utf8_lossy(&helper_graph);
    assert!(helper_graph.contains("helper.jet"), "{helper_graph}");
    assert!(
        helper_graph.contains("print(\\\"helper\\\")"),
        "{helper_graph}"
    );

    let (status, helper_proof) =
        http_get(port, "/canvas/proof?source_id=helper.jet").expect("GET helper proof");
    assert_eq!(status, 200);
    let helper_proof = String::from_utf8_lossy(&helper_proof);
    assert!(
        helper_proof.contains("\"protocol\":\"jet.canvas.proof\""),
        "{helper_proof}"
    );
    assert!(
        helper_proof.contains("\"source_id\":\"helper.jet\""),
        "{helper_proof}"
    );

    let helper_src = fs::read_to_string(dir.join("helper.jet")).unwrap();
    let helper_revision = jet::Canvas::source_revision(&helper_src);
    let helper_command = format!(
        "{{\"schema_version\":1,\"action_id\":\"canvas.command:check\",\"source_id\":\"helper.jet\",\"revision\":\"{}\"}}",
        helper_revision
    );
    let (status, helper_receipt) =
        http_post(port, "/canvas/command", &helper_command).expect("POST helper command");
    assert_eq!(status, 200);
    let helper_receipt = String::from_utf8_lossy(&helper_receipt);
    assert!(
        helper_receipt.contains("\"source_id\":\"helper.jet\""),
        "{helper_receipt}"
    );
    assert!(
        helper_receipt.contains("\"command\":[\"jet\",\"check\",\"helper.jet\"]"),
        "{helper_receipt}"
    );
    let helper_debug = format!(
        "{{\"schema_version\":1,\"source_id\":\"helper.jet\",\"revision\":\"{}\",\"commands\":[\"s\"]}}",
        helper_revision
    );
    let (status, helper_debug_body) =
        http_post(port, "/canvas/debug", &helper_debug).expect("POST helper debug");
    assert_eq!(status, 200);
    let helper_debug_body = String::from_utf8_lossy(&helper_debug_body);
    assert!(
        helper_debug_body.contains("\"protocol\":\"jet.canvas.debug\""),
        "{helper_debug_body}"
    );
    assert!(
        helper_debug_body.contains(&format!("\"revision\":\"{}\"", helper_revision)),
        "{helper_debug_body}"
    );
    assert!(
        helper_debug_body.contains("\"state\":\"running\""),
        "{helper_debug_body}"
    );
    assert!(
        helper_debug_body.contains("\"active_line\":"),
        "{helper_debug_body}"
    );
    assert!(
        !helper_debug_body.contains("\"active_line\":null"),
        "{helper_debug_body}"
    );
    let helper_session = json_field(&helper_debug_body, "id");
    let helper_debug_next = format!(
        "{{\"schema_version\":1,\"source_id\":\"helper.jet\",\"revision\":\"{}\",\"session_id\":\"{}\",\"commands\":[\"s\"]}}",
        helper_revision, helper_session
    );
    let (status, helper_debug_next_body) =
        http_post(port, "/canvas/debug", &helper_debug_next).expect("POST helper debug next");
    assert_eq!(status, 200);
    let helper_debug_next_body = String::from_utf8_lossy(&helper_debug_next_body);
    assert!(
        helper_debug_next_body.contains("\"state\":\"running\""),
        "{helper_debug_next_body}"
    );
    assert!(
        !helper_debug_next_body.contains("\"active_line\":null"),
        "{helper_debug_next_body}"
    );
    let helper_debug_finish = format!(
        "{{\"schema_version\":1,\"source_id\":\"helper.jet\",\"revision\":\"{}\",\"session_id\":\"{}\",\"commands\":[\"c\"]}}",
        helper_revision, helper_session
    );
    let (status, helper_debug_finish_body) =
        http_post(port, "/canvas/debug", &helper_debug_finish).expect("POST helper debug finish");
    assert_eq!(status, 200);
    let helper_debug_finish_body = String::from_utf8_lossy(&helper_debug_finish_body);
    assert!(
        helper_debug_finish_body.contains("\"state\":\"finished\""),
        "{helper_debug_finish_body}"
    );
    assert!(
        helper_debug_finish_body.contains("\"active_line\":null"),
        "{helper_debug_finish_body}"
    );
    let stale_breakpoint = format!(
        "{{\"schema_version\":1,\"source_id\":\"helper.jet\",\"revision\":\"{}\",\"breakpoint_spans\":[\"999999:1000000\"],\"commands\":[\"c\"]}}",
        helper_revision
    );
    let (status, stale_breakpoint_body) =
        http_post(port, "/canvas/debug", &stale_breakpoint).expect("POST stale breakpoint");
    assert_eq!(status, 200);
    let stale_breakpoint_body = String::from_utf8_lossy(&stale_breakpoint_body);
    assert!(
        stale_breakpoint_body.contains("\"state\":\"stale\""),
        "{stale_breakpoint_body}"
    );
    let stale_debug =
        "{\"schema_version\":1,\"source_id\":\"helper.jet\",\"revision\":\"sha256-stale\",\"commands\":[\"s\"]}";
    let (status, stale_debug_body) =
        http_post(port, "/canvas/debug", stale_debug).expect("POST stale helper debug");
    assert_eq!(status, 409);
    let stale_debug_body = String::from_utf8_lossy(&stale_debug_body);
    assert!(
        stale_debug_body.contains("\"kind\":\"conflict\""),
        "{stale_debug_body}"
    );
    let ended_debug = format!(
        "{{\"schema_version\":1,\"source_id\":\"helper.jet\",\"revision\":\"{}\",\"session_id\":\"{}\",\"commands\":[\"s\"]}}",
        helper_revision, helper_session
    );
    let (status, ended_debug_body) =
        http_post(port, "/canvas/debug", &ended_debug).expect("POST ended helper debug");
    assert_eq!(status, 409);
    let ended_debug_body = String::from_utf8_lossy(&ended_debug_body);
    assert!(
        ended_debug_body.contains("\"kind\":\"session\""),
        "{ended_debug_body}"
    );
    let helper_query = format!(
        "{{\"schema_version\":1,\"op\":\"find\",\"source_id\":\"helper.jet\",\"revision\":\"{}\",\"query\":\"run\"}}",
        helper_revision
    );
    let (status, helper_query_body) =
        http_post(port, "/canvas/query", &helper_query).expect("POST helper query");
    assert_eq!(status, 200);
    assert!(
        String::from_utf8_lossy(&helper_query_body).contains("\"protocol\":\"jet.canvas.query\"")
    );

    let helper_edit = format!(
        "{{\"schema_version\":1,\"op\":\"replace_source\",\"source_id\":\"helper.jet\",\"revision\":\"{}\",\"source\":\"fn run() {{\\n    print(\\\"edited helper\\\")\\n}}\\n\"}}",
        helper_revision
    );
    let (status, helper_edit_body) =
        http_post(port, "/canvas/transaction", &helper_edit).expect("POST helper transaction");
    assert_eq!(
        status,
        200,
        "helper transaction: {}",
        String::from_utf8_lossy(&helper_edit_body)
    );
    assert!(String::from_utf8_lossy(&helper_edit_body).contains("\"changed\":true"));
    assert!(fs::read_to_string(dir.join("helper.jet"))
        .unwrap()
        .contains("edited helper"));
    assert_eq!(fs::read_to_string(&src_path).unwrap(), FIXTURE_SRC);

    let (status, catalog) =
        http_get(port, "/canvas/core-catalog?query=http").expect("GET Canvas core catalog");
    assert_eq!(status, 200);
    let catalog = String::from_utf8_lossy(&catalog);
    assert!(catalog.contains("\"op\":\"core_catalog\""), "{catalog}");
    assert!(
        catalog.contains("\"catalog_schema_version\":1"),
        "{catalog}"
    );
    assert!(
        catalog.contains("\"authority\":[\"canvas.catalog:core.read\"]"),
        "{catalog}"
    );
    assert!(catalog.contains("\"path\":\"core.http\""), "{catalog}");
    assert!(
        catalog.contains("\"source\":\"docs/reference/core-library.md\""),
        "{catalog}"
    );

    let project_revision = json_field(&project, "project_revision");
    let manifest_src = fs::read_to_string(dir.join("package.jet")).unwrap();
    let manifest_revision = jet::Canvas::source_revision(&manifest_src);
    let project_req = format!(
        "{{\"schema_version\":1,\"op\":\"add_dependency\",\"preview\":true,\"project_revision\":\"{}\",\"files\":[{{\"path\":\"package.jet\",\"revision\":\"{}\"}}],\"manifest\":\"package.jet\",\"name\":\"logging\",\"spec\":\"0.1.0\"}}",
        project_revision, manifest_revision
    );
    let (status, body) =
        http_post(port, "/canvas/project/transaction", &project_req).expect("POST project tx");
    assert_eq!(status, 200);
    let body = String::from_utf8_lossy(&body);
    assert!(
        body.contains("\"protocol\":\"jet.canvas.project.edit\""),
        "{body}"
    );
    assert!(body.contains("\"writes\":\"preview_only\""), "{body}");
    assert!(body.contains("+    logging: \\\"0.1.0\\\","), "{body}");
    assert!(!fs::read_to_string(dir.join("package.jet"))
        .unwrap()
        .contains("logging"));

    drop(guard);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn jet_dev_web_live_canvas_debug_session_round_trip() {
    if !have_tool("rustc") {
        eprintln!("note: skipping jet_dev_web_live_canvas_debug_session_round_trip (need rustc)");
        return;
    }

    let dir = std::env::temp_dir().join(format!("jet_dev_canvas_debug_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let entry = dir.join("app.jet");
    let source =
        "fn run() {\n    a := 1\n    b := a + 1\n    c := b + 1\n    print(\"debug\")\n}\n";
    fs::write(&entry, source).unwrap();

    let mut child = Command::new(jet_bin())
        .args(["dev", "app.jet", "--target=web"])
        .current_dir(&dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start Canvas debug dev server");
    struct KillOnDrop(std::process::Child);
    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let stdout = child.stdout.take().unwrap();
    let guard = KillOnDrop(child);
    let port = wait_for_ports(stdout).canvas;
    let revision = jet::Canvas::source_revision(source);

    let first = format!(
        "{{\"schema_version\":1,\"revision\":\"{}\",\"commands\":[\"s\"]}}",
        revision
    );
    let (status, body) = http_post(port, "/canvas/debug", &first).expect("start Canvas debug");
    assert_eq!(
        status,
        200,
        "start response: {}",
        String::from_utf8_lossy(&body)
    );
    let body = String::from_utf8_lossy(&body);
    assert!(body.contains("\"protocol\":\"jet.canvas.debug\""), "{body}");
    assert!(body.contains("\"state\":\"running\""), "{body}");
    assert!(body.contains("\"tier\":\"jet-dev-interpreter\""), "{body}");
    assert!(!body.contains("\"active_line\":null"), "{body}");
    let session = debug_session_id(&body);

    let next = format!(
        "{{\"schema_version\":1,\"revision\":\"{}\",\"session_id\":\"{}\",\"commands\":[\"s\"]}}",
        revision, session
    );
    let (status, body) = http_post(port, "/canvas/debug", &next).expect("resume Canvas debug");
    assert_eq!(
        status,
        200,
        "resume response: {}",
        String::from_utf8_lossy(&body)
    );
    let body = String::from_utf8_lossy(&body);
    assert!(body.contains("\"state\":\"running\""), "{body}");
    assert!(!body.contains("\"active_line\":null"), "{body}");

    let invalid = format!(
        "{{\"schema_version\":1,\"revision\":\"{}\",\"session_id\":\"{}\",\"commands\":[\"teleport\"]}}",
        revision, session
    );
    let (status, body) =
        http_post(port, "/canvas/debug", &invalid).expect("reject invalid debug command");
    assert_eq!(
        status,
        409,
        "invalid response: {}",
        String::from_utf8_lossy(&body)
    );
    assert!(String::from_utf8_lossy(&body).contains("\"kind\":\"unsupported\""));

    let after_invalid = format!(
        "{{\"schema_version\":1,\"revision\":\"{}\",\"session_id\":\"{}\",\"commands\":[\"s\"]}}",
        revision, session
    );
    let (status, body) = http_post(port, "/canvas/debug", &after_invalid)
        .expect("resume after invalid debug command");
    assert_eq!(
        status,
        200,
        "resume response: {}",
        String::from_utf8_lossy(&body)
    );
    assert!(String::from_utf8_lossy(&body).contains("\"state\":\"running\""));

    let finish = format!(
        "{{\"schema_version\":1,\"revision\":\"{}\",\"session_id\":\"{}\",\"commands\":[\"c\"]}}",
        revision, session
    );
    let (status, body) = http_post(port, "/canvas/debug", &finish).expect("finish Canvas debug");
    assert_eq!(
        status,
        200,
        "finish response: {}",
        String::from_utf8_lossy(&body)
    );
    let body = String::from_utf8_lossy(&body);
    assert!(body.contains("\"state\":\"finished\""), "{body}");
    assert!(body.contains("\"active_line\":null"), "{body}");

    let stale_breakpoint = format!(
        "{{\"schema_version\":1,\"revision\":\"{}\",\"breakpoint_spans\":[\"999999:1000000\"],\"commands\":[\"c\"]}}",
        revision
    );
    let (status, body) =
        http_post(port, "/canvas/debug", &stale_breakpoint).expect("stale breakpoint");
    assert_eq!(
        status,
        200,
        "stale breakpoint response: {}",
        String::from_utf8_lossy(&body)
    );
    assert!(
        String::from_utf8_lossy(&body).contains("\"state\":\"stale\""),
        "{}",
        String::from_utf8_lossy(&body)
    );

    let stale_revision =
        "{\"schema_version\":1,\"revision\":\"sha256-stale\",\"commands\":[\"s\"]}";
    let (status, body) = http_post(port, "/canvas/debug", stale_revision).expect("stale revision");
    assert_eq!(
        status,
        409,
        "stale revision response: {}",
        String::from_utf8_lossy(&body)
    );
    assert!(String::from_utf8_lossy(&body).contains("\"kind\":\"conflict\""));

    let ended = format!(
        "{{\"schema_version\":1,\"revision\":\"{}\",\"session_id\":\"{}\",\"commands\":[\"s\"]}}",
        revision, session
    );
    let (status, body) = http_post(port, "/canvas/debug", &ended).expect("ended session");
    assert_eq!(
        status,
        409,
        "ended response: {}",
        String::from_utf8_lossy(&body)
    );
    assert!(String::from_utf8_lossy(&body).contains("\"kind\":\"session\""));
    assert_eq!(fs::read_to_string(&entry).unwrap(), source);

    drop(guard);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn jet_dev_web_project_queries_round_trip_and_reject_stale_revision() {
    if !have_tool("rustc") {
        eprintln!("note: skipping jet_dev_web_project_queries_round_trip_and_reject_stale_revision (need rustc)");
        return;
    }

    let dir = std::env::temp_dir().join(format!(
        "jet_dev_canvas_project_query_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let entry = dir.join("app.jet");
    fs::write(
        &entry,
        "fn helper() Int -> {\n    return 1\n}\n\nfn run() {\n    helper()\n}\n",
    )
    .unwrap();
    fs::write(
        dir.join("helper.jet"),
        "fn helper() Int -> {\n    return 2\n}\n\nfn use_helper() {\n    helper()\n}\n",
    )
    .unwrap();
    fs::write(
        dir.join("package.jet"),
        "name: \"canvas_project_query\"\nversion: \"0.1.0\"\n",
    )
    .unwrap();

    let mut child = Command::new(jet_bin())
        .args(["dev", "app.jet", "--target=web"])
        .current_dir(&dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start project query dev server");
    struct KillOnDrop(std::process::Child);
    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let stdout = child.stdout.take().unwrap();
    let guard = KillOnDrop(child);
    let port = wait_for_ports(stdout).canvas;

    let (status, project) = http_get(port, "/canvas/project").expect("GET project");
    assert_eq!(status, 200);
    let project_revision = json_field(&String::from_utf8_lossy(&project), "project_revision");
    let source = fs::read_to_string(&entry).unwrap();
    let revision = jet::Canvas::source_revision(&source);
    let search = format!(
        "{{\"schema_version\":1,\"op\":\"project_search\",\"source_id\":\"app.jet\",\"revision\":\"{}\",\"project_revision\":\"{}\",\"query\":\"helper\"}}",
        revision, project_revision
    );
    let (status, body) = http_post(port, "/canvas/query", &search).expect("POST project search");
    assert_eq!(
        status,
        200,
        "project search response: {}",
        String::from_utf8_lossy(&body)
    );
    let body = String::from_utf8_lossy(&body);
    assert!(body.contains("\"op\":\"project_search\""), "{body}");
    assert!(body.contains("\"source_id\":\"helper.jet\""), "{body}");

    let references = format!(
        "{{\"schema_version\":1,\"op\":\"references\",\"source_id\":\"app.jet\",\"revision\":\"{}\",\"project_revision\":\"{}\",\"symbol\":\"helper\"}}",
        revision, project_revision
    );
    let (status, body) =
        http_post(port, "/canvas/query", &references).expect("POST project references");
    assert_eq!(status, 200);
    let body = String::from_utf8_lossy(&body);
    assert!(body.contains("\"op\":\"references\""), "{body}");
    assert!(body.contains("\"kind\":\"reference\""), "{body}");

    fs::write(
        dir.join("helper.jet"),
        "fn helper() Int -> {\n    return 3\n}\n",
    )
    .unwrap();
    let (status, body) = http_post(port, "/canvas/query", &search).expect("POST stale search");
    assert_eq!(status, 409);
    let body = String::from_utf8_lossy(&body);
    assert!(body.contains("\"kind\":\"conflict\""), "{body}");
    assert!(fs::read_to_string(dir.join("helper.jet"))
        .unwrap()
        .contains("return 3"));

    drop(guard);
    let _ = fs::remove_dir_all(&dir);
}

// ============================================================================
// Section: user-defined `fn dev()` entry mode (was tests/web_dev_fn.rs)
// ============================================================================

fn fn_fixture_src(port: u16, greeting: &str) -> String {
    format!(
        r#"use core.web.devserver as devserver

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
                "jet dev's own core.web.devserver never came up on port {}",
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

    for path in ["/C:/Windows/win.ini", "/\\Windows\\win.ini", "/\\\\server\\share\\secret"] {
        let (status, _) = http_get(port, path).expect("embedded static-path hostile request failed");
        assert_eq!(
            status, 400,
            "embedded Windows absolute static path escaped the build root: {path}"
        );
    }

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
            r#"use core.web.devserver as devserver

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

#[test]
fn jet_dev_static_generator_serves_reloads_and_watches_inputs() {
    if !have_tool("rustc") {
        eprintln!(
            "note: skipping jet_dev_static_generator_serves_reloads_and_watches_inputs (need rustc)"
        );
        return;
    }

    let dir = std::env::temp_dir().join(format!(
        "jet_dev_static_generator_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source_path = dir.join("generate.jet");
    fs::write(
        &source_path,
        include_str!("../examples/features/devloop/static_site_generator/run.jet"),
    )
    .unwrap();
    fs::write(
        dir.join("package.jet"),
        include_str!("../examples/features/devloop/static_site_generator/package.jet"),
    )
    .unwrap();
    fs::write(dir.join("content.txt"), "first headline").unwrap();
    fs::write(dir.join("widget.wasm"), [0, 97, 115, 109]).unwrap();

    let mut child = Command::new(jet_bin())
        .args(["dev", "generate.jet"])
        .current_dir(&dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to start static generator dev loop");

    struct KillOnDrop(std::process::Child);
    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let stdout = child.stdout.take().unwrap();
    let guard = KillOnDrop(child);
    let port = wait_for_app_preview(stdout);
    wait_for_server_up(port, Duration::from_secs(20));

    let (status, html_body) = http_get_without_session(port, "/").expect("GET static index");
    assert_eq!(status, 200);
    let html = String::from_utf8_lossy(&html_body);
    assert!(html.contains("<h1>first headline</h1>"), "{html}");
    assert!(html.contains("__jet_dev_status"), "live reload not injected");

    let (status, content_type, wasm) =
        http_get_with_content_type(port, "/widget.wasm").expect("GET generated wasm");
    assert_eq!(status, 200);
    assert_eq!(content_type, "application/wasm");
    assert_eq!(wasm, [0, 97, 115, 109]);

    let (_, baseline_body) = http_get_without_session(port, "/__jet_dev_version").unwrap();
    let baseline = String::from_utf8_lossy(&baseline_body).trim().to_string();

    fs::write(dir.join("content.txt"), "second headline").unwrap();
    let after_input = wait_for_version_change(port, &baseline, Duration::from_secs(20));
    assert_ne!(after_input, baseline);
    let (_, html_body) = http_get_without_session(port, "/").expect("GET index after input edit");
    assert!(
        String::from_utf8_lossy(&html_body).contains("<h1>second headline</h1>"),
        "generator input edit was not served"
    );

    let edited_source = include_str!("../examples/features/devloop/static_site_generator/run.jet")
        .replace("<h1>{headline}</h1>", "<h1>source edit: {headline}</h1>");
    assert_ne!(
        edited_source,
        include_str!("../examples/features/devloop/static_site_generator/run.jet"),
        "static generator source fixture did not change"
    );
    fs::write(&source_path, edited_source).unwrap();
    let after_source = wait_for_version_change(port, &after_input, Duration::from_secs(20));
    assert_ne!(after_source, after_input);
    let (_, html_body) = http_get_without_session(port, "/").expect("GET index after source edit");
    assert!(
        String::from_utf8_lossy(&html_body).contains("<h1>source edit: second headline</h1>"),
        "generator source edit was not served"
    );

    drop(guard);
    let _ = fs::remove_dir_all(&dir);
}

/// #439 / E3-UL6: browser matrix — WatchSession + reconnect, budget,
/// cleanup, and deterministic receipt after edit.
#[test]
fn ul6_browser_watch_matrix_budget_reconnect_cleanup() {
    if !have_tool("rustc") {
        eprintln!("note: skipping ul6_browser_watch_matrix_budget_reconnect_cleanup (need rustc)");
        return;
    }

    let dir = std::env::temp_dir().join(format!("jet_ul6_web_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src_path = dir.join("app.jet");
    fs::write(
        &src_path,
        "#Target(Web)\nfn run() {\n    print(\"hello\")\n}\n",
    )
    .unwrap();

    let mut session = jet_devserver::WatchSession::open(&src_path).unwrap();
    session.recover();
    assert!(session.graph().node_count() >= 1);

    let started = Instant::now();
    std::thread::sleep(Duration::from_millis(30));
    fs::write(
        &src_path,
        "#Target(Web)\nfn run() {\n    print(\"hello-2\")\n}\n",
    )
    .unwrap();
    let receipt = session.poll().expect("web source change");
    let ms = started.elapsed().as_millis();
    assert!(
        jet_devserver::within_budget(&receipt) || ms <= jet_devserver::EDIT_TO_VISIBLE_BUDGET_MS,
        "budget miss ms={ms} receipt={:?}",
        receipt.edit_to_visible_ms
    );
    assert!(!receipt.render().is_empty());
    session.acknowledge(&receipt).unwrap();

    let _ = fs::remove_dir_all(&dir);
    assert!(!dir.exists() || fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0) == 0);
}
