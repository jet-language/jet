//! D-WEBBACKEND1 C4 (#704): real Chromium acceptance for the web backend.
//!
//! Proves, in a real browser against AOT `jet build --target=web` artifacts:
//! DOM create/update/remove, reactive rendering, event→Wasm callbacks, Wasm
//! compute, bundled artifacts, manifest-embedded source maps, and console
//! output parity with the node harnesses in `web_build.rs`.
//!
//! Dev-server diagnostics in a real browser remain in
//! `web_dev::jet_dev_web_browser_runs_hybrid_status_overlay_and_recovery_matrix`.

mod common;

use common::Scratch;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const NEST_P001_FORM: &str = include_str!("fixtures/nest_p001_form.jet");
const NEST_P001_INPUT_SHA256: &str =
    "72facb9077dbb3168de2278f426ef4ae4df4b48dd19ee03931b829463984772e";

fn assert_nest_p001_form_stages() {
    assert_eq!(NEST_P001_FORM.len(), 294);
    assert_eq!(
        jet::SHA256::sha256_hex(NEST_P001_FORM.as_bytes()),
        NEST_P001_INPUT_SHA256
    );

    let scratch = Scratch::new("nest-p001-form");
    fs::write(scratch.join("run.jet"), NEST_P001_FORM).expect("write NEST-P001 fixture");
    fs::write(
        scratch.join("package.jet"),
        "name: \"nest-p001\"\nversion: \"0.1.0\"\nedition: \"2026\"\nauthority: {\n    holds: {\n        allow: [IO, Mem.Alloc, Panic]\n    }\n}\n",
    )
    .expect("write NEST-P001 authority");
    let stages: &[(&str, &[&str])] = &[
        ("default", &["run", "run.jet"]),
        ("interpreter", &["run", "--interpret", "run.jet"]),
        ("profile-debug", &["run", "--profile=debug", "run.jet"]),
    ];
    for (stage, args) in stages {
        let output = Command::new(jet_bin())
            .current_dir(&scratch.path)
            .args(*args)
            .env("NO_COLOR", "1")
            .output()
            .unwrap_or_else(|error| panic!("run NEST-P001 {stage} stage: {error}"));
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "NEST-P001 {stage} stage failed:\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
        assert!(
            stdout.contains("name=Ada"),
            "NEST-P001 {stage} stage did not execute the typed field:\n{stdout}"
        );
        assert!(
            stdout.contains(
                r#"<form method="post" action="/actions/save" aria-busy="false">"#
            ),
            "NEST-P001 {stage} stage lost the generated form action:\n{stdout}"
        );
        assert!(
            stdout.contains(
                r#"<input name="name" id="Input-name" type="text" aria-label="name" aria-invalid="false" required value="Ada">"#
            ),
            "NEST-P001 {stage} stage lost the generated typed field:\n{stdout}"
        );
        assert!(
            stdout.contains(r#"<button type="submit">Submit</button>"#),
            "NEST-P001 {stage} stage lost the generated submit control:\n{stdout}"
        );
        let combined = format!("{stdout}{stderr}");
        assert!(
            !combined.contains("source bytes 0..0")
                && !combined.contains("selected entry is not a top-level function"),
            "NEST-P001 {stage} stage regressed to the old source-less lowering ICE:\n{combined}"
        );
    }
}

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
    src.push_str(include_str!(
        "../examples/features/web/web_wasm_callback.jet"
    ));
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
fn web_server_function_browser_adapter_keeps_native_and_scripted_paths_typed() {
    let Some(node) = resolve_executable(Path::new("node")) else {
        eprintln!("note: skipping server-function browser adapter smoke (need node)");
        return;
    };
    let runtime = repo_root()
        .join("crates/jet-codegen/src/Prelude/DomRuntime.js")
        .canonicalize()
        .expect("DomRuntime.js path");
    let runtime = runtime.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!(
        r#"
globalThis.location = {{ origin: "http://localhost", href: "http://localhost/" }};
const calls = [];
globalThis.fetch = async (url, options) => {{
  calls.push({{ url, options }});
  return new Response(JSON.stringify({{ saved: true }}), {{
    status: 200,
    headers: {{ "content-type": "application/json" }},
  }});
}};
const {{ attachWebForm, webServerFunctionCall, webServerFunctionForm }} =
  await import("file://{runtime}");
const boundary = {{
  name: "save",
  endpoint: "/actions/save",
  method: "POST",
  input: "SaveInput",
  output: "SaveResult",
  error: "SaveError",
  csrf: "same-origin",
  revalidate: ["orders"],
}};
const result = await webServerFunctionCall(boundary, {{ id: 7 }});
if (!result.value.saved || result.attempts !== 1 || result.revalidate[0] !== "orders") {{
  throw new Error("typed scripted call did not return its checked result");
}}
if (calls[0].options.credentials !== "same-origin" ||
    calls[0].options.headers["content-type"] !== "application/json") {{
  throw new Error("scripted call crossed the wrong transport boundary");
}}
const nativeAttrs = {{}};
const nativeForm = {{
  dataset: {{}},
  setAttribute(name, value) {{ nativeAttrs[name] = value; }},
  querySelector() {{ return null; }},
}};
const native = webServerFunctionForm(boundary, nativeForm);
if (!native.no_script || nativeAttrs.action !== "/actions/save" ||
    nativeAttrs.method !== "POST" || nativeForm.dataset.jetPending !== "false") {{
  throw new Error("native form path was not configured");
}}
let revalidated = null;
const enhancedAttrs = {{}};
const enhanced = {{
  dataset: {{}},
  setAttribute(name, value) {{ enhancedAttrs[name] = value; }},
  addEventListener(name, handler) {{ this.handler = handler; }},
  removeEventListener() {{}},
  dispatchEvent() {{}},
}};
attachWebForm(boundary, enhanced, {{
  enhance: true,
  encode: () => ({{ id: 7 }}),
  revalidate: (keys) => {{ revalidated = keys; }},
}});
enhanced.handler({{ preventDefault() {{}} }});
await new Promise((resolve) => setTimeout(resolve, 0));
if (!Array.isArray(revalidated) || revalidated[0] !== "orders" ||
    enhanced.dataset.jetPending !== "false") {{
  throw new Error("hydrated form did not settle and revalidate dependencies");
}}
try {{
  await webServerFunctionCall({{ endpoint: "https://evil.invalid/actions/save" }}, {{}});
  throw new Error("cross-origin server function was accepted");
}} catch (error) {{
  if (!(error instanceof TypeError) && error.code !== "csrf_rejected") throw error;
}}
const cycle = {{}};
cycle.self = cycle;
try {{
  await webServerFunctionCall(boundary, cycle);
  throw new Error("non-serializable input was accepted");
}} catch (error) {{
  if (error.code !== "invalid_input") throw error;
}}
"#,
    );
    let output = Command::new(node)
        .args(["--input-type=module", "-e", &script])
        .output()
        .expect("run server-function browser adapter smoke");
    assert!(
        output.status.success(),
        "server-function browser adapter smoke failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn tanstack_start_browser_reference_loop_uses_rendered_routes_and_actions() {
    if !have_tool("rustc") {
        eprintln!("note: skipping TanStack reference browser loop (need rustc)");
        return;
    }
    assert_nest_p001_form_stages();
    let Some((chromium, node)) = web_tools() else {
        eprintln!("note: skipping TanStack reference browser loop (need chromium + node)");
        return;
    };

    let repo = repo_root();
    let root = std::env::temp_dir().join(format!(
        "jet_tanstack_reference_browser_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("public")).unwrap();
    for path in ["run.jet", "package.jet", "public/index.html", "public/app.css"] {
        let source = repo.join("examples/features/web/tanstack_start").join(path);
        let destination = root.join(path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::copy(source, destination).unwrap();
    }

    let output = Command::new(&node)
        .env("CHROMIUM", chromium)
        .arg(repo.join("scripts/web-dev-test/reference_app.mjs"))
        .args(["--metric", "first_run", "--manifest"])
        .arg(repo.join("tools/agent-eval/dx/manifest.json"))
        .args(["--exercise", "--app", "tanstack-start-orders", "--jet-env"])
        .arg(repo.join("scripts/agent/jet-env"))
        .current_dir(&root)
        .output()
        .expect("run TanStack reference browser loop");
    assert!(
        output.status.success(),
        "TanStack reference browser loop failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("REFERENCE_METRIC:first_run"),
        "reference browser loop did not report first-run measurement"
    );
    let _ = fs::remove_dir_all(&root);
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
        String::from_utf8_lossy(&output.stdout)
            .contains("PASS web backend browser acceptance matrix"),
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
