//! D-CANVASTEST1=A: stdlib-only browser scenarios for Canvas.

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Condvar, Mutex, Once, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

static BUILD_JET: Once = Once::new();
const MAX_CANVAS_BROWSERS: usize = 4;
static CANVAS_BROWSER_POOL: OnceLock<(Mutex<usize>, Condvar)> = OnceLock::new();

struct CanvasBrowserPermit;

impl CanvasBrowserPermit {
    fn acquire() -> Self {
        let (available, wake) =
            CANVAS_BROWSER_POOL.get_or_init(|| (Mutex::new(MAX_CANVAS_BROWSERS), Condvar::new()));
        let mut available = available.lock().expect("Canvas browser permit lock poisoned");
        while *available == 0 {
            available = wake.wait(available).expect("Canvas browser permit lock poisoned");
        }
        *available -= 1;
        Self
    }
}

impl Drop for CanvasBrowserPermit {
    fn drop(&mut self) {
        let (available, wake) =
            CANVAS_BROWSER_POOL.get_or_init(|| (Mutex::new(MAX_CANVAS_BROWSERS), Condvar::new()));
        *available.lock().expect("Canvas browser permit lock poisoned") += 1;
        wake.notify_one();
    }
}

const DEMO: &str = r#"fn helper() -> Int {
    return 1
}

fn square(n: Int) -> Int {
    return n * n
}

fn summarize(limit: Int) -> Int {
    total := square(limit)
    if total > 10 {
        return total
    } else {
        return total + 1
    }
}

fn scratch(limit: Int, text: String, flag: Bool, ratio: Float) {
    print(limit)
}

fn run() {
    print(summarize(4))
}
"#;

#[test]
fn open_and_render() {
    run_canvas_scenario("open-and-render");
}

#[test]
fn pan_zoom_fit() {
    run_canvas_scenario("pan-zoom-fit");
}

#[test]
fn click_select_details() {
    run_canvas_scenario("click-select-details");
}

#[test]
fn node_drag_persists_without_source_change() {
    run_canvas_scenario("node-drag-persists-without-source-change");
}

#[test]
fn read_graph_overview() {
    run_canvas_scenario("read-graph-overview");
}

#[test]
fn palette_insert_core_fn() {
    run_canvas_scenario("palette-insert-core-fn");
}

#[test]
fn palette_insert_catalog_sweep() {
    run_canvas_scenario("palette-insert-catalog-sweep");
}

#[test]
fn palette_insert_flow_variable_project_core() {
    run_canvas_scenario("palette-insert-flow-variable-project-core");
}

#[test]
fn wire_data_and_exec() {
    run_canvas_scenario("wire-data-and-exec");
}

#[test]
fn exec_rewire_reorders_statements() {
    run_canvas_scenario("exec-rewire-reorders-statements");
}

#[test]
fn exec_rewire_refuses_cross_block() {
    run_canvas_scenario("exec-rewire-refuses-cross-block");
}

#[test]
fn exec_rewire_binding_order_diagnostic() {
    run_canvas_scenario("exec-rewire-binding-order-diagnostic");
}

#[test]
fn pattern_arm_add_edit_remove() {
    run_canvas_scenario("pattern-arm-add-edit-remove");
}

#[test]
fn pattern_arm_invalid_refused() {
    run_canvas_scenario("pattern-arm-invalid-refused");
}

#[test]
fn multi_input_append_remove() {
    run_canvas_scenario("multi-input-append-remove");
}

#[test]
fn inline_edit_values() {
    run_canvas_scenario("inline-edit-values");
}

#[test]
fn rename_variable_sidebar() {
    run_canvas_scenario("rename-variable-sidebar");
}

#[test]
fn fallible_context() {
    run_canvas_scenario("fallible-context");
}

#[test]
fn excluded_entry_rendering() {
    run_canvas_scenario("excluded-entry-rendering");
}

#[test]
fn no_dead_end_ad_hoc_insert() {
    run_canvas_scenario("no-dead-end-ad-hoc-insert");
}

#[test]
fn failed_insert_shows_panel() {
    run_canvas_scenario("failed-insert-shows-panel");
}

#[test]
fn check_button_populates_panel() {
    run_canvas_scenario("check-button-populates-panel");
}

#[test]
fn bubble_appears_and_clears() {
    run_canvas_scenario("bubble-appears-and-clears");
}

#[test]
fn undo_restores_source() {
    run_canvas_scenario("undo-restores-source");
}

#[test]
fn undo_depth_20_mixed_run() {
    run_canvas_scenario("undo-depth-20-mixed-run");
}

#[test]
fn run_button_output_visible() {
    run_canvas_scenario("run-button-output-visible");
}

#[test]
fn graph_source_toggle_preserves_selection() {
    run_canvas_scenario("graph-source-toggle-preserves-selection");
}

#[test]
fn random_ops_source_sync() {
    run_canvas_scenario("random-ops-source-sync");
}

