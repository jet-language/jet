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

struct CanvasTools {
    chromium: PathBuf,
    node: PathBuf,
}

struct GeckoTools {
    firefox: PathBuf,
    geckodriver: PathBuf,
    node: PathBuf,
}

static CANVAS_TOOLS: OnceLock<Option<CanvasTools>> = OnceLock::new();
static GECKO_TOOLS: OnceLock<Result<GeckoTools, String>> = OnceLock::new();

#[test]
fn full_verification_uses_clean_short_external_tmpdir() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let probe = |mode: &str, parent: Option<&Path>| {
        let mut command = Command::new("bash");
        command
            .current_dir(&repo)
            .arg("scripts/agent/verify-full.sh")
            .arg(mode)
            .env("JET_NIX_TMP_CLEANED", "1")
            .env_remove("JET_VERIFY_TMPDIR");
        if let Some(parent) = parent {
            command.env("JET_VERIFY_TMPDIR", parent);
        }
        let output = command.output().expect("probe full-verification temp root");
        let roots = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(PathBuf::from)
            .collect::<Vec<_>>();
        assert_eq!(roots.len(), 4, "temp probe must report every exported temp root");
        assert!(roots.iter().all(|path| path == &roots[0]));
        assert!(
            roots[0]
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("jet-verify."))
        );
        assert!(!roots[0].exists(), "verification temp root must be removed on exit");
        (output.status, roots[0].clone())
    };

    let (status, root) = probe("--probe-temp-root", None);
    assert!(status.success());
    assert_eq!(root.parent(), Some(Path::new("/tmp")));

    let override_parent = std::env::temp_dir().join(format!(
        "jet-verify-parent-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    fs::create_dir_all(&override_parent).unwrap();
    let (status, root) = probe("--probe-temp-root", Some(&override_parent));
    assert!(status.success());
    assert_eq!(root.parent(), Some(override_parent.as_path()));
    let (status, root) = probe("--probe-temp-root-signal", Some(&override_parent));
    assert_eq!(status.code(), Some(143));
    assert_eq!(root.parent(), Some(override_parent.as_path()));
    fs::remove_dir(override_parent).unwrap();
}

#[test]
#[cfg(unix)]
fn strict_full_verification_rejects_each_missing_canvas_tool_once() {
    use std::os::unix::fs::PermissionsExt as _;

    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let tools = std::env::temp_dir().join(format!(
        "jet-canvas-preflight-tools-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    fs::create_dir_all(&tools).unwrap();
    let chromium = tools.join("chromium");
    let firefox = tools.join("firefox");
    let geckodriver = tools.join("geckodriver");
    let node = tools.join("node");
    fs::write(&chromium, "#!/bin/sh\necho 'Chromium 1'\n").unwrap();
    fs::write(&firefox, "#!/bin/sh\necho 'Mozilla Firefox 1'\n").unwrap();
    fs::write(&geckodriver, "#!/bin/sh\necho 'geckodriver 1'\n").unwrap();
    fs::write(&node, "#!/bin/sh\necho 'v1.0.0'\n").unwrap();
    fs::set_permissions(&chromium, fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(&firefox, fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(&geckodriver, fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(&node, fs::Permissions::from_mode(0o755)).unwrap();
    for (missing, chromium, firefox, geckodriver, node) in [
        (
            "chromium",
            tools.join("missing-chromium"),
            firefox.clone(),
            geckodriver.clone(),
            node.clone(),
        ),
        (
            "firefox",
            chromium.clone(),
            tools.join("missing-firefox"),
            geckodriver.clone(),
            node.clone(),
        ),
        (
            "geckodriver",
            chromium.clone(),
            firefox.clone(),
            tools.join("missing-geckodriver"),
            node.clone(),
        ),
        (
            "node",
            chromium,
            firefox,
            geckodriver,
            tools.join("missing-node"),
        ),
    ] {
        let output = Command::new("bash")
            .current_dir(&repo)
            .arg("scripts/agent/verify-full.sh")
            .env("JET_VERIFY_CANVAS_PREREQUISITES_ONLY", "1")
            .env("JET_VERIFY_TEMP_PROBE_ONLY", "1")
            .env("JET_NIX_TMP_CLEANED", "1")
            .env("JET_CANVAS_CHROMIUM", &chromium)
            .env("JET_CANVAS_FIREFOX", &firefox)
            .env("JET_CANVAS_GECKODRIVER", &geckodriver)
            .env("JET_CANVAS_NODE", &node)
            .output()
            .expect("run strict Canvas prerequisite preflight");
        assert!(!output.status.success(), "missing {missing} must fail");
        let stderr = String::from_utf8_lossy(&output.stderr);
        let expected = format!(
            "error: Canvas interaction tests require Chromium, Firefox/geckodriver, and Node; missing: {missing}. Run scripts/agent/jet-env full scripts/agent/verify-full.sh."
        );
        assert_eq!(stderr.matches(&expected).count(), 1, "{stderr}");
        assert!(!stderr.contains("ignored:"), "{stderr}");
    }
    fs::remove_dir_all(tools).unwrap();
}

#[test]
fn strict_full_verification_rejects_executable_impostors() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("bash")
        .current_dir(&repo)
        .arg("scripts/agent/verify-full.sh")
        .env("JET_VERIFY_CANVAS_PREREQUISITES_ONLY", "1")
        .env("JET_NIX_TMP_CLEANED", "1")
        .env("JET_CANVAS_CHROMIUM", "true")
        .env("JET_CANVAS_FIREFOX", "true")
        .env("JET_CANVAS_GECKODRIVER", "true")
        .env("JET_CANVAS_NODE", "true")
        .output()
        .expect("run strict Canvas prerequisite preflight");
    assert!(!output.status.success(), "arbitrary executables must fail identity checks");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(stderr.matches("Canvas interaction tests require").count(), 1, "{stderr}");
    assert!(stderr.contains("missing: chromium,firefox,geckodriver,node"), "{stderr}");
}

#[test]
fn strict_full_verification_accepts_canvas_tools_in_dev_shell() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let chromium = resolve_executable("chromium").expect("dev-shell chromium path");
    let firefox = resolve_executable("firefox").expect("dev-shell firefox path");
    let geckodriver = resolve_executable("geckodriver").expect("dev-shell geckodriver path");
    let node = resolve_executable("node").expect("dev-shell node path");
    let output = Command::new("bash")
        .current_dir(&repo)
        .arg("scripts/agent/verify-full.sh")
        .env("JET_VERIFY_CANVAS_PREREQUISITES_ONLY", "1")
        .env("JET_NIX_TMP_CLEANED", "1")
        .env("JET_CANVAS_CHROMIUM", &chromium)
        .env("JET_CANVAS_FIREFOX", &firefox)
        .env("JET_CANVAS_GECKODRIVER", &geckodriver)
        .env("JET_CANVAS_NODE", &node)
        .output()
        .expect("run strict Canvas prerequisite preflight");
    assert!(
        output.status.success(),
        "dev-shell Canvas prerequisite preflight failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn cdp_driver_owns_parallel_browsers_under_long_tmpdir() {
    let Some(tools) = canvas_tools() else {
        eprintln!("ignored: CDP lifecycle regression needs dev-shell Chromium and Node");
        return;
    };
    let _permits = (0..MAX_CANVAS_BROWSERS)
        .map(|_| CanvasBrowserPermit::acquire())
        .collect::<Vec<_>>();
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let long_tmp = std::env::temp_dir().join(format!(
        "canvas-driver-lifecycle-{}-{}",
        "x".repeat(120),
        std::process::id()
    ));
    fs::create_dir_all(&long_tmp).expect("create long Canvas driver temp root");
    let output = Command::new(&tools.node)
        .current_dir(&repo)
        .env("CHROMIUM", &tools.chromium)
        .env("TMPDIR", &long_tmp)
        .arg("scripts/canvas-test/driver-lifecycle.mjs")
        .arg(MAX_CANVAS_BROWSERS.to_string())
        .output()
        .expect("run Canvas CDP lifecycle regression");
    fs::remove_dir_all(&long_tmp).expect("remove long Canvas driver temp root");
    assert!(
        output.status.success(),
        "parallel CDP lifecycle failed\n--- stdout ---\n{}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("PASS 4 isolated Chromium lifecycles")
    );
}

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
fn promote_pin_keyboard_gesture() {
    run_canvas_scenario("promote-pin-keyboard-gesture");
}

#[test]
fn conversion_keyboard_gesture() {
    run_canvas_scenario("conversion-keyboard-gesture");
}

#[test]
fn math_expression_keyboard_edit() {
    run_canvas_scenario("math-expression-keyboard-edit");
}

#[test]
fn collapse_expand_pointer_gesture() {
    run_canvas_scenario("collapse-expand-pointer-gesture");
}

#[test]
fn source_comment_keyboard_reload() {
    run_canvas_scenario("source-comment-keyboard-reload");
}

#[test]
fn workspace_keyboard_view_state() {
    run_canvas_scenario("workspace-keyboard-view-state");
}

#[test]
fn node_docs_pointer_hover() {
    run_canvas_scenario("node-docs-pointer-hover");
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

#[test]
fn gecko_smoke() {
    if std::env::var_os("JET_CANVAS_GECKO_SMOKE").as_deref()
        != Some(std::ffi::OsStr::new("1"))
    {
        eprintln!("ignored: Gecko smoke lane not selected");
        return;
    }
    let tools = gecko_tools().unwrap_or_else(|error| panic!("{error}"));
    run_gecko_cleanup_probe(tools);
    for name in [
        "open-and-render",
        "pan-zoom-fit",
        "click-select-details",
        "palette-insert-core-fn",
        "wire-data-and-exec",
        "undo-restores-source",
        "check-button-populates-panel",
        "random-ops-source-sync",
    ] {
        run_browser_scenario(
            name,
            "firefox",
            &tools.node,
            &[
                ("FIREFOX", tools.firefox.as_path()),
                ("GECKODRIVER", tools.geckodriver.as_path()),
            ],
        );
    }
}

fn run_gecko_cleanup_probe(tools: &GeckoTools) {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new(&tools.node)
        .current_dir(&repo)
        .env("FIREFOX", &tools.firefox)
        .env("GECKODRIVER", &tools.geckodriver)
        .arg("scripts/canvas-test/gecko-lifecycle.mjs")
        .output()
        .expect("run Gecko cleanup lifecycle probe");
    assert!(
        output.status.success(),
        "Gecko cleanup lifecycle probe failed\n--- stdout ---\n{}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    eprintln!("{}", String::from_utf8_lossy(&output.stdout));
}

fn run_canvas_scenario(name: &str) {
    let Some(tools) = canvas_tools() else {
        eprintln!("ignored: Canvas scenario `{name}` needs dev-shell Chromium and Node");
        return;
    };
    run_browser_scenario(
        name,
        "chromium",
        &tools.node,
        &[("CHROMIUM", tools.chromium.as_path())],
    );
}

fn run_browser_scenario(
    name: &str,
    browser: &str,
    node: &Path,
    environment: &[(&str, &Path)],
) {
    let _browser_permit = CanvasBrowserPermit::acquire();
    ensure_jet_built();

    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let case = CanvasCase::new(&repo, name);
    let port = free_port();
    let mut server = DevServer::start(&repo, &case.dir, &case.entry, port);
    let mut command = Command::new(node);
    command
        .current_dir(&repo)
        .envs(environment.iter().copied())
        .arg("scripts/canvas-test/run.mjs")
        .arg("--scenario")
        .arg(name)
        .arg("--browser")
        .arg(browser)
        .arg("--port")
        .arg(port.to_string())
        .arg("--out-dir")
        .arg(&case.screenshots)
        .arg("--seed")
        .arg("373");
    let output = command
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
}

fn ensure_jet_built() {
    BUILD_JET.call_once(|| {
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let target = canvas_cargo_target_dir(&repo);
        let status = Command::new("cargo")
            .current_dir(&repo)
            .env("CARGO_TARGET_DIR", &target)
            .arg("build")
            .status()
            .expect("cargo build for Canvas scenarios");
        assert!(status.success(), "cargo build failed before Canvas scenarios");
        assert!(
            target.join("debug/jet").is_file(),
            "cargo build did not produce the Canvas scenario jet binary"
        );
    });
}

fn cargo_target_dir(repo: &Path) -> PathBuf {
    match std::env::var_os("CARGO_TARGET_DIR") {
        Some(dir) if Path::new(&dir).is_absolute() => PathBuf::from(dir),
        Some(dir) => repo.join(dir),
        None => repo.join("target"),
    }
}

fn canvas_cargo_target_dir(repo: &Path) -> PathBuf {
    // Cargo fingerprints include source paths, but one shared target can still
    // reuse another worktree's same-name package artifacts. Keep the cache in
    // the configured shared target while keying its fingerprint namespace by
    // this checkout. Running `cargo build` every time then becomes a cheap
    // no-op only when Cargo proves this worktree's embedded Canvas JS current.
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in repo.as_os_str().as_encoded_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    cargo_target_dir(repo).join(format!("canvas-scenarios-{hash:016x}"))
}

fn canvas_tools() -> Option<&'static CanvasTools> {
    CANVAS_TOOLS
        .get_or_init(|| {
            let strict = std::env::var_os("JET_CANVAS_PREREQUISITES").as_deref()
                == Some(std::ffi::OsStr::new("strict"));
            let chromium = resolve_canvas_tool("chromium", strict)?;
            let node = resolve_canvas_tool("node", strict)?;
            Some(CanvasTools { chromium, node })
        })
        .as_ref()
}

fn gecko_tools() -> Result<&'static GeckoTools, &'static str> {
    GECKO_TOOLS
        .get_or_init(|| {
            let strict = std::env::var_os("JET_CANVAS_PREREQUISITES").as_deref()
                == Some(std::ffi::OsStr::new("strict"));
            let firefox = resolve_canvas_tool("firefox", strict)
                .ok_or_else(|| "Gecko smoke needs a valid Firefox executable".to_owned())?;
            let geckodriver = resolve_canvas_tool("geckodriver", strict)
                .ok_or_else(|| "Gecko smoke needs a valid geckodriver executable".to_owned())?;
            let node = resolve_canvas_tool("node", strict)
                .ok_or_else(|| "Gecko smoke needs a valid Node executable".to_owned())?;
            Ok(GeckoTools {
                firefox,
                geckodriver,
                node,
            })
        })
        .as_ref()
        .map_err(String::as_str)
}

fn resolve_canvas_tool(name: &str, strict: bool) -> Option<PathBuf> {
    let resolved_key = format!("JET_CANVAS_{}_RESOLVED", name.to_ascii_uppercase());
    let override_key = format!("JET_CANVAS_{}", name.to_ascii_uppercase());
    let candidate = if strict {
        std::env::var_os(&resolved_key)
    } else {
        std::env::var_os(&override_key).or_else(|| Some(name.into()))
    }?;
    let path = resolve_executable(Path::new(&candidate))?;
    let output = Command::new(&path).arg("--version").output().ok()?;
    let version = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let valid = match name {
        "chromium" => version.contains("Chromium") || version.contains("Chrome"),
        "firefox" => version.contains("Firefox"),
        "geckodriver" => version.starts_with("geckodriver"),
        "node" => version.starts_with('v')
            && version.as_bytes().get(1).is_some_and(u8::is_ascii_digit),
        _ => false,
    };
    valid.then_some(path)
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
}

impl Drop for CanvasCase {
    fn drop(&mut self) {
        if let Err(err) = fs::remove_dir_all(&self.dir) {
            if err.kind() != std::io::ErrorKind::NotFound {
                eprintln!(
                    "warning: could not remove Canvas scenario artifacts at {}: {err}",
                    self.dir.display()
                );
            }
        }
    }
}

struct DevServer {
    child: Child,
}

impl DevServer {
    fn start(repo: &Path, cwd: &Path, entry: &Path, port: u16) -> DevServer {
        let jet = canvas_cargo_target_dir(repo).join("debug/jet");
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
