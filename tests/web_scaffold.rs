//! Card #2239: `jet new <name> --target=web` to a served browser page.
#![allow(non_snake_case)]

mod common;

use common::Scratch;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
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

fn http_get(port: u16, path: &str) -> Option<(u16, Vec<u8>)> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: localhost:{port}\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).ok()?;
    let separator = b"\r\n\r\n";
    let split = raw
        .windows(separator.len())
        .position(|window| window == separator)
        .map(|index| index + separator.len())?;
    let status = String::from_utf8_lossy(&raw[..split])
        .lines()
        .next()?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;
    Some((status, raw[split..].to_vec()))
}

fn wait_for_server(port: u16, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        if matches!(http_get(port, "/__jet_dev_version"), Some((200, _))) {
            return;
        }
        if Instant::now() >= deadline {
            panic!("web scaffold server did not start on {port}");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn jet_new_web_scaffold_runs_from_new_to_browser() {
    if !have_tool("rustc") {
        eprintln!("note: skipping jet_new_web_scaffold_runs_from_new_to_browser (need rustc)");
        return;
    }

    let scratch = Scratch::new("web-scaffold");
    let project_root = scratch.path.clone();
    let created = Command::new(jet_bin())
        .current_dir(&project_root)
        .args(["new", "web_app", "--target=web"])
        .output()
        .expect("spawn jet new web scaffold");
    assert!(
        created.status.success(),
        "jet new --target=web failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&created.stdout),
        String::from_utf8_lossy(&created.stderr)
    );

    let project = project_root.join("web_app");
    let source = fs::read_to_string(project.join("run.jet")).expect("web scaffold source");
    assert!(source.contains("#Target(Web)"), "web target missing:\n{source}");
    assert!(source.contains("reactive.signal"), "reactive example missing:\n{source}");
    assert!(source.contains("ui.button"), "editable button example missing:\n{source}");
    for command in ["jet dev", "jet test", "jet build --target web"] {
        assert!(source.contains(command), "scaffold comment missing `{command}`");
    }
    assert!(
        !project.join("run.html").exists(),
        "web scaffold must use the generated default page"
    );

    let manifest = fs::read_to_string(project.join("package.jet")).expect("web scaffold manifest");
    assert!(
        manifest.contains("authority: .{ holds: { allow: [IO, Browser] } }")
            || manifest.contains("authority: .{ holds: { allow: [Browser, IO] } }"),
        "web scaffold authority must grant Browser:\n{manifest}"
    );

    let build = Command::new(jet_bin())
        .current_dir(&project)
        .args(["build", "--target", "web"])
        .output()
        .expect("spawn jet build for web scaffold");
    assert!(
        build.status.success(),
        "web scaffold build failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr)
    );
    assert!(project.join("build/index.html").is_file());

    let test = Command::new(jet_bin())
        .current_dir(&project)
        .arg("test")
        .output()
        .expect("spawn jet test for web scaffold");
    assert!(
        test.status.success(),
        "web scaffold test failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&test.stdout),
        String::from_utf8_lossy(&test.stderr)
    );

    let port = unused_local_port();
    struct KillOnDrop(Child);
    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let _server = KillOnDrop(
        Command::new(jet_bin())
            .current_dir(&project)
            .args(["dev", &format!("--port={port}")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn jet dev for web scaffold"),
    );
    wait_for_server(port, Duration::from_secs(30));

    let (status, body) = http_get(port, "/").expect("GET scaffold page");
    assert_eq!(status, 200, "web scaffold page was not served");
    let page = String::from_utf8_lossy(&body);
    assert!(page.contains("<title>jet web app</title>"), "{page}");
    assert!(page.contains("import \"./app.js\";"), "{page}");
    assert!(
        !page.contains("jet_main"),
        "generated scaffold page must not know an export name: {page}"
    );
}
