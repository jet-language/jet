//! c-devserver (owner-directed 2026-07-01): `jet dev <file>` when the file
//! defines a top-level `fn dev()` — the file configures and starts its OWN
//! `core.devserver` value instead of the CLI hardcoding `--target=web
//! --port=<N>`. Mirrors `tests/web_dev.rs`'s structure (spawn the real `jet`
//! binary, poll for the port over a raw `TcpStream`, edit the watched file,
//! confirm a rebuild happens), but drives it all through zero CLI flags —
//! `jet dev app.jet`, no `--target=web`.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn have_tool(name: &str) -> bool {
    Command::new(name).arg("--version").output().is_ok()
}

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

const FIXTURE_SRC: &str = r#"use core.devserver as devserver

fn dev() {
    server #= devserver.for_app("app.jet")
    server.port(8181)
    server.serve()
}

fn main() {
    print("hello, world")
}
"#;

/// Same source with one literal changed, so a rebuild produces different
/// `app.js` output than the initial build — proof a save actually triggers a
/// recompile rather than re-serving a cached response.
const FIXTURE_SRC_EDITED: &str = r#"use core.devserver as devserver

fn dev() {
    server #= devserver.for_app("app.jet")
    server.port(8181)
    server.serve()
}

fn main() {
    print("hello there")
}
"#;

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

/// Polls for the server to come up on the fixed port `8181` the fixture's
/// own `dev()` passes to `.port(...)` — never a blind `sleep`.
fn wait_for_server_up(port: u16, timeout: Duration) {
    let start = Instant::now();
    loop {
        if http_get(port, "/__jet_dev_version").is_some() {
            return;
        }
        if start.elapsed() > timeout {
            panic!("jet dev's own core.devserver never came up on port {}", port);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

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
fn jet_dev_runs_user_defined_dev_fn_and_rebuilds_on_save() {
    if !have_tool("rustc") {
        eprintln!("note: skipping jet_dev_runs_user_defined_dev_fn_and_rebuilds_on_save (need rustc)");
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
    fs::write(&src_path, FIXTURE_SRC).unwrap();

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

    let port: u16 = 8181;
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

    fs::write(&src_path, FIXTURE_SRC_EDITED).unwrap();
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
    fs::write(
        &src_path,
        r#"use core.devserver as devserver

fn dev() {
    server #= devserver.app()
    server.port(8182)
    server.serve()
}

fn main() {
    print("hello, world")
}
"#,
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

    let port: u16 = 8182;
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
