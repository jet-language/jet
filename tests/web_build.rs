//! D-WEBBACKEND1 M2 (c123): `--target=web` WASM + JS artifact golden runs.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn have_tool(name: &str) -> bool {
    Command::new(name).arg("--version").output().is_ok()
}

fn build_web_fixture(stem: &str, src: &str, shown: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("jet_web_{stem}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::create_dir_all(dir.join("build")).unwrap();

    let out = jet::compile_web_with_path(src, shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected web fixture:\n{}",
            jet::render_diagnostics(shown, src, &diags)
        )
    });
    let web = out
        .web
        .expect("web target compile must produce web artifacts");
    assert!(
        web.manifest_json.contains("\"status\": \"m2\""),
        "manifest should be M2: {}",
        web.manifest_json
    );
    assert!(
        web.manifest_json.contains("\"partitions\""),
        "manifest missing partitions"
    );
    fs::write(dir.join("build/web.manifest.json"), &web.manifest_json).unwrap();
    fs::write(dir.join("build/jet_dom_runtime.js"), &web.dom_runtime).unwrap();
    fs::write(dir.join("build/app.js"), &web.js_app).unwrap();
    fs::write(dir.join("build/app_wasm.rs"), &web.wasm_rust).unwrap();

    let wasm_path = dir.join("build/app.wasm");
    let rustc = Command::new("rustc")
        .current_dir(&dir)
        .args([
            "--edition",
            "2021",
            "--target",
            "wasm32-unknown-unknown",
            "--crate-type",
            "cdylib",
            "-O",
            "build/app_wasm.rs",
            "-o",
            "build/app.wasm",
        ])
        .output()
        .unwrap();
    assert!(
        rustc.status.success(),
        "rustc rejected wasm for {stem}:\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    assert!(wasm_path.is_file(), "missing app.wasm for {stem}");
    dir
}

