//! D-WEBBACKEND1 C4 (#704): real Chromium acceptance for the web backend.
//!
//! Proves, in a real browser against AOT `jet build --target=web` artifacts:
//! DOM create/update/remove, reactive rendering, event→Wasm callbacks, Wasm
//! compute, bundled artifacts, manifest-embedded source maps, and console
//! output parity with the node harnesses in `web_build.rs`.
//!
//! Dev-server diagnostics in a real browser remain in
//! `web_dev::jet_dev_web_browser_runs_hybrid_status_overlay_and_recovery_matrix`.

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn have_tool(name: &str) -> bool {
    Command::new(name).arg("--version").output().is_ok()
}

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn unused_local_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .expect("bind ephemeral port")
        .local_addr()
        .expect("local addr")
        .port()
}

fn resolve_executable(candidate: impl AsRef<Path>) -> Option<PathBuf> {
    let candidate = candidate.as_ref();
    if candidate.components().count() > 1 {
        return candidate.is_file().then(|| candidate.to_path_buf());
    }
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(candidate))
        .find(|path| path.is_file())
}

fn web_tools() -> Option<(PathBuf, PathBuf)> {
    let chromium = std::env::var_os("JET_WEB_CHROMIUM")
        .or_else(|| std::env::var_os("CHROMIUM"))
        .or_else(|| Some("chromium".into()))
        .and_then(|name| resolve_executable(Path::new(&name)))?;
    let node = std::env::var_os("JET_WEB_NODE")
        .or_else(|| std::env::var_os("NODE"))
        .or_else(|| Some("node".into()))
        .and_then(|name| resolve_executable(Path::new(&name)))?;
    let chromium_ok = Command::new(&chromium)
        .arg("--version")
        .output()
        .ok()
        .map(|out| {
            let version = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            version.contains("Chromium") || version.contains("Chrome")
        })
        .unwrap_or(false);
    let node_ok = Command::new(&node)
        .arg("--version")
        .output()
        .ok()
        .map(|out| {
            let version = String::from_utf8_lossy(&out.stdout);
            version.starts_with('v') && version.as_bytes().get(1).is_some_and(u8::is_ascii_digit)
        })
        .unwrap_or(false);
    (chromium_ok && node_ok).then_some((chromium, node))
}