#[test]
fn harness_click_noop_selftest() {
    run_canvas_scenario("harness-click-noop-selftest");
}

fn run_canvas_scenario(name: &str) {
    if !tool_available("chromium") {
        eprintln!("ignored: Canvas scenario `{name}` needs dev-shell chromium on PATH");
        return;
    }
    if !tool_available("node") {
        eprintln!("ignored: Canvas scenario `{name}` needs dev-shell node on PATH");
        return;
    }
    let _browser_permit = CanvasBrowserPermit::acquire();
    ensure_jet_built();

    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let case = CanvasCase::new(&repo, name);
    let port = free_port();
    let mut server = DevServer::start(&repo, &case.dir, &case.entry, port);
    let output = Command::new("node")
        .current_dir(&repo)
        .arg("scripts/canvas-test/run.mjs")
        .arg("--scenario")
        .arg(name)
        .arg("--port")
        .arg(port.to_string())
        .arg("--out-dir")
        .arg(&case.screenshots)
        .arg("--seed")
        .arg("373")
        .output()
        .expect("run Canvas scenario driver");
    let server_log = server.stop();
    if !output.status.success() {
        panic!(
            "Canvas scenario `{name}` failed\n\n--- driver stdout ---\n{}\n--- driver stderr ---\n{}\n--- jet dev ---\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
            server_log
        );
    }
    eprintln!("{}", String::from_utf8_lossy(&output.stdout));
    if name == "node-drag-persists-without-source-change" {
        case.cleanup();
    }
}

fn ensure_jet_built() {
    BUILD_JET.call_once(|| {
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let bin = cargo_target_dir(&repo).join("debug/jet");
        if bin.exists() {
            return;
        }
        let status = Command::new("cargo")
            .current_dir(&repo)
            .arg("build")
            .status()
            .expect("cargo build for Canvas scenarios");
        assert!(status.success(), "cargo build failed before Canvas scenarios");
    });
}

fn cargo_target_dir(repo: &Path) -> PathBuf {
    match std::env::var_os("CARGO_TARGET_DIR") {
        Some(dir) if Path::new(&dir).is_absolute() => PathBuf::from(dir),
        Some(dir) => repo.join(dir),
        None => repo.join("target"),
    }
}

fn tool_available(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

struct CanvasCase {
    dir: PathBuf,
    entry: PathBuf,
    screenshots: PathBuf,
}

impl CanvasCase {
    fn new(repo: &Path, name: &str) -> CanvasCase {
        let root = std::env::var("JET_VERIFY_TMPDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| repo.join("target/test-tmp"));
        let dir = root.join(format!("canvas_scenario_{}_{}", name.replace('-', "_"), std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create Canvas scenario dir");
        let entry = dir.join("main.jet");
        fs::write(&entry, DEMO).expect("write Canvas scenario source");
        let screenshots = dir.join("screenshots");
        fs::create_dir_all(&screenshots).expect("create Canvas screenshot dir");
        CanvasCase {
            dir,
            entry,
            screenshots,
        }
    }

    fn cleanup(&self) {
        fs::remove_dir_all(&self.dir).expect("remove passed Canvas scenario artifacts");
    }
}

struct DevServer {
    child: Child,
}

impl DevServer {
    fn start(repo: &Path, cwd: &Path, entry: &Path, port: u16) -> DevServer {
        let jet = cargo_target_dir(repo).join("debug/jet");
        let mut child = Command::new(jet)
            .current_dir(cwd)
            .arg("dev")
            .arg(entry)
            .arg("--target=web")
            .arg(format!("--port={port}"))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start jet dev Canvas server");
        wait_for_server(&mut child, port);
        DevServer { child }
    }

    fn stop(&mut self) -> String {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let mut out = String::new();
        if let Some(mut stdout) = self.child.stdout.take() {
            let _ = stdout.read_to_string(&mut out);
        }
        if let Some(mut stderr) = self.child.stderr.take() {
            let mut err = String::new();
            let _ = stderr.read_to_string(&mut err);
            out.push_str(&err);
        }
        out
    }
}

impl Drop for DevServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn wait_for_server(child: &mut Child, port: u16) {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(30) {
        if let Some(status) = child.try_wait().expect("poll jet dev") {
            panic!("jet dev exited before serving Canvas: {status}");
        }
        if http_ok(port, "/__jet_dev_status") {
            return;
        }
        thread::sleep(Duration::from_millis(80));
    }
    panic!("timed out waiting for jet dev on port {port}");
}

fn http_ok(port: u16, path: &str) -> bool {
    let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) else {
        return false;
    };
    let request = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut response = String::new();
    if stream.read_to_string(&mut response).is_err() {
        return false;
    }
    response.starts_with("HTTP/1.1 200")
}

fn free_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .expect("bind free local port")
        .local_addr()
        .expect("local addr")
        .port()
}