fn build_web_project(stem: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("jet_web_{stem}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("build")).unwrap();
    for (path, src) in files {
        fs::write(dir.join(path), src).unwrap();
    }

    let entry = dir.join("main.jet");
    let out = jet::compile_web(entry.to_str().unwrap()).unwrap_or_else(|diags| {
        panic!(
            "front end rejected web project:\n{}",
            jet::render_diagnostics(
                entry.to_str().unwrap(),
                &fs::read_to_string(&entry).unwrap(),
                &diags,
            )
        )
    });
    let web = out.web.expect("web target compile must produce web artifacts");
    fs::write(dir.join("build/web.manifest.json"), &web.manifest_json).unwrap();
    fs::write(dir.join("build/jet_dom_runtime.js"), &web.dom_runtime).unwrap();
    fs::write(dir.join("build/app.js"), &web.js_app).unwrap();
    fs::write(dir.join("build/app_wasm.rs"), &web.wasm_rust).unwrap();

    let rustc = Command::new("rustc")
        .current_dir(&dir)
        .args([
            "--edition",
            "2021",
            "--target",
            "wasm32-unknown-unknown",
            "--crate-type",
            "cdylib",
            "-O",
            "build/app_wasm.rs",
            "-o",
            "build/app.wasm",
        ])
        .output()
        .unwrap();
    assert!(
        rustc.status.success(),
        "rustc rejected wasm for {stem}:\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    dir
}

fn run_web_app(dir: &PathBuf) -> String {
    let node = Command::new("node")
        .current_dir(dir.join("build"))
        .arg("app.js")
        .output()
        .unwrap();
    assert!(
        node.status.success(),
        "node run failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&node.stdout),
        String::from_utf8_lossy(&node.stderr)
    );
    String::from_utf8_lossy(&node.stdout).into_owned()
}

/// A minimal, dependency-free `document` stub — just the API surface
/// `DomRuntime.js`'s `paint()`/`createBackend()` actually touches (see that
/// file's `jetDomContainer()`) — so the real-DOM code path can be exercised
/// under plain `node` (no browser, no new npm dependency) exactly the way a
/// real page would drive it: import an exported function, call it multiple
/// times as if in response to clicks, and observe the DOM tree it built.
const FAKE_DOM_HARNESS: &str = r#"
class FakeElement {
  constructor(tag) { this.tagName = tag; this.style = {}; this.dataset = {}; this.children = []; this.textContent = ""; this.id = ""; }
  appendChild(child) { this.children.push(child); return child; }
  querySelector(sel) {
    if (sel === "[data-jet-node]") return this.children.find((c) => c.dataset.jetNode) ?? null;
    return null;
  }
}
class FakeDocument {
  constructor() { this.body = new FakeElement("body"); this._byId = new Map(); }
  createElement(tag) { return new FakeElement(tag); }
  getElementById(id) { return this._byId.get(id) ?? null; }
}
const doc = new FakeDocument();
const origAppend = doc.body.appendChild.bind(doc.body);
doc.body.appendChild = (el) => { if (el.id) doc._byId.set(el.id, el); return origAppend(el); };
globalThis.document = doc;

const { render } = await import("./app.js");
for (const n of [0, 1, 2]) {
  render(n);
  const container = doc.getElementById("jet-app");
  const box = container.children.find((c) => c.dataset.jetNode);
  console.log(`click ${n}: children=${container.children.length} text=${JSON.stringify(box.textContent)} left=${box.style.left} top=${box.style.top} background=${box.style.background} color=${box.style.color}`);
}
"#;

/// Runs `FAKE_DOM_HARNESS` against the compiled `app.js` — the same click-then-
/// observe loop a real browser session would produce, proving `paint()` mounts
/// one real element and reuses it (not one-new-div-per-click) as the exported
/// `render(n)` is called repeatedly, the way a button's `onclick` calls it.
fn run_web_click_harness(dir: &PathBuf) -> String {
    let harness_path = dir.join("build/harness.mjs");
    fs::write(&harness_path, FAKE_DOM_HARNESS).unwrap();
    let node = Command::new("node")
        .current_dir(dir.join("build"))
        .arg("harness.mjs")
        .output()
        .unwrap();
    assert!(
        node.status.success(),
        "node harness failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&node.stdout),
        String::from_utf8_lossy(&node.stderr)
    );
    String::from_utf8_lossy(&node.stdout).into_owned()
}

const WEB_API_HARNESS: &str = r##"
class FakeElement {
  constructor(id) {
    this.id = id;
    this.value = "";
    this.textContent = "";
    this.listeners = new Map();
  }
  addEventListener(name, handler) {
    const list = this.listeners.get(name) ?? [];
    list.push(handler);
    this.listeners.set(name, list);
  }
  dispatchEvent(ev) {
    ev.target = this;
    for (const handler of this.listeners.get(ev.type) ?? []) handler(ev);
  }
}
class FakeDocument {
  constructor() {
    this.input = new FakeElement("new-task");
  }
  querySelector(sel) {
    if (sel === "#new-task") return this.input;
    return null;
  }
  getElementById(id) {
    return id === "new-task" ? this.input : null;
  }
  createElement(tag) {
    return new FakeElement(tag);
  }
}
class FakeStorage {
  constructor() { this.map = new Map(); }
  getItem(key) { return this.map.has(String(key)) ? this.map.get(String(key)) : null; }
  setItem(key, value) { this.map.set(String(key), String(value)); }
  removeItem(key) { this.map.delete(String(key)); }
  clear() { this.map.clear(); }
}
globalThis.document = new FakeDocument();
globalThis.localStorage = new FakeStorage();

const { init } = await import("./app.js");
init();
console.log(`tasks=${localStorage.getItem("tasks")}`);
document.input.value = "write flagship slice";
document.input.dispatchEvent({ type: "input" });
console.log(`draft=${localStorage.getItem("draft")}`);
"##;

fn run_web_api_harness(dir: &PathBuf) -> String {
    let harness_path = dir.join("build/web_api_harness.mjs");
    fs::write(&harness_path, WEB_API_HARNESS).unwrap();
    let node = Command::new("node")
        .current_dir(dir.join("build"))
        .arg("web_api_harness.mjs")
        .output()
        .unwrap();
    assert!(
        node.status.success(),
        "node web API harness failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&node.stdout),
        String::from_utf8_lossy(&node.stderr)
    );
    String::from_utf8_lossy(&node.stdout).into_owned()
}

/// D-UISHOWCASE1 (c134 Phase 8, flagship showcase — 197_ui_showcase.jet):
/// same fake-`document` trick as `FAKE_DOM_HARNESS`, but exercises the
/// dashboard's two independent entry points instead of one click-driven
/// `render(n)`. `initApp`/`initFuel` each return the real `Signal` object
/// `core.reactive.signal` compiles to (`jetDom.makeSignal` — a plain
/// `{get, set}` cell); this harness drives both directly, the same way
/// 197_ui_showcase.html's `requestAnimationFrame` loop and click handler do,
/// and reads every painted box back out of the fake DOM tree.
const SHOWCASE_HARNESS: &str = r#"
class FakeElement {
  constructor(tag) { this.tagName = tag; this.style = {}; this.dataset = {}; this.children = []; this.textContent = ""; this.id = ""; }
  appendChild(child) { this.children.push(child); return child; }
}
class FakeDocument {
  constructor() { this.body = new FakeElement("body"); this._byId = new Map(); }
  createElement(tag) { return new FakeElement(tag); }
  getElementById(id) { return this._byId.get(id) ?? null; }
}
const doc = new FakeDocument();
const origAppend = doc.body.appendChild.bind(doc.body);
doc.body.appendChild = (el) => { if (el.id) doc._byId.set(el.id, el); return origAppend(el); };
globalThis.document = doc;

const { initApp, initFuel } = await import("./app.js");
const boosts = initApp();
const elapsed = initFuel();

const container = doc.getElementById("jet-app");
const boxes = () => container.children.map((c) => ({ text: c.textContent, bg: c.style.background, color: c.style.color }));
const find = (prefix) => boxes().find((b) => b.text.startsWith(prefix));

console.log(`nodes=${container.children.length}`);
for (const b of boxes()) {
  console.log(`box text=${JSON.stringify(b.text)} bg=${b.bg} color=${b.color}`);
}

boosts.set(boosts.get() + 1);
boosts.set(boosts.get() + 1);
console.log(`boosts: ${find("Boosts").text}`);

for (const t of [0, 150, 300, 450, 600, 900]) {
  elapsed.set(t);
  console.log(`elapsed=${t}: ${find("Fuel").text}`);
}
"#;

fn run_showcase_harness(dir: &PathBuf) -> String {
    let harness_path = dir.join("build/showcase_harness.mjs");
    fs::write(&harness_path, SHOWCASE_HARNESS).unwrap();
    let node = Command::new("node")
        .current_dir(dir.join("build"))
        .arg("showcase_harness.mjs")
        .output()
        .unwrap();
    assert!(
        node.status.success(),
        "node showcase harness failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&node.stdout),
        String::from_utf8_lossy(&node.stderr)
    );
    String::from_utf8_lossy(&node.stdout).into_owned()
}

/// Smoothstep ease, mirroring 197_ui_showcase.jet's `tween(40.0, 95.0,
/// elapsed_ms, 600)` exactly (3t^2 - 2t^3, clamped at the ends) — the
/// independent oracle the harness's printed `Fuel: N%` lines are checked
/// against, so the assertion is "the interpolation math", not "whatever the
/// code happens to print".
fn expected_fuel_pct(elapsed_ms: i64) -> i64 {
    let (from, to, duration) = (40.0_f64, 95.0_f64, 600_i64);
    let value = if elapsed_ms <= 0 {
        from
    } else if elapsed_ms >= duration {
        to
    } else {
        let t = elapsed_ms as f64 / duration as f64;
        let eased = t * t * (3.0 - 2.0 * t);
        from + (to - from) * eased
    };
    value.trunc() as i64
}

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

#[test]
fn jet_cli_explain_partition_requires_web_target() {
    let jet = jet_bin();
    let out = Command::new(&jet)
        .args([
            "build",
            "--explain-partition",
            "examples/features/basics/hello.jet",
        ])
        .output()
        .unwrap();
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !combined.contains("isn't a flag"),
        "CLI should accept --explain-partition, got:\n{combined}"
    );
    assert!(
        combined.contains("`--explain-partition` requires `--target=web`"),
        "expected web-target guard, got:\n{combined}"
    );
}

#[test]
fn jet_cli_web_build_succeeds() {
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping jet CLI web build test");
        return;
    }
    let jet = jet_bin();
    let out = Command::new(&jet)
        .args([
            "build",
            "--target=web",
            "examples/features/web/web_compute.jet",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "jet CLI web build failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// D-WEBDEFAULT1 (ratified 2026-07-01, c134): a file-level `@Target(Web)` marker makes
/// `jet build <file>` (no `--target=` flag at all) infer the web backend.
#[test]
fn jet_cli_infers_web_target_from_file_marker() {
    if !have_tool("rustc") {
        eprintln!("note: skipping web-target-inference (file marker) test");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_web_target_marker_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("app.jet"),
        "@Target(Web)\nuse core.ui as ui\nfn run() {\n    b :: ui.null_backend()\n    n :: ui.node_color(\"hi\", 10.0, 5.0, \"#3366ff\")\n    c :: ui.constraint(0.0, 0.0, 50.0, 20.0)\n    s :: b.measure(n, c)\n    f :: ui.rect(0.0, 0.0, s.width, s.height)\n    b.layout(n, f)\n    b.paint(n)\n}\n",
    )
    .unwrap();

    let jet = jet_bin();
    let out = Command::new(&jet)
        .current_dir(&dir)
        .args(["build", "app.jet"]) // deliberately no --target=
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "jet build (no --target flag) should infer web from @Target(Web):\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        dir.join("build/app.js").is_file(),
        "no build/app.js — web backend wasn't inferred"
    );
    assert!(
        dir.join("build/app.wasm").is_file(),
        "no build/app.wasm — web backend wasn't inferred"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// D-WEBDEFAULT1 (ratified 2026-07-01, c134): a package's `pkg.jet` `target: "web"` makes
/// `jet build <file>` infer the web backend even with no file-level marker
/// and no `--target=` flag — the managed-package counterpart to the loose-
/// file marker above.
#[test]
fn jet_cli_infers_web_target_from_manifest() {
    if !have_tool("rustc") {
        eprintln!("note: skipping web-target-inference (manifest) test");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_web_target_manifest_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("pkg.jet"),
        "payload: {\n    name: \"webproj\",\n    version: \"0.1.0\",\n    target: \"web\",\n}\n",
    )
    .unwrap();
    fs::write(
        dir.join("main.jet"),
        "use core.ui as ui\nfn run() {\n    b :: ui.null_backend()\n    n :: ui.node_color(\"hi\", 10.0, 5.0, \"#3366ff\")\n    c :: ui.constraint(0.0, 0.0, 50.0, 20.0)\n    s :: b.measure(n, c)\n    f :: ui.rect(0.0, 0.0, s.width, s.height)\n    b.layout(n, f)\n    b.paint(n)\n}\n",
    )
    .unwrap();

    let jet = jet_bin();
    let out = Command::new(&jet)
        .current_dir(&dir)
        .args(["build", "main.jet"]) // deliberately no --target=
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "jet build (no --target flag) should infer web from pkg.jet target: \"web\":\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        dir.join("build/app.js").is_file(),
        "no build/app.js — web backend wasn't inferred from pkg.jet"
    );
    assert!(
        dir.join("build/app.wasm").is_file(),
        "no build/app.wasm — web backend wasn't inferred from pkg.jet"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// D-WEBDEFAULT1 (ratified 2026-07-01, c134): `jet run` never infers the web backend from
/// `@Target(Web)`, even with no `--target=` flag — "run" means "execute and
/// show console output," which a web build can't satisfy (there's no runtime
/// to run a `.wasm`+`.js` bundle as a console program). This is a real
/// regression that was caught while dogfooding the marker on 196/197: the
/// first cut of D-WEBDEFAULT1 inferred web for every command including
/// `run`, which made `jet run <file-with-@Target(Web)>` fail trying to exec
/// the wrong artifact as a native binary. `build`/`dev`/`check` still infer.
#[test]
fn jet_cli_run_never_infers_web_target_from_marker() {
    let dir = std::env::temp_dir().join(format!(
        "jet_web_target_run_never_infers_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        &dir.join("app.jet"),
        "@Target(Web)\nfn run() {\n    print(\"native\")\n}\n",
    )
    .unwrap();

    let jet = jet_bin();
    let out = Command::new(&jet)
        .current_dir(&dir)
        .args(["run", "app.jet"]) // no --target= — must stay native despite the marker
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "jet run should stay native for @Target(Web) (never infer for `run`):\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "native",
        "expected native program output, not a web-build side effect"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// D-HTMLPAIR1 (ratified 2026-07-01, c134): an explicit `@Html("path.html")` marker wins
/// over the `<stem>.html` sibling-filename convention.
#[test]
fn jet_cli_uses_explicit_html_marker() {
    if !have_tool("rustc") {
        eprintln!("note: skipping @Html marker test");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_html_marker_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("app.jet"),
        "@Target(Web)\n@Html(\"custom.html\")\nfn run() {}\n",
    )
    .unwrap();
    // A sibling `app.html` exists too — the explicit marker must win over it.
    fs::write(
        dir.join("app.html"),
        "<html>sibling, should be ignored</html>",
    )
    .unwrap();
    fs::write(dir.join("custom.html"), "<html>custom marker page</html>").unwrap();

    let jet = jet_bin();
    let out = Command::new(&jet)
        .current_dir(&dir)
        .args(["build", "app.jet"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "jet build with @Html marker failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let served = fs::read_to_string(dir.join("build/index.html")).unwrap();
    assert!(
        served.contains("custom marker page"),
        "expected the @Html(\"custom.html\") content, got:\n{served}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// D-HTMLPAIR1 (ratified 2026-07-01, c134): a `@Html(...)` path that doesn't exist is a
/// loud build error, never a silent fallback to the generic page.
#[test]
fn jet_cli_html_marker_missing_file_is_an_error() {
    if !have_tool("rustc") {
        eprintln!("note: skipping @Html missing-file test");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_html_marker_missing_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("app.jet"),
        "@Target(Web)\n@Html(\"does_not_exist.html\")\nfn run() {}\n",
    )
    .unwrap();

    let jet = jet_bin();
    let out = Command::new(&jet)
        .current_dir(&dir)
        .args(["build", "app.jet"])
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "jet build should fail loudly when @Html names a missing file"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("does_not_exist.html"),
        "error should name the missing file, got:\n{stderr}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn compile_web_file_loads() {
    let out = jet::compile_web("examples/features/web/web_compute.jet").expect("compile_web");
    assert!(out.web.is_some());
}

#[test]
fn web_body_outside_tir_is_diagnostic() {
    let src = include_str!("ui/web_tir_unsupported.jet");
    let diags = jet::compile_web_with_path(src, "tests/ui/web_tir_unsupported.jet")
        .expect_err("web compile should reject bodies outside TIR");
    assert!(
        diags.iter().any(|d| d.code == "E-WEB-TIR-UNSUPPORTED"),
        "expected E-WEB-TIR-UNSUPPORTED, got {diags:?}"
    );
}

#[test]
fn web_executable_emission_is_structurally_tir_only() {
    let source = include_str!("../crates/jet-codegen/src/Codegen/Web.rs");
    assert!(source.contains("tir: TIR::TFunc"), "web functions must retain lowered TFunc");
    assert!(
        source.contains("WebEmitResult<WebArtifacts>"),
        "validator/emitter drift must return a structured data fact"
    );
    for forbidden in [
        "body: Vec<Stmt>",
        "fn js_emit_expr(expr: &Expr",
        "fn wasm_emit_expr(expr: &Expr",
        "wasm_default",
        "\"undefined\".to_string()",
        "unreachable!(",
        "panic!(",
    ] {
        assert!(
            !source.contains(forbidden),
            "web executable emission regressed to AST/default fallback: {forbidden}"
        );
    }
}

#[test]
fn wasm_void_body_and_internal_helper_are_emitted_from_tir() {
    let src = r#"@Target(Web)
fn tick() {}
@WasmExport
fn ping() { tick() }
fn twice(n: Int) -> Int { return n * 2 }
@WasmExport
fn compute(n: Int) -> Int { return twice(n) }
fn run() {}
"#;
    let out = jet::compile_web_with_path(src, "tests/fixtures/web_wasm_helpers.jet")
        .expect("supported Wasm helpers should compile");
    let wasm = &out.web.expect("web artifacts").wasm_rust;
    assert!(wasm.contains("fn jet_wasm_tick()"));
    assert!(wasm.contains("jet_wasm_tick();"), "void body side effect was dropped:\n{wasm}");
    assert!(wasm.contains("fn jet_wasm_twice(user_n: i64) -> i64"));
    assert!(wasm.contains("jet_wasm_twice(user_n)"), "export did not call internal helper:\n{wasm}");
}

#[test]
fn web_inline_modules_keep_qualified_function_identity() {
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping web module identity test");
        return;
    }
    let src = r#"@Target(Web)
module left {
    @Target(Js)
    pub fn value() -> Int { return 1 }
}
module right {
    @Target(Js)
    pub fn value() -> Int { return 2 }
}
@Target(Js)
fn run() { print(left.value() + right.value()) }
"#;
    let dir = build_web_fixture("module_identity", src, "tests/fixtures/web_module_identity.jet");
    let js = fs::read_to_string(dir.join("build/app.js")).unwrap();
    assert!(js.contains("function left__value()"), "left module identity was dropped:\n{js}");
    assert!(js.contains("function right__value()"), "right module identity was dropped:\n{js}");
    assert_eq!(run_web_app(&dir), "3\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn web_wasm_inline_modules_emit_distinct_qualified_calls() {
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping web Wasm module identity test");
        return;
    }
    let src = r#"@Target(Web)
module left { pub fn value() -> Int { return 1 } }
module right { pub fn value() -> Int { return 2 } }
@WasmExport
fn total() -> Int { return left.value() + right.value() }
@Target(Js)
fn run() { print(total()) }
"#;
    let dir = build_web_fixture("wasm_module_identity", src, "tests/fixtures/web_wasm_module_identity.jet");
    let wasm = fs::read_to_string(dir.join("build/app_wasm.rs")).unwrap();
    assert!(wasm.contains("fn jet_wasm_left__value() -> i64"), "left Wasm identity was dropped:\n{wasm}");
    assert!(wasm.contains("fn jet_wasm_right__value() -> i64"), "right Wasm identity was dropped:\n{wasm}");
    assert!(wasm.contains("jet_wasm_left__value()"), "left qualified call was dropped:\n{wasm}");
    assert!(wasm.contains("jet_wasm_right__value()"), "right qualified call was dropped:\n{wasm}");
    assert_eq!(run_web_app(&dir), "3\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn web_file_modules_keep_qualified_js_function_identity() {
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping web file-module JS identity test");
        return;
    }
    let dir = build_web_project(
        "file_module_js_identity",
        &[
            (
                "main.jet",
                "@Target(Web)\nuse \"./left\" as left\nuse \"./right\" as right\n@Target(Js)\nfn run() { print(left.value() + right.value()) }\n",
            ),
            (
                "left.jet",
                "@Target(Js)\npub fn value() -> Int { return 1 }\n",
            ),
            (
                "right.jet",
                "@Target(Js)\npub fn value() -> Int { return 2 }\n",
            ),
        ],
    );
    let js = fs::read_to_string(dir.join("build/app.js")).unwrap();
    assert!(js.contains("function left__value()"), "left identity was dropped:\n{js}");
    assert!(js.contains("function right__value()"), "right identity was dropped:\n{js}");
    assert_eq!(run_web_app(&dir), "3\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn web_file_modules_emit_distinct_qualified_wasm_calls() {
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping web file-module Wasm identity test");
        return;
    }
    let dir = build_web_project(
        "file_module_wasm_identity",
        &[
            (
                "main.jet",
                "@Target(Web)\nuse \"./left\" as left\nuse \"./right\" as right\n@WasmExport\nfn total() -> Int { return left.value() + right.value() }\n@Target(Js)\nfn run() { print(total()) }\n",
            ),
            ("left.jet", "pub fn value() -> Int { return 1 }\n"),
            ("right.jet", "pub fn value() -> Int { return 2 }\n"),
        ],
    );
    let wasm = fs::read_to_string(dir.join("build/app_wasm.rs")).unwrap();
    assert!(wasm.contains("fn jet_wasm_left__value() -> i64"), "left identity was dropped:\n{wasm}");
    assert!(wasm.contains("fn jet_wasm_right__value() -> i64"), "right identity was dropped:\n{wasm}");
    assert!(wasm.contains("jet_wasm_left__value()"), "left call was dropped:\n{wasm}");
    assert!(wasm.contains("jet_wasm_right__value()"), "right call was dropped:\n{wasm}");
    assert_eq!(run_web_app(&dir), "3\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn web_file_module_wasm_export_uses_qualified_bridge() {
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping web file-module bridge test");
        return;
    }
    let dir = build_web_project(
        "file_module_bridge",
        &[
            (
                "main.jet",
                "@Target(Web)\nuse \"./math\" as math\n@Target(Js)\nfn run() { print(math.value()) }\n",
            ),
            (
                "math.jet",
                "@WasmExport\npub fn value() -> Int { return 7 }\n",
            ),
        ],
    );
    let js = fs::read_to_string(dir.join("build/app.js")).unwrap();
    assert!(
        js.contains("await bridge_math__value()"),
        "qualified call did not use Wasm bridge:\n{js}"
    );
    let wasm = fs::read_to_string(dir.join("build/app_wasm.rs")).unwrap();
    assert!(
        wasm.contains("pub extern \"C\" fn jet_export_math__value() -> i64"),
        "qualified export symbol was dropped:\n{wasm}"
    );
    assert_eq!(run_web_app(&dir), "7\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn web_file_module_same_leaf_partitions_ignore_load_order() {
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping web file-module partition identity test");
        return;
    }
    for (stem, imports) in [
        (
            "mixed_partition_left_first",
            "use \"./left\" as left\nuse \"./right\" as right",
        ),
        (
            "mixed_partition_right_first",
            "use \"./right\" as right\nuse \"./left\" as left",
        ),
    ] {
        let main = format!(
            "@Target(Web)\n{imports}\n@Target(Js)\nfn run() {{ print(left.value() + right.value()) }}\n"
        );
        let dir = build_web_project(
            stem,
            &[
                ("main.jet", &main),
                (
                    "left.jet",
                    "@Target(Js)\nfn helper() -> Int { return 1 }\n@Target(Js)\npub fn value() -> Int { return helper() }\n",
                ),
                (
                    "right.jet",
                    "fn helper() -> Int { return 2 }\n@WasmExport\npub fn value() -> Int { return helper() }\n",
                ),
            ],
        );
        let js = fs::read_to_string(dir.join("build/app.js")).unwrap();
        assert!(
            js.contains("function left__value()"),
            "JS sibling inherited Wasm bucket under {stem}:\n{js}"
        );
        assert!(
            js.contains("function left__helper()"),
            "JS local helper lost caller identity under {stem}:\n{js}"
        );
        assert!(
            js.contains("await bridge_right__value()"),
            "Wasm sibling inherited JS bucket under {stem}:\n{js}"
        );
        let wasm = fs::read_to_string(dir.join("build/app_wasm.rs")).unwrap();
        assert!(
            wasm.contains("jet_wasm_right__helper()"),
            "Wasm local helper lost caller identity under {stem}:\n{wasm}"
        );
        assert_eq!(run_web_app(&dir), "3\n", "load order changed behavior");
        let _ = fs::remove_dir_all(&dir);
    }
}

#[test]
fn module_local_run_cannot_hijack_web_entrypoint() {
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping web entrypoint identity test");
        return;
    }
    let src = r#"@Target(Web)
module helper { pub fn run() -> Int { return 7 } }
@Target(Js)
fn run() { print("top-level") }
"#;
    let dir = build_web_fixture("entry_identity", src, "tests/fixtures/web_entry_identity.jet");
    let manifest = fs::read_to_string(dir.join("build/web.manifest.json")).unwrap();
    assert!(manifest.contains("\"entry\": \"Js\""), "module-local run hijacked manifest entry:\n{manifest}");
    let js = fs::read_to_string(dir.join("build/app.js")).unwrap();
    assert!(!js.contains("wasm.jet_export_run()"), "module-local run hijacked JS startup:\n{js}");
    assert_eq!(run_web_app(&dir), "top-level\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn web_missing_return_is_a_preflight_diagnostic() {
    let src = "@Target(Web)\n@Target(Js)\nfn missing() -> Int { n :: 1 }\nfn run() {}\n";
    let diags = jet::compile_web_with_path(src, "tests/fixtures/web_missing_return.jet")
        .expect_err("non-void JS function without return must be rejected");
    assert!(diags.iter().any(|d| d.code == "E0114"), "{diags:?}");
}

#[test]
fn wasm_unsupported_export_abi_is_a_preflight_diagnostic() {
    let src = "@Target(Web)\n@WasmExport\nfn echo(s: ^String) -> String { return s }\nfn run() {}\n";
    let diags = jet::compile_web_with_path(src, "tests/fixtures/web_bad_wasm_abi.jet")
        .expect_err("unsupported Wasm ABI must be rejected before emission");
    assert!(diags.iter().any(|d| d.code == "E-WEB-TIR-UNSUPPORTED"), "{diags:?}");
}

#[test]
fn wasm_unsupported_internal_abi_is_a_preflight_diagnostic() {
    let src = "@Target(Web)\nfn helper(s: ^String) -> String { return s }\nfn run() {}\n";
    let diags = jet::compile_web_with_path(src, "tests/fixtures/web_bad_internal_wasm_abi.jet")
        .expect_err("unsupported internal Wasm ABI must be rejected before emission");
    assert!(diags.iter().any(|d| d.code == "E-WEB-TIR-UNSUPPORTED"), "{diags:?}");
}

#[test]
fn wasm_cross_bucket_call_is_a_normal_preflight_diagnostic() {
    let src = "@Target(Web)\n@Target(Js)\nfn browser_value() -> Int { return 1 }\n@WasmExport\nfn compute() -> Int { return browser_value() }\nfn run() {}\n";
    let diags = jet::compile_web_with_path(src, "tests/fixtures/web_cross_bucket_call.jet")
        .expect_err("Wasm must not call a JS-bucket function directly");
    assert!(diags.iter().any(|d| d.code == "E-WEB-CROSS-PARTITION"), "{diags:?}");
}

#[test]
fn canvas_style_wasm_tir_control_flow_and_print_compile() {
    let src = r#"@Target(Web)
fn square(n: Int) -> Int { return n * n }
fn summarize(limit: Int) -> Int {
    total := square(limit)
    if total > 10 { return total } else { return total + 1 }
}
fn scratch(limit: Int, text: String, flag: Bool, ratio: Float) { print(limit) }
fn run() { print(summarize(4)) }
"#;
    let out = jet::compile_web_with_path(src, "tests/fixtures/web_canvas_tir.jet")
        .expect("ordinary Canvas control flow and print must compile through TIR");
    let wasm = &out.web.expect("web artifacts").wasm_rust;
    assert!(wasm.contains("if (user_total > 10)"), "TIR if was not emitted:\n{wasm}");
    assert!(wasm.contains("println!(\"{}\""), "TIR print was not emitted:\n{wasm}");
    assert!(wasm.contains("user_text: String"), "internal owned String parameter was rejected:\n{wasm}");
}

#[test]
fn host_dev_entry_is_not_web_runtime_and_run_prints_literal_from_tir() {
    let src = r#"@Target(Web)
use core.web.devserver as devserver
@Target(Js)
fn dev() {
    server :: devserver.app()
    server.port(8080)
    server.serve()
}
module tools {
    fn dev() -> Int { return 7 }
}
fn run() { print("hello, web") }
"#;
    let out = jet::compile_web_with_path(src, "tests/fixtures/web_host_dev.jet")
        .expect("host dev entry and web run body must compile through their own execution paths");
    let web = out.web.expect("web artifacts");
    let wasm = &web.wasm_rust;
    assert!(wasm.contains("fn jet_wasm_tools__dev() -> i64"), "module tools.dev was not emitted:\n{wasm}");
    assert!(wasm.contains("println!(\"{}\", \"hello, web\")"), "literal TIR print was not emitted:\n{wasm}");
    let js = &web.js_app;
    assert!(!js.contains("function dev("), "top-level host dev leaked into JS runtime:\n{js}");
}

#[test]
fn default_wasm_top_level_dev_is_not_web_runtime() {
    let src = r#"@Target(Web)
use core.web.devserver as devserver
fn dev() {
    server :: devserver.app()
    server.port(8080)
    server.serve()
}
fn run() { print("hello") }
"#;
    let out = jet::compile_web_with_path(src, "tests/fixtures/web_host_dev_wasm.jet")
        .expect("default-Wasm host dev entry must stay outside web runtime");
    let wasm = &out.web.expect("web artifacts").wasm_rust;
    assert!(!wasm.contains("jet_wasm_dev"), "top-level host dev leaked into Wasm runtime:\n{wasm}");
}

#[test]
fn module_local_dev_is_validated_as_web_runtime() {
    let src = r#"@Target(Web)
module tools {
    fn dev(name: String) { print("hello {name}") }
}
fn run() { print("hello") }
"#;
    let diags = jet::compile_web_with_path(src, "tests/fixtures/web_module_dev_unsupported.jet")
        .expect_err("unsupported module-local dev body must not receive host-entry exemption");
    assert!(
        diags.iter().any(|d| d.code == "E-WEB-TIR-UNSUPPORTED" && d.what.contains("`dev`")),
        "{diags:?}"
    );
}

#[test]
fn web_hello_dom_shim_roundtrip() {
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping web_build hello (need rustc + node)");
        return;
    }
    let src = include_str!("../examples/features/web/web_hello.jet");
    let dir = build_web_fixture("hello", src, "examples/features/web/web_hello.jet");
    let stdout = run_web_app(&dir);
    let expected = include_str!("../examples/features/expected/web/web_hello.web.out");
    assert_eq!(stdout, expected);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn web_reactive_dom_snapshot_roundtrip() {
    // Phase 7 (c134): a `reactive.signal` + `ui.reactive_render` render loop
    // over the null/DOM backend, compiled to JS and actually executed under
    // `node` — the "web DOM snapshot" acceptance bar. `jetDom.commands()`
    // accumulates every paint, so the log after the second render includes
    // both the "hello" and "world" paints (deterministic reactive replay).
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping web_build reactive (need rustc + node)");
        return;
    }
    let src = include_str!("../examples/features/web/ui_web_reactive.jet");
    let dir = build_web_fixture("reactive", src, "examples/features/web/ui_web_reactive.jet");
    let stdout = run_web_app(&dir);
    let expected = include_str!("../examples/features/expected/web/ui_web_reactive.web.out");
    assert_eq!(stdout, expected);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn web_click_counter_dom_roundtrip() {
    // 196_ui_web_click.jet: every top-level `@Target(Js) fn` is exported (not just
    // `main`), and `paint()` mounts a real, reused DOM element when a
    // `document` exists. This proves both, end to end: a fake `document` (no
    // browser, no new dependency) stands in for the click-driven host page
    // (examples/features/web/ui_web_click.html), calling the exported
    // `render(n)` three times and observing the same element update in place.
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping web_build click counter (need rustc + node)");
        return;
    }
    let src = include_str!("../examples/features/web/ui_web_click.jet");
    let dir = build_web_fixture("click", src, "examples/features/web/ui_web_click.jet");
    let stdout = run_web_click_harness(&dir);
    let expected = include_str!("../examples/features/expected/web/ui_web_click.harness.out");
    assert_eq!(stdout, expected);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn web_events_and_storage_roundtrip() {
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping web_build browser API (need rustc + node)");
        return;
    }
    let src = r##"@Target(Web)
use core.web as web

@Target(Js)
fn init() {
    saved :: web.storage.local.get("tasks") ?? "[]"
    web.storage.local.set("tasks", saved)
    web.on("#new-task", "input", (ev) => {
        web.storage.local.set("draft", web.value("#new-task"))
    })
}

fn run() {}
"##;
    let dir = build_web_fixture("webapi", src, "tests/fixtures/web_api.jet");
    let stdout = run_web_api_harness(&dir);
    assert_eq!(
        stdout, "tasks=[]\ndraft=write flagship slice\n",
        "web.on/web.value/web.storage.local should roundtrip through generated JS"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn web_compute_wasm_bridge_roundtrip() {
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping web_build compute (need rustc + node)");
        return;
    }
    let src = include_str!("../examples/features/web/web_compute.jet");
    let dir = build_web_fixture("compute", src, "examples/features/web/web_compute.jet");
    let stdout = run_web_app(&dir);
    let expected = include_str!("../examples/features/expected/web/web_compute.out");
    assert_eq!(stdout, expected);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn codable_struct_wasm_bridge_reconstructs_typed_argument() {
    let src = include_str!("ui/web_abi_codable.jet");
    let dir = build_web_fixture("codable_struct", src, "tests/ui/web_abi_codable.jet");
    let wasm_rust = fs::read_to_string(dir.join("build/app_wasm.rs")).unwrap();
    let signature = "fn jet_export_sum_point(user_p_x: i64, user_p_y: i64) -> i64";
    let reconstruction = "let user_p = user_Point { user_x: user_p_x, user_y: user_p_y };";
    let field_read = "((user_p).user_x + (user_p).user_y)";
    assert!(wasm_rust.contains(signature), "flattened ABI drifted:\n{wasm_rust}");
    assert!(
        wasm_rust.find(reconstruction) < wasm_rust.find(field_read),
        "typed Point must be reconstructed before its fields are read:\n{wasm_rust}"
    );
    let stdout = run_web_app(&dir);
    assert_eq!(stdout, "7\n", "flattened Point arguments changed behavior");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn web_showcase_dashboard_roundtrip() {
    // 197_ui_showcase.jet (Tower c134 Phase 8 flagship): proves, end to end
    // under the fake-DOM harness, that (1) `initApp`/`initFuel` mount FOUR
    // distinct real DOM elements from FOUR independent `ui.null_backend()`
    // instances — not one element repeatedly overwritten, which is what the
    // pre-fix `paint()` (a single `[data-jet-node]` `querySelector` shared
    // across every backend) would have produced; (2) each card's typed
    // `Color` struct reaches the DOM as the exact right `background` hex,
    // via the real `to_hex` conversion; (3) the reactive `boosts` counter
    // box repaints through nothing but external `Signal.set()` calls; and
    // (4) the Fuel card's motion tween is *exactly* the smoothstep formula
    // (`expected_fuel_pct`) at every elapsed-time sample, not just "some
    // plausible-looking number".
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping web_build showcase dashboard (need rustc + node)");
        return;
    }
    let src = include_str!("../examples/features/web/ui_showcase.jet");
    let dir = build_web_fixture("showcase", src, "examples/features/web/ui_showcase.jet");
    let stdout = run_showcase_harness(&dir);
    let lines: Vec<&str> = stdout.lines().collect();

    assert_eq!(
        lines[0], "nodes=4",
        "expected 4 independently-mounted DOM boxes:\n{stdout}"
    );

    assert!(
        lines
            .iter()
            .any(|l| l.contains("\"Altitude: 12,400 ft\"") && l.contains("bg=#3366ff")),
        "missing/wrong Altitude card:\n{stdout}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("\"Airspeed: 410 kt\"") && l.contains("bg=#12b886")),
        "missing/wrong Airspeed card:\n{stdout}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("\"Boosts: 0\"") && l.contains("bg=#8855ee")),
        "missing/wrong initial Boosts card:\n{stdout}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("\"Fuel: 40%\"") && l.contains("bg=#e8790c")),
        "missing/wrong initial Fuel card:\n{stdout}"
    );

    assert!(
        lines.contains(&"boosts: Boosts: 2"),
        "reactive counter didn't repaint from two external Signal.set() calls:\n{stdout}"
    );

    for elapsed_ms in [0i64, 150, 300, 450, 600, 900] {
        let want = format!(
            "elapsed={elapsed_ms}: Fuel: {}%",
            expected_fuel_pct(elapsed_ms)
        );
        assert!(
            lines.contains(&want.as_str()),
            "tween mismatch at elapsed={elapsed_ms} — want line {want:?} in:\n{stdout}"
        );
    }

    let _ = fs::remove_dir_all(&dir);
}