fn wait_for_server(port: u16, marker: &str, timeout: Duration) {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) {
            let req = "GET /click/web.manifest.json HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
            if stream.write_all(req.as_bytes()).is_ok() {
                let mut raw = String::new();
                if stream.read_to_string(&mut raw).is_ok() && raw.contains("200 OK") {
                    return;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for {marker} on port {port}");
}

struct StaticServer {
    child: Child,
}

impl StaticServer {
    fn start(node: &Path, root: &Path, port: u16) -> StaticServer {
        let child = Command::new(node)
            .arg(repo_root().join("scripts/web-test/serve.mjs"))
            .arg("--port")
            .arg(port.to_string())
            .arg("--root")
            .arg(root)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start static web server");
        wait_for_server(port, "static web server", Duration::from_secs(10));
        StaticServer { child }
    }
}

impl Drop for StaticServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn jet_build_web(cwd: &Path, entry: &str) {
    let out = Command::new(jet_bin())
        .current_dir(cwd)
        .args(["build", "--target=web", entry])
        .output()
        .expect("jet build --target=web");
    assert!(
        out.status.success(),
        "jet build --target=web failed in {}:\nstdout: {}\nstderr: {}",
        cwd.display(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn publish_build(case_dir: &Path, serve_name: &str, serve_root: &Path) {
    let build = case_dir.join("build");
    let dest = serve_root.join(serve_name);
    let _ = fs::remove_dir_all(&dest);
    fs::create_dir_all(&dest).expect("create serve dir");
    for entry in fs::read_dir(&build).expect("read build dir") {
        let entry = entry.expect("build entry");
        let file_name = entry.file_name();
        fs::copy(entry.path(), dest.join(file_name)).expect("copy build artifact");
    }
}

fn write_callback_source(dest: &Path) {
    let mut src = String::from("#Target(Web)\n#HTML(\"index.html\")\n");
    src.push_str(include_str!("../examples/features/web/web_wasm_callback.jet"));
    fs::write(dest, src).expect("write callback source");
}

fn prepare_acceptance_root(root: &Path) {
    let repo = repo_root();
    let click = root.join("click_src");
    fs::create_dir_all(&click).unwrap();
    fs::copy(
        repo.join("examples/features/web/ui_web_click.jet"),
        click.join("app.jet"),
    )
    .unwrap();
    fs::copy(
        repo.join("examples/features/web/ui_web_click.html"),
        click.join("ui_web_click.html"),
    )
    .unwrap();
    jet_build_web(&click, "app.jet");
    publish_build(&click, "click", root);

    let reactive = root.join("reactive_src");
    fs::create_dir_all(&reactive).unwrap();
    fs::write(
        reactive.join("app.jet"),
        include_str!("../examples/features/web/ui_web_reactive.jet"),
    )
    .unwrap();
    jet_build_web(&reactive, "app.jet");
    publish_build(&reactive, "reactive", root);

    let compute = root.join("compute_src");
    fs::create_dir_all(&compute).unwrap();
    fs::write(
        compute.join("app.jet"),
        include_str!("../examples/features/web/web_compute.jet"),
    )
    .unwrap();
    jet_build_web(&compute, "app.jet");
    publish_build(&compute, "compute", root);

    let callback = root.join("callback_src");
    fs::create_dir_all(&callback).unwrap();
    write_callback_source(&callback.join("app.jet"));
    fs::write(
        callback.join("index.html"),
        include_str!("fixtures/web_browser_callback.html"),
    )
    .unwrap();
    jet_build_web(&callback, "app.jet");
    publish_build(&callback, "callback", root);

    let lifecycle = root.join("lifecycle_src");
    fs::create_dir_all(&lifecycle).unwrap();
    let lifecycle_src = format!(
        "#HTML(\"index.html\")\n{}",
        include_str!("fixtures/web_browser_lifecycle.jet")
    );
    fs::write(lifecycle.join("app.jet"), lifecycle_src).unwrap();
    fs::write(
        lifecycle.join("index.html"),
        include_str!("fixtures/web_browser_lifecycle.html"),
    )
    .unwrap();
    jet_build_web(&lifecycle, "app.jet");
    publish_build(&lifecycle, "lifecycle", root);
}

#[test]
fn web_browser_aot_acceptance_proves_dom_reactive_wasm_bundle_and_maps() {
    if !have_tool("rustc") {
        eprintln!("note: skipping web_browser acceptance (need rustc)");
        return;
    }
    let Some((chromium, node)) = web_tools() else {
        eprintln!("note: skipping web_browser acceptance (need chromium + node)");
        return;
    };

    let root = std::env::temp_dir().join(format!("jet_web_browser_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    prepare_acceptance_root(&root);

    let port = unused_local_port();
    let _server = StaticServer::start(&node, &root, port);
    let output = Command::new(&node)
        .current_dir(repo_root())
        .env("CHROMIUM", &chromium)
        .arg("scripts/web-test/acceptance.mjs")
        .arg("--port")
        .arg(port.to_string())
        .output()
        .expect("run web browser acceptance");
    assert!(
        output.status.success(),
        "web browser acceptance failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("PASS web backend browser acceptance matrix"),
        "acceptance did not report completion"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn web_browser_source_map_cdp_jet_breakpoint() {
    if !have_tool("rustc") {
        eprintln!("note: skipping web source-map CDP (need rustc)");
        return;
    }
    let Some((chromium, node)) = web_tools() else {
        eprintln!("note: skipping web source-map CDP (need chromium + node)");
        return;
    };

    let root = std::env::temp_dir().join(format!("jet_web_sourcemap_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    prepare_acceptance_root(&root);

    let port = unused_local_port();
    let _server = StaticServer::start(&node, &root, port);
    let output = Command::new(&node)
        .current_dir(repo_root())
        .env("CHROMIUM", &chromium)
        .arg("scripts/web-test/sourcemap.mjs")
        .arg("--port")
        .arg(port.to_string())
        .arg("--prefix")
        .arg("/click")
        .arg("--wasm-prefix")
        .arg("/compute")
        .output()
        .expect("run web source-map CDP");
    assert!(
        output.status.success(),
        "web source-map CDP failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("PASS web source-map CDP"),
        "source-map CDP did not report completion"
    );
    let _ = fs::remove_dir_all(&root);
}
