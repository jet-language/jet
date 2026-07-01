//! c134 Phase 7: `jet dev <file>.jet --target=web` — the std-only watch +
//! rebuild-on-save + live-reload dev server (Source/CmdDevWeb.rs). Starts the
//! real `jet` binary as a child process, talks to it over a plain TCP socket
//! (no new dependency — matches the server's own std-only HTTP), edits a
//! throwaway temp copy of a fixture, and confirms the rebuild lands.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
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

const FIXTURE_SRC: &str = r#"use core.ui as ui

fn main() {
    backend #= ui.null_backend()
    node #= ui.node("hello", 100.0, 20.0)
    constraint #= ui.constraint(0.0, 0.0, 200.0, 100.0)
    size #= backend.measure(node, constraint)
    print(size.width)
    print(size.height)
}
"#;

/// Same source with one literal changed, so the compiled `app.js` differs
/// byte-for-byte from `FIXTURE_SRC`'s — proof a rebuild actually re-ran the
/// compiler rather than re-serving a cached response.
const FIXTURE_SRC_EDITED: &str = r#"use core.ui as ui

fn main() {
    backend #= ui.null_backend()
    node #= ui.node("hello there", 100.0, 20.0)
    constraint #= ui.constraint(0.0, 0.0, 200.0, 100.0)
    size #= backend.measure(node, constraint)
    print(size.width)
    print(size.height)
}
"#;

/// Minimal blocking HTTP/1.1 GET over a raw `TcpStream` — the same shape of
/// client `jet dev --target=web`'s own std-only server (Source/CmdDevWeb.rs)
/// expects, so the test doesn't need `curl` or any HTTP crate.
fn http_get(port: u16, path: &str) -> Option<(u16, Vec<u8>)> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .ok()?;
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
