//! D-WEBBACKEND1 M2 (c123): `--target=web` WASM + JS artifact golden runs.

mod common;

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

#[test]
fn anonymous_unions_build_for_web() {
    let shown = "examples/features/types/anonymous_unions.jet";
    let src = fs::read_to_string(shown).unwrap();
    let dir = build_web_fixture("anonymous_unions", &src, shown);
    let wasm_rust = fs::read_to_string(dir.join("build/app_wasm.rs")).unwrap();
    assert!(wasm_rust.contains("__JetUnion_Int_String"));
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
  constructor(tag, doc) { this.tagName = tag; this.ownerDocument = doc; this.style = {}; this.dataset = {}; this.children = []; this.textContent = ""; this.id = ""; this.attrs = new Map(); }
  appendChild(child) { this.children.push(child); return child; }
  setAttribute(name, value) { this.attrs.set(String(name), String(value)); }
  getAttribute(name) { return this.attrs.get(String(name)) ?? null; }
  removeAttribute(name) { this.attrs.delete(String(name)); }
  focus() { this.ownerDocument.activeElement = this; }
}
class FakeDocument {
  constructor() { this.activeElement = null; this._byId = new Map(); this.body = new FakeElement("body", this); }
  createElement(tag) { return new FakeElement(tag, this); }
  getElementById(id) { return this._byId.get(id) ?? null; }
}
const doc = new FakeDocument();
const origAppend = doc.body.appendChild.bind(doc.body);
doc.body.appendChild = (el) => { if (el.id) doc._byId.set(el.id, el); return origAppend(el); };
globalThis.document = doc;

// The real companion HTML owns this control outside Jet's render tree.
const boostButton = doc.createElement("button");
boostButton.id = "boost-btn";
doc.body.appendChild(boostButton);

const { init_app, init_fuel } = await import("./app.js");
const boosts = init_app();
const elapsed = init_fuel();

const container = doc.getElementById("jet-app");
const boxes = () => container.children.map((c) => ({ text: c.textContent, bg: c.style.background, color: c.style.color, role: c.getAttribute("role"), aria: c.getAttribute("aria-label") }));
const find = (prefix) => boxes().find((b) => b.text.startsWith(prefix));

console.log(`nodes=${container.children.length}`);
for (const b of boxes()) {
  console.log(`box text=${JSON.stringify(b.text)} bg=${b.bg} color=${b.color} role=${b.role} aria=${JSON.stringify(b.aria)}`);
}

boosts.set(boosts.get() + 1);
boosts.set(boosts.get() + 1);
console.log(`boosts: ${find("Boosts").text}`);
boostButton.focus();
boosts.set(boosts.get() + 1);
console.log(`external-focus=${doc.activeElement === boostButton}`);

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

const TYPED_UI_TREE_HARNESS: &str = r#"
class FakeElement {
  constructor(tag, doc) { this.tagName = tag.toUpperCase(); this.ownerDocument = doc; this.style = {}; this.dataset = {}; this.children = []; this.textContent = ""; this.value = ""; this.id = ""; this.attrs = new Map(); this.parent = null; }
  appendChild(child) { child.parent = this; this.children.push(child); return child; }
  setAttribute(name, value) { this.attrs.set(String(name), String(value)); }
  getAttribute(name) { return this.attrs.get(String(name)) ?? null; }
  removeAttribute(name) { this.attrs.delete(String(name)); }
  focus() { this.ownerDocument.activeElement = this; }
  remove() { if (this.parent) this.parent.children = this.parent.children.filter((child) => child !== this); }
}
class FakeDocument {
  constructor() { this.activeElement = null; this._byId = new Map(); this.body = new FakeElement("body", this); }
  createElement(tag) { return new FakeElement(tag, this); }
  getElementById(id) { return this._byId.get(id) ?? null; }
}
const doc = new FakeDocument();
const origAppend = doc.body.appendChild.bind(doc.body);
doc.body.appendChild = (el) => { if (el.id) doc._byId.set(el.id, el); return origAppend(el); };
globalThis.document = doc;

const { render_tree, render_focus_tree, render_two_backends } = await import("./app.js");
render_tree(true);
let container = doc.getElementById("jet-app");
console.log(container.children.map((el) => `${el.tagName}:${el.getAttribute("role")}:${el.getAttribute("aria-label")}`).join("|"));
render_tree(false);
console.log(container.children.map((el) => `${el.tagName}:${el.getAttribute("role")}:${el.getAttribute("aria-label")}`).join("|"));
render_focus_tree();
const second = container.children.find((el) => el.getAttribute("aria-label") === "Cancel");
second.focus();
render_focus_tree();
console.log(`focus=${doc.activeElement === second}:${doc.activeElement?.getAttribute("aria-label")}`);
render_two_backends();
const backendA = container.children.find((el) => el.getAttribute("aria-label") === "Backend A");
backendA.focus();
render_two_backends();
console.log(`scoped=${doc.activeElement === backendA}:${doc.activeElement?.getAttribute("aria-label")}`);
"#;

fn run_typed_ui_tree_harness(dir: &PathBuf) -> String {
    let harness_path = dir.join("build/typed_ui_tree_harness.mjs");
    fs::write(&harness_path, TYPED_UI_TREE_HARNESS).unwrap();
    let node = Command::new("node")
        .current_dir(dir.join("build"))
        .arg("typed_ui_tree_harness.mjs")
        .output()
        .unwrap();
    assert!(
        node.status.success(),
        "typed UI tree harness failed:\nstdout: {}\nstderr: {}",
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

/// D-WEBDEFAULT1 (ratified 2026-07-01, c134): a file-level `#Target(Web)` marker makes
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
        "#Target(Web)\nuse core.ui as ui\nfn run() {\n    b :: ui.null_backend()\n    n :: ui.node_color(\"hi\", 10.0, 5.0, \"#3366ff\")\n    c :: ui.constraint(0.0, 0.0, 50.0, 20.0)\n    s :: b.measure(n, c)\n    f :: ui.rect(0.0, 0.0, s.width, s.height)\n    b.layout(n, f)\n    b.paint(n)\n}\n",
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
        "jet build (no --target flag) should infer web from #Target(Web):\nstdout: {}\nstderr: {}",
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

/// D-WEBDEFAULT1 (ratified 2026-07-01, c134): a package's `package.jet` `target: "web"` makes
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
        dir.join("package.jet"),
        "name: \"webproj\"\nversion: \"0.1.0\"\ntarget: \"web\"\n",
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
        "jet build (no --target flag) should infer web from package.jet target: \"web\":\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        dir.join("build/app.js").is_file(),
        "no build/app.js — web backend wasn't inferred from package.jet"
    );
    assert!(
        dir.join("build/app.wasm").is_file(),
        "no build/app.wasm — web backend wasn't inferred from package.jet"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// D-WEBDEFAULT1 (ratified 2026-07-01, c134): `jet run` never infers the web backend from
/// `#Target(Web)`, even with no `--target=` flag — "run" means "execute and
/// show console output," which a web build can't satisfy (there's no runtime
/// to run a `.wasm`+`.js` bundle as a console program). This is a real
/// regression that was caught while dogfooding the marker on 196/197: the
/// first cut of D-WEBDEFAULT1 inferred web for every command including
/// `run`, which made `jet run <file-with-#Target(Web)>` fail trying to exec
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
        "#Target(Web)\nfn run() {\n    print(\"native\")\n}\n",
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
        "jet run should stay native for #Target(Web) (never infer for `run`):\nstdout: {}\nstderr: {}",
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

/// D-HTMLPAIR1 (ratified 2026-07-01, c134): an explicit `#HTML("path.html")` marker wins
/// over the `<stem>.html` sibling-filename convention.
#[test]
fn jet_cli_uses_explicit_html_marker() {
    if !have_tool("rustc") {
        eprintln!("note: skipping #HTML marker test");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_html_marker_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("app.jet"),
        "#[Target(Web), HTML(\"custom.html\")]\nfn run() {}\n",
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
        "jet build with #HTML marker failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let served = fs::read_to_string(dir.join("build/index.html")).unwrap();
    assert!(
        served.contains("custom marker page"),
        "expected the #HTML(\"custom.html\") content, got:\n{served}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// D-HTMLPAIR1 (ratified 2026-07-01, c134): a `#HTML(...)` path that doesn't exist is a
/// loud build error, never a silent fallback to the generic page.
#[test]
fn jet_cli_html_marker_missing_file_is_an_error() {
    if !have_tool("rustc") {
        eprintln!("note: skipping #HTML missing-file test");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_html_marker_missing_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("app.jet"),
        "#[Target(Web), HTML(\"does_not_exist.html\")]\nfn run() {}\n",
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
        "jet build should fail loudly when #HTML names a missing file"
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

fn source_map_string_field<'a>(map: &'a str, field: &str) -> &'a str {
    let prefix = format!("\"{field}\":\"");
    let value = map
        .split_once(&prefix)
        .unwrap_or_else(|| panic!("missing source-map field `{field}`:\n{map}"))
        .1;
    value
        .split_once('"')
        .unwrap_or_else(|| panic!("unterminated source-map field `{field}`:\n{map}"))
        .0
}

fn decode_source_map_mappings(map: &str) -> Vec<(usize, usize, usize, usize, usize)> {
    fn digit(byte: u8) -> i64 {
        match byte {
            b'A'..=b'Z' => i64::from(byte - b'A'),
            b'a'..=b'z' => i64::from(byte - b'a') + 26,
            b'0'..=b'9' => i64::from(byte - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            _ => panic!("invalid base64 VLQ byte: {byte}"),
        }
    }

    fn segment(encoded: &str) -> Vec<i64> {
        let mut values = Vec::new();
        let mut value = 0i64;
        let mut shift = 0;
        for byte in encoded.bytes() {
            let digit = digit(byte);
            value |= (digit & 31) << shift;
            if digit & 32 == 0 {
                let negative = value & 1 == 1;
                let magnitude = value >> 1;
                values.push(if negative { -magnitude } else { magnitude });
                value = 0;
                shift = 0;
            } else {
                shift += 5;
            }
        }
        assert_eq!(shift, 0, "unterminated base64 VLQ segment: {encoded}");
        values
    }

    let mut out = Vec::new();
    let mut source = 0i64;
    let mut original_line = 0i64;
    let mut original_column = 0i64;
    for (generated_line, line) in source_map_string_field(map, "mappings").split(';').enumerate() {
        let mut generated_column = 0i64;
        for encoded in line.split(',').filter(|segment| !segment.is_empty()) {
            let values = segment(encoded);
            assert_eq!(values.len(), 4, "expected an unmapped-name segment");
            generated_column += values[0];
            source += values[1];
            original_line += values[2];
            original_column += values[3];
            out.push((
                generated_line,
                usize::try_from(generated_column).unwrap(),
                usize::try_from(source).unwrap(),
                usize::try_from(original_line).unwrap(),
                usize::try_from(original_column).unwrap(),
            ));
        }
    }
    out
}

#[test]
fn js_source_map_uses_line_markers_and_hides_host_paths() {
    let src = "#Target(Web)\n#Target(JS)\nfn run() {\n\n    first :: 1\n    print(\"left {first}\\n//# __jet_source_map line 999\\nright\")\n}\n";
    let shown = format!(
        "{}/private/build-host/project/main.jet",
        std::env::temp_dir().display()
    );
    let first = jet::compile_web_with_path(src, &shown)
        .expect("web source-map fixture")
        .web
        .expect("web artifacts");
    let second = jet::compile_web_with_path(src, &shown)
        .expect("repeat web source-map fixture")
        .web
        .expect("web artifacts");

    assert_eq!(first.js_app, second.js_app);
    assert_eq!(first.js_source_map, second.js_source_map);
    assert!(!first.js_app.contains("sourceMappingURL="));
    assert!(first.js_source_map.starts_with(
        "{\"version\":3,\"file\":\"app.js\",\"sources\":[\"main.jet\"],\"sourcesContent\":["
    ));
    assert!(
        first
            .js_source_map
            .contains("\"#Target(Web)\\n#Target(JS)\\nfn run() {\\n\\n    first :: 1\\n    print(\\\"left {first}\\\\n//# __jet_source_map line 999\\\\nright\\\")\\n}\\n\""),
        "sourcesContent must contain the exact Jet bytes:\n{}",
        first.js_source_map
    );
    assert!(!first.js_source_map.contains("/private/build-host"));
    assert!(
        first.js_app.contains("//# __jet_source_map line 999"),
        "user text must survive internal marker stripping:\n{}",
        first.js_app
    );

    let mappings = decode_source_map_mappings(&first.js_source_map);
    assert_eq!(
        mappings
            .iter()
            .map(|(_, _, source, original_line, original_column)| {
                (*source, *original_line, *original_column)
            })
            .collect::<Vec<_>>(),
        vec![(0, 4, 0), (0, 5, 0)]
    );
    for (generated_line, generated_column, _, _, _) in mappings {
        let line = first.js_app.lines().nth(generated_line).unwrap();
        let first_code = line.find(|c: char| !c.is_whitespace()).unwrap();
        assert_eq!(generated_column, line[..first_code].encode_utf16().count());
        assert!(
            line[first_code..].starts_with("let first =")
                || line[first_code..].starts_with("jetDom.print(`left ${first}"),
            "mapping does not point at a Jet statement: {line}"
        );
    }
}

#[test]
fn web_build_publishes_maps_and_release_omits_them() {
    if !have_tool("rustc") {
        eprintln!("note: skipping web map publication test (need rustc)");
        return;
    }
    let src = "#Target(Web)\n#Target(JS)\nfn run() {\n    print(\"hi\")\n}\n";
    let dir = std::env::temp_dir().join(format!("jet_web_maps_pub_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("main.jet"), src).unwrap();

    let jet = PathBuf::from(env!("CARGO_BIN_EXE_jet"));

    let default_out = Command::new(&jet)
        .current_dir(&dir)
        .args(["build", "--target=web", "main.jet"])
        .output()
        .expect("jet build --target=web");
    assert!(
        default_out.status.success(),
        "default web build failed:\n{}",
        String::from_utf8_lossy(&default_out.stderr)
    );
    let js = fs::read_to_string(dir.join("build/app.js")).unwrap();
    assert!(js.contains("//# sourceMappingURL=app.js.map"), "missing js map URL:\n{js}");
    let js_map = fs::read_to_string(dir.join("build/app.js.map")).unwrap();
    assert!(js_map.contains("\"version\":3"), "{js_map}");
    assert!(js_map.contains("\"sourcesContent\":["), "{js_map}");
    assert!(!js_map.contains("/home/"), "host path leaked into js map");
    let wasm_map = fs::read_to_string(dir.join("build/app.wasm.map")).unwrap();
    assert!(wasm_map.contains("\"file\":\"app.wasm\""), "{wasm_map}");
    assert!(wasm_map.contains("\"sourcesContent\":["), "{wasm_map}");
    let wasm = fs::read(dir.join("build/app.wasm")).unwrap();
    assert!(
        wasm.windows(b"sourceMappingURL".len())
            .any(|w| w == b"sourceMappingURL"),
        "wasm missing sourceMappingURL custom section"
    );
    assert!(
        wasm.windows(b"app.wasm.map".len())
            .any(|w| w == b"app.wasm.map"),
        "wasm missing relative map URL"
    );

    let release_out = Command::new(&jet)
        .current_dir(&dir)
        .args(["build", "--target=web", "--profile=release", "main.jet"])
        .output()
        .expect("jet build --target=web --profile=release");
    assert!(
        release_out.status.success(),
        "release web build failed:\n{}",
        String::from_utf8_lossy(&release_out.stderr)
    );
    let release_js = fs::read_to_string(dir.join("build/app.js")).unwrap();
    assert!(
        !release_js.contains("sourceMappingURL="),
        "release app.js must not reference maps"
    );
    assert!(!dir.join("build/app.js.map").is_file(), "release must not write app.js.map");
    assert!(!dir.join("build/app.wasm.map").is_file(), "release must not write app.wasm.map");
    let release_manifest = fs::read_to_string(dir.join("build/web.manifest.json")).unwrap();
    assert!(
        !release_manifest.contains("\"sourceMap\""),
        "release manifest must omit sourceMap"
    );
    let release_wasm = fs::read(dir.join("build/app.wasm")).unwrap();
    assert!(
        !release_wasm
            .windows(b"sourceMappingURL".len())
            .any(|w| w == b"sourceMappingURL"),
        "release wasm must not embed sourceMappingURL"
    );

    // Wasm-export body must produce non-empty app.wasm.map mappings.
    let wasm_src = "#Target(Web)\n#WasmExport\nfn add(a: Int, b: Int) => Int { return a + b }\nfn run() {}\n";
    fs::write(dir.join("wasm.jet"), wasm_src).unwrap();
    let wasm_out = Command::new(&jet)
        .current_dir(&dir)
        .args(["build", "--target=web", "wasm.jet"])
        .output()
        .expect("jet build wasm export");
    assert!(
        wasm_out.status.success(),
        "wasm export build failed:\n{}",
        String::from_utf8_lossy(&wasm_out.stderr)
    );
    let wasm_map2 = fs::read_to_string(dir.join("build/app.wasm.map")).unwrap();
    let mappings_field = wasm_map2
        .split("\"mappings\":")
        .nth(1)
        .and_then(|rest| rest.split(',').next())
        .unwrap_or("");
    assert!(
        mappings_field.contains('"') && mappings_field.len() > 3,
        "wasm export map must carry mappings:\n{wasm_map2}"
    );
    let _ = fs::remove_dir_all(&dir);
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
fn web_unsupported_tui_backend_method_is_preflight_diagnostic() {
    let src = r#"#Target(Web)
use core.ui as ui

#Target(JS)
fn run() {
    b :: ui.tui_backend()
    print(b.frame_lines())
}
"#;
    let diags = jet::compile_web_with_path(src, "tests/fixtures/web_tui_frame_lines.jet")
        .expect_err("Tui-only backend methods must not pass JS preflight");
    assert!(
        diags.iter().any(|d| d.code == "E-WEB-TIR-UNSUPPORTED"),
        "{diags:?}"
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
    let src = r#"#Target(Web)
fn tick() {}
#WasmExport
fn ping() { tick() }
fn twice(n: Int) => Int { return n * 2 }
#WasmExport
fn compute(n: Int) => Int { return twice(n) }
fn run() {}
"#;
    let out = jet::compile_web_with_path(src, "tests/fixtures/web_wasm_helpers.jet")
        .expect("supported Wasm helpers should compile");
    let wasm = &out.web.expect("web artifacts").wasm_rust;
    assert!(wasm.contains("fn jet_wasm_tick()"));
    assert!(wasm.contains("jet_wasm_tick();"), "void body side effect was dropped:\n{wasm}");
    assert!(wasm.contains("fn jet_wasm_twice(__jet_n: i64) -> i64"));
    assert!(wasm.contains("jet_wasm_twice(__jet_n)"), "export did not call internal helper:\n{wasm}");
}

#[test]
fn web_value_and_range_arm_tables_emit_on_js_and_wasm() {
    // D-IF3 arm tables must share the MixedSwitch / RangeSwitch path with native.
    let src = r#"#Target(Web)
#Target(JS)
fn digit(n: Int) => String {
    if n == {
        0 -> return "0"
        1 -> return "1"
        else -> return "x"
    }
}
#Target(JS)
fn band(n: Int) => String {
    if n == {
        0..9 -> return "low"
        else -> return "high"
    }
}
#WasmExport
fn wasm_digit(n: Int) => String {
    if n == {
        0 -> return "0"
        1 -> return "1"
        else -> return "x"
    }
}
#WasmExport
fn wasm_band(n: Int) => String {
    if n == {
        0..9 -> return "low"
        10..19 -> return "mid"
        else -> return "high"
    }
}
fn run() {}
"#;
    let out = jet::compile_web_with_path(src, "tests/fixtures/web_arm_tables.jet")
        .expect("value/range arm tables must compile on web");
    let web = out.web.expect("web artifacts");
    assert!(
        web.js_app.contains("} else if"),
        "JS MixedSwitch must emit an if/else-if chain:\n{}",
        web.js_app
    );
    assert!(
        web.js_app.contains("__jet_switch_subject"),
        "JS arm tables must bind the switch subject:\n{}",
        web.js_app
    );
    assert!(
        web.wasm_rust.contains("__jet_switch_subject"),
        "Wasm MixedSwitch/RangeSwitch must bind the switch subject:\n{}",
        web.wasm_rust
    );
    assert!(
        web.wasm_rust.contains("} else if"),
        "Wasm arm tables must emit an if/else-if chain:\n{}",
        web.wasm_rust
    );
    assert!(
        web.wasm_rust.contains(">= 0") && web.wasm_rust.contains("<= 9"),
        "Wasm RangeSwitch must emit inclusive range tests:\n{}",
        web.wasm_rust
    );
}

#[test]
fn web_loops_and_index_assign_emit_on_js_and_wasm() {
    let src = r#"#Target(Web)
#Target(JS)
fn sum_to(n: Int) => Int {
    total := 0
    i := 0
    loop {
        if i >= n { break }
        total = total + i
        i = i + 1
    }
    return total
}
#Target(JS)
fn sum_while(n: Int) => Int {
    total := 0
    i := 0
    loop i < n {
        i += 1
        if i == 2 { next }
        total += i
    }
    return total
}
#Target(JS)
fn sum_counted(n: Int) => Int {
    total := 0
    loop i, 0..<n { total += i }
    return total
}
#Target(JS)
fn bump(n: Int) => Int {
    xs := [n]
    xs[0] = 9
    return xs[0]
}
#WasmExport
fn wasm_sum_to(n: Int) => Int {
    total := 0
    i := 0
    loop {
        if i >= n { break }
        total = total + i
        i = i + 1
    }
    return total
}
#WasmExport
fn wasm_sum_while(n: Int) => Int {
    total := 0
    i := 0
    loop i < n {
        i += 1
        if i == 2 { next }
        total += i
    }
    return total
}
#WasmExport
fn wasm_sum_counted(n: Int) => Int {
    total := 0
    loop i, 0..<n { total += i }
    return total
}
#WasmExport
fn wasm_bump(n: Int) => Int {
    xs := [n]
    xs[0] = 9
    return xs[0]
}
fn run() {}
"#;
    let out = jet::compile_web_with_path(src, "tests/fixtures/web_loops_index.jet")
        .expect("loops + index assign must compile on web");
    let web = out.web.expect("web artifacts");
    assert!(
        web.js_app.contains("while (true)"),
        "JS Loop must emit while(true):\n{}",
        web.js_app
    );
    assert!(
        web.js_app.contains("break;"),
        "JS Break must emit break:\n{}",
        web.js_app
    );
    assert!(
        web.js_app.contains("while ((i < n))")
            && web.js_app.contains("continue;")
            && web.js_app.contains("__jet_loop_first"),
        "JS While/Continue/CountedLoop forms were not all emitted:\n{}",
        web.js_app
    );
    assert!(
        web.js_app.contains("] = ") || web.js_app.contains("[0] ="),
        "JS IndexAssign must emit array write:\n{}",
        web.js_app
    );
    assert!(
        web.wasm_rust.contains("loop {") || web.wasm_rust.contains("loop{"),
        "Wasm Loop must emit loop:\n{}",
        web.wasm_rust
    );
    assert!(
        web.wasm_rust.contains("break;"),
        "Wasm Break must emit break:\n{}",
        web.wasm_rust
    );
    assert!(
        web.wasm_rust.contains("while (__jet_i < __jet_n)")
            && web.wasm_rust.contains("continue;")
            && web.wasm_rust.contains("__jet_loop_first"),
        "Wasm While/Continue/CountedLoop forms were not all emitted:\n{}",
        web.wasm_rust
    );
    assert!(
        web.wasm_rust.contains("as usize] ="),
        "Wasm IndexAssign must emit list write:\n{}",
        web.wasm_rust
    );
}

#[test]
fn web_fallible_match_and_option_if_emit_on_js_and_wasm() {
    let src = r#"#Target(Web)
enum Toggle { On Off }

#Target(JS)
fn make_result(flag: Bool) => Int ? String {
    if flag return .Ok(7)
    return .Err("x")
}
#Target(JS)
fn make_opt(flag: Bool) => Int? {
    if flag return .Val(3)
    return .None
}
#Target(JS)
fn classify(flag: Bool) => Int {
    r :: make_result(flag)
    if r == {
        .Ok(n) -> return n
        .Err(_) -> return -1
    }
}
#Target(JS)
fn maybe(flag: Bool) => Int {
    x :: make_opt(flag)
    if x == .None { return 0 }
    if x == .Val(n) { return n }
    return -1
}
#Target(JS)
fn js_make_toggle(flag: Bool) => Toggle {
    if flag { return .On }
    return .Off
}
#Target(JS)
fn js_matches_and(flag: Bool) => Int {
    x :: make_opt(flag)
    toggle :: js_make_toggle(flag)
    if x == .Val(n) && toggle == .On && flag { return n }
    return 0
}
#Target(Wasm)
fn wasm_make_result(flag: Bool) => Int ? String {
    if flag return .Ok(7)
    return .Err("x")
}
#Target(Wasm)
fn wasm_make_opt(flag: Bool) => Int? {
    if flag return .Val(3)
    return .None
}
#WasmExport
fn wasm_classify(flag: Bool) => Int {
    r :: wasm_make_result(flag)
    if r == {
        .Ok(n) -> return n
        .Err(_) -> return -1
    }
}
#WasmExport
fn wasm_maybe(flag: Bool) => Int {
    x :: wasm_make_opt(flag)
    if x == .None { return 0 }
    if x == .Val(n) { return n }
    return -1
}
#Target(Wasm)
fn wasm_make_toggle(flag: Bool) => Toggle {
    if flag { return .On }
    return .Off
}
#WasmExport
fn wasm_matches_and(flag: Bool) => Int {
    x :: wasm_make_opt(flag)
    toggle :: wasm_make_toggle(flag)
    if x == .Val(n) && toggle == .On && flag { return n }
    return 0
}
fn run() {}
"#;
    let out = jet::compile_web_with_path(src, "tests/fixtures/web_fallible_match.jet")
        .expect("Result/Option match must compile on web");
    let web = out.web.expect("web artifacts");
    assert!(
        web.js_app.contains("tag === \"Ok\"") && web.js_app.contains("tag === \"Err\""),
        "JS EnumMatch must emit Ok and Err branches:\n{}",
        web.js_app
    );
    assert!(
        web.js_app.contains("tag: \"None\"") || web.js_app.contains("\"None\""),
        "JS must emit None tag:\n{}",
        web.js_app
    );
    assert!(
        web.js_app.contains("tag === \"On\"") && web.js_app.contains("if (flag)"),
        "JS Matches + And must emit both short-circuit tests:\n{}",
        web.js_app
    );
    assert!(
        web.wasm_rust.contains("Ok(") && web.wasm_rust.contains("Err("),
        "Wasm must emit Ok/Err patterns:\n{}",
        web.wasm_rust
    );
    assert!(
        web.wasm_rust.contains("Some(") && web.wasm_rust.contains("None"),
        "Wasm must emit Some/None:\n{}",
        web.wasm_rust
    );
    assert!(
        web.wasm_rust.contains("matches!(&(__jet_toggle), __jet_Toggle::__jet_On)")
            && web.wasm_rust.contains("if __jet_flag"),
        "Wasm Matches + And must emit both short-circuit tests:\n{}",
        web.wasm_rust
    );
}

#[test]
fn web_js_iflet_evaluates_subject_once_and_checks_payload_ranges() {
    let src = r#"#Target(Web)
enum Packet {
    Data(Int)
    Empty
}

#Target(JS)
fn make_packet(n: Int) => Packet = .Data(n)

#Target(JS)
fn make_opt(n: Int) => Int? = .Val(n)

#Target(JS)
fn bind_opt(n: Int) => Int {
    if make_opt(n) == .Val(value) { return value }
    return 0
}

#Target(JS)
fn classify_range(n: Int) => Int {
    packet :: make_packet(n)
    if packet == {
        .Data(1..3) -> return 1
        else -> return 0
    }
}

#Target(Wasm)
fn wasm_packet(n: Int) => Packet = .Data(n)

#WasmExport
fn wasm_classify(n: Int) => Int {
    packet :: wasm_packet(n)
    if packet == {
        .Data(1..3) -> return 1
        else -> return 0
    }
}

fn run() {}
"#;
    let out = jet::compile_web_with_path(src, "tests/fixtures/web_iflet_ranges.jet")
        .expect("supported binding and range patterns must compile");
    let web = out.web.expect("web artifacts");
    assert_eq!(
        web.js_app.matches("make_opt(n)").count(),
        2,
        "IfLet subject must be evaluated once:\n{}",
        web.js_app
    );
    assert!(
        web.js_app.contains("values[0] >= 1")
            && web.js_app.contains("values[0] <= 3"),
        "JS EnumMatch must test payload ranges:\n{}",
        web.js_app
    );
    assert!(
        web.wasm_rust.contains("_ @ 1..=3"),
        "Wasm EnumMatch must retain payload range:\n{}",
        web.wasm_rust
    );
}

#[test]
fn web_js_matches_binds_subject_once() {
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping JS Matches single-evaluation test");
        return;
    }
    let src = r#"#Target(Web)
enum Toggle { On Off }

#Target(JS)
fn make_opt(n: Int) => Int? = .Val(n)

#Target(JS)
fn make_toggle() => Toggle = .On

#Target(JS)
fn classify(n: Int) => Int {
    opt :: make_opt(n)
    toggle :: make_toggle()
    if opt == .Val(value) && toggle == .On { return value }
    return 0
}

#Target(JS)
fn run() { print(classify(2)) }
"#;
    let dir = build_web_fixture(
        "matches_once",
        src,
        "tests/fixtures/web_matches_once.jet",
    );
    let js = fs::read_to_string(dir.join("build/app.js")).unwrap();
    assert_eq!(
        js.matches("const __jet_match_subject = toggle;").count(),
        1,
        "Matches subject must be bound once before its predicate:\n{js}"
    );
    assert_eq!(
        js.matches("(__jet_match_subject)").count(),
        1,
        "the predicate must read only the bound subject:\n{js}"
    );
    assert_eq!(run_web_app(&dir), "2\n");
    let _ = fs::remove_dir_all(dir);
}

/// I9 / #1485: whole numbers on the JS tier are BigInt, so powers (and
/// follow-on arithmetic) stay exact above 2^53 — the same answers AOT prints.
#[test]
fn web_js_whole_numbers_stay_exact_above_two_to_the_fifty_three() {
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping JS BigInt exactness test (need rustc + node)");
        return;
    }
    // File-level `#Target(JS)` puts `run` on the plain-JS path (no Wasm body).
    let src = r#"#Target(JS)
fn run() {
    print(7 / 2)
    print(2 ^ 60)
    print(3 ^ 39)
    print(2 ^ 60 > 2 ^ 53)
    wide := 2 ^ 60
    print(wide + 1)
    print(wide /% 10)
}
"#;
    let dir = build_web_fixture(
        "js_bigint_exact",
        src,
        "tests/fixtures/web_js_bigint_exact.jet",
    );
    assert_eq!(
        run_web_app(&dir),
        "3.5\n1152921504606846976\n4052555153018976267\ntrue\n1152921504606846977\n115292150460684697\n",
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn web_js_enum_match_break_targets_the_enclosing_loop() {
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping JS EnumMatch break test");
        return;
    }
    let src = r#"#Target(Web)
enum Choice { Stop Keep }

#Target(JS)
fn count(choice: Choice) => Int {
    hits := 0
    loop i, 0..<3 {
        if choice == {
            .Stop -> { break }
            .Keep -> {}
        }
        hits += 1
    }
    return hits
}

#Target(JS)
fn run() { print(count(.Stop)) }
"#;
    let dir = build_web_fixture(
        "enum_match_break",
        src,
        "tests/fixtures/web_enum_match_break.jet",
    );
    let js = fs::read_to_string(dir.join("build/app.js")).unwrap();
    assert!(
        !js.contains("switch (__jet_match.tag)"),
        "EnumMatch switch captures an unlabeled Jet break:\n{js}"
    );
    assert_eq!(run_web_app(&dir), "0\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn web_for_in_preflight_rejects_unimplemented_iteration_fields() {
    for (shown, src) in [
        (
            "tests/fixtures/web_for_in_step.jet",
            r#"#Target(Web)
#Target(JS)
fn sum() => Int {
    total := 0
    loop value, [1, 2, 3], 2 { total += value }
    return total
}
fn run() {}
"#,
        ),
        (
            "tests/fixtures/web_for_in_method.jet",
            r#"#Target(Web)
#Target(JS)
fn count_chars() => Int {
    total := 0
    loop ch, "abc".chars() { total += 1 }
    return total
}
fn run() {}
"#,
        ),
    ] {
        let diags = jet::compile_web_with_path(src, shown)
            .expect_err("JS must reject ForIn forms its emitter does not implement");
        assert!(
            diags.iter().any(|d| d.code == "E-WEB-TIR-UNSUPPORTED"),
            "{shown}: {diags:?}"
        );
    }
}

#[test]
fn web_if_expr_emits_safe_js_and_wasm_value_blocks() {
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping web IfExpr test");
        return;
    }
    let src = r#"#Target(Web)
#WasmExport
fn wasm_pick(flag: Bool) => Int {
    return if flag -> {
        n :: 6
        n + 1
    } else -> 3
}

#Target(JS)
fn js_pick(flag: Bool) => Int {
    return if flag -> {
        n :: 6
        n + 2
    } else -> 4
}

#Target(JS)
fn run() {
    print(js_pick(true))
    print(if true -> wasm_pick(true) else -> 0)
}
"#;
    let dir = build_web_fixture("if_expr", src, "tests/fixtures/web_if_expr.jet");
    let js = fs::read_to_string(dir.join("build/app.js")).unwrap();
    let wasm = fs::read_to_string(dir.join("build/app_wasm.rs")).unwrap();
    assert!(
        js.contains("await (async () =>") && js.contains("let n = 6n;"),
        "JS IfExpr must retain branch statements and await bridge calls:\n{js}"
    );
    assert!(
        wasm.contains("if __jet_flag {")
            && (wasm.contains("let __jet_n = 6;") || wasm.contains("let mut __jet_n = 6;"))
            && wasm.contains("(__jet_n + 1)"),
        "Wasm IfExpr must retain branch statements and values:\n{wasm}"
    );
    let check = Command::new("node")
        .arg("--check")
        .arg(dir.join("build/app.js"))
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "node --check rejected JS:\n{}",
        String::from_utf8_lossy(&check.stderr)
    );
    assert_eq!(run_web_app(&dir), "8\n7\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn web_backends_traverse_impure_regions() {
    let src = r#"#Target(Web)
#Target(JS)
fn js_value() => Int {
    value := 0
    #Impure("preserve JS body") {
        value = 7
    }
    return value
}
#WasmExport
fn wasm_value() => Int {
    value := 0
    #Impure("preserve Wasm body") {
        value = 9
    }
    return value
}
fn run() { print(js_value()) }
"#;
    let out = jet::compile_web_with_path(src, "tests/fixtures/web_impure_regions.jet")
        .expect("Impure regions should be transparent to web codegen");
    let web = out.web.expect("web artifacts");
    assert!(
        web.js_app.contains("value = 7"),
        "JS body was dropped:\n{}",
        web.js_app
    );
    assert!(
        web.wasm_rust.contains("value = 9"),
        "Wasm body was dropped:\n{}",
        web.wasm_rust
    );
}

#[test]
fn web_inline_modules_keep_qualified_function_identity() {
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping web module identity test");
        return;
    }
    let src = r#"#Target(Web)
module left {
    #Target(JS)
    pub fn value() => Int { return 1 }
}
module right {
    #Target(JS)
    pub fn value() => Int { return 2 }
}
#Target(JS)
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
    let src = r#"#Target(Web)
module left { pub fn value() => Int { return 1 } }
module right { pub fn value() => Int { return 2 } }
#WasmExport
fn total() => Int { return left.value() + right.value() }
#Target(JS)
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
                "#Target(Web)\nuse \"./left\" as left\nuse \"./right\" as right\n#Target(JS)\nfn run() { print(left.value() + right.value()) }\n",
            ),
            (
                "left.jet",
                "#Target(JS)\npub fn value() => Int { return 1 }\n",
            ),
            (
                "right.jet",
                "#Target(JS)\npub fn value() => Int { return 2 }\n",
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
                "#Target(Web)\nuse \"./left\" as left\nuse \"./right\" as right\n#WasmExport\nfn total() => Int { return left.value() + right.value() }\n#Target(JS)\nfn run() { print(total()) }\n",
            ),
            ("left.jet", "pub fn value() => Int { return 1 }\n"),
            ("right.jet", "pub fn value() => Int { return 2 }\n"),
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
                "#Target(Web)\nuse \"./math\" as math\n#Target(JS)\nfn run() { print(math.value()) }\n",
            ),
            (
                "math.jet",
                "#WasmExport\npub fn value() => Int { return 7 }\n",
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
            "#Target(Web)\n{imports}\n#Target(JS)\nfn run() {{ print(left.value() + right.value()) }}\n"
        );
        let dir = build_web_project(
            stem,
            &[
                ("main.jet", &main),
                (
                    "left.jet",
                    "#Target(JS)\nfn helper() => Int { return 1 }\n#Target(JS)\npub fn value() => Int { return helper() }\n",
                ),
                (
                    "right.jet",
                    "fn helper() => Int { return 2 }\n#WasmExport\npub fn value() => Int { return helper() }\n",
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
    let src = r#"#Target(Web)
module helper { pub fn run() => Int { return 7 } }
#Target(JS)
fn run() { print("top-level") }
"#;
    let dir = build_web_fixture("entry_identity", src, "tests/fixtures/web_entry_identity.jet");
    let manifest = fs::read_to_string(dir.join("build/web.manifest.json")).unwrap();
    assert!(manifest.contains("\"entry\": \"JS\""), "module-local run hijacked manifest entry:\n{manifest}");
    let js = fs::read_to_string(dir.join("build/app.js")).unwrap();
    assert!(!js.contains("wasm.jet_export_run()"), "module-local run hijacked JS startup:\n{js}");
    assert_eq!(run_web_app(&dir), "top-level\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn web_missing_return_is_a_preflight_diagnostic() {
    let src = "#Target(Web)\n#Target(JS)\nfn missing() => Int { n :: 1 }\nfn run() {}\n";
    let diags = jet::compile_web_with_path(src, "tests/fixtures/web_missing_return.jet")
        .expect_err("non-void JS function without return must be rejected");
    assert!(diags.iter().any(|d| d.code == "E0114"), "{diags:?}");
}

#[test]
fn wasm_unsupported_export_abi_is_a_preflight_diagnostic() {
    let src =
        "#Target(Web)\n#WasmExport\nfn echo(xs: [Float]) => [Float] { return ~xs }\nfn run() {}\n";
    let diags = jet::compile_web_with_path(src, "tests/fixtures/web_bad_wasm_abi.jet")
        .expect_err("unsupported Wasm ABI must be rejected before emission");
    assert!(
        diags.iter().any(|d| d.code == "E-WEB-TIR-UNSUPPORTED"),
        "{diags:?}"
    );
}

#[test]
fn wasm_unsupported_internal_abi_is_a_preflight_diagnostic() {
    let src = "#Target(Web)\nfn helper(xs: [Float]) => [Float] { return ~xs }\nfn run() {}\n";
    let diags = jet::compile_web_with_path(src, "tests/fixtures/web_bad_internal_wasm_abi.jet")
        .expect_err("unsupported internal Wasm ABI must be rejected before emission");
    assert!(
        diags.iter().any(|d| d.code == "E-WEB-TIR-UNSUPPORTED"),
        "{diags:?}"
    );
}

#[test]
fn wasm_cross_bucket_call_is_a_normal_preflight_diagnostic() {
    let src = "#Target(Web)\n#Target(JS)\nfn browser_value() => Int { return 1 }\n#WasmExport\nfn compute() => Int { return browser_value() }\nfn run() {}\n";
    let diags = jet::compile_web_with_path(src, "tests/fixtures/web_cross_bucket_call.jet")
        .expect_err("Wasm must not call a JS-bucket function directly");
    assert!(diags.iter().any(|d| d.code == "E-WEB-CROSS-PARTITION"), "{diags:?}");
}

#[test]
fn canvas_style_wasm_tir_control_flow_and_print_compile() {
    let src = r#"#Target(Web)
fn square(n: Int) => Int { return n * n }
fn summarize(limit: Int) => Int {
    total := square(limit)
    if total > 10 { return total } else { return total + 1 }
}
fn scratch(limit: Int, text: String, flag: Bool, ratio: Float) { print(limit) }
fn run() { print(summarize(4)) }
"#;
    let out = jet::compile_web_with_path(src, "tests/fixtures/web_canvas_tir.jet")
        .expect("ordinary Canvas control flow and print must compile through TIR");
    let wasm = &out.web.expect("web artifacts").wasm_rust;
    assert!(wasm.contains("if (__jet_total > 10)"), "TIR if was not emitted:\n{wasm}");
    assert!(wasm.contains("println!(\"{}\""), "TIR print was not emitted:\n{wasm}");
    assert!(
        wasm.contains("__jet_text: &String") || wasm.contains("__jet_text: String"),
        "internal String parameter was rejected:\n{wasm}"
    );
}

#[test]
fn host_dev_entry_is_not_web_runtime_and_run_prints_literal_from_tir() {
    let src = r#"#Target(Web)
use core.web.devserver as devserver
#Target(JS)
fn dev() {
    server :: devserver.app()
    server.port(8080)
    server.serve()
}
module tools {
    fn dev() => Int { return 7 }
}
fn run() { print("hello, web") }
"#;
    let out = jet::compile_web_with_path(src, "tests/fixtures/web_host_dev.jet")
        .expect("host dev entry and web run body must compile through their own execution paths");
    let web = out.web.expect("web artifacts");
    let wasm = &web.wasm_rust;
    assert!(
        wasm.contains("fn jet_wasm_tools__dev() -> i64"),
        "module tools.dev was not emitted:\n{wasm}"
    );
    assert!(
        wasm.contains("println!(\"{}\", \"hello, web\".to_string())"),
        "literal TIR print was not emitted:\n{wasm}"
    );
    let js = &web.js_app;
    assert!(
        !js.contains("function dev("),
        "top-level host dev leaked into JS runtime:\n{js}"
    );
}

#[test]
fn default_wasm_top_level_dev_is_not_web_runtime() {
    let src = r#"#Target(Web)
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
    let src = r#"#Target(Web)
module tools {
    fn dev(name: String) {
        #Unsafe("exercise the web TIR gate") { print("hello {name}") }
    }
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
    // 196_ui_web_click.jet: every top-level `#Target(JS) fn` is exported (not just
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
fn web_typed_tree_a11y_focus_and_cleanup_roundtrip() {
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping typed web UI tree (need rustc + node)");
        return;
    }
    let src = r#"#Target(Web)
use core.ui as ui

#Target(JS)
fn render_tree(with_role: Bool) {
    tree := ui.node("plain", 80.0, 24.0)
    if with_role {
        tree = ui.node_role("Named", 80.0, 24.0, ui.aria_role_label())
    }
    backend := ui.null_backend()
    size := backend.measure(tree, ui.constraint(0.0, 0.0, 80.0, 24.0))
    backend.layout(tree, ui.rect(0.0, 0.0, size.width, size.height))
    backend.paint(tree)
}

#Target(JS)
fn render_focus_tree() {
    tree := ui.box([ui.button("Save"), ui.button("Cancel")])
    backend := ui.null_backend()
    size := backend.measure(tree, ui.constraint(0.0, 0.0, 80.0, 24.0))
    backend.layout(tree, ui.rect(0.0, 0.0, size.width, size.height))
    backend.paint(tree)
}

#Target(JS)
fn render_two_backends() {
    first := ui.null_backend()
    first_tree := ui.button("Backend A")
    first.layout(first_tree, ui.rect(0.0, 0.0, 80.0, 24.0))
    first.paint(first_tree)

    second := ui.null_backend()
    second_tree := ui.button("Backend B")
    second.layout(second_tree, ui.rect(0.0, 24.0, 80.0, 24.0))
    second.paint(second_tree)
}

fn run() {}
"#;
    let dir = build_web_fixture("typed_tree", src, "tests/fixtures/web_typed_tree.jet");
    let stdout = run_typed_ui_tree_harness(&dir);
    assert_eq!(
        stdout,
        "DIV:label:Named\nDIV:null:null\nfocus=true:Cancel\nscoped=true:Backend A\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn web_reactive_render_replaces_stale_dependencies() {
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping web reactive dependency test (need rustc + node)");
        return;
    }
    let src = r#"#Target(Web)
use core.reactive as reactive
use core.ui as ui

fn run() {
    choose_left := reactive.signal(true)
    left := reactive.signal(1)
    right := reactive.signal(10)
    ui.reactive_render(() => {
        if choose_left.get() { print(left.get()) } else { print(right.get()) }
    })
    choose_left.set(false)
    left.set(2)
    right.set(11)
}
"#;
    let dir = build_web_fixture("reactive_stale", src, "tests/fixtures/web_reactive_stale.jet");
    assert_eq!(run_web_app(&dir), "1\n10\n11\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn web_reactive_effect_lifecycle_roundtrip() {
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping web effect lifecycle test (need rustc + node)");
        return;
    }
    let src = r#"#Target(Web)
use core.reactive as reactive
#Target(JS)
fn run() {
    value := reactive.signal(1)
    subscription := reactive.effect(() => {
        print(value.get())
    })
    print(subscription.is_active())
    value.set(2)
    subscription.unsubscribe()
    print(subscription.is_active())
    subscription.unsubscribe()
    value.set(3)
}
"#;
    let dir = build_web_fixture(
        "reactive_effect_lifecycle",
        src,
        "tests/fixtures/web_reactive_effect_lifecycle.jet",
    );
    assert_eq!(run_web_app(&dir), "1\ntrue\n2\nfalse\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn web_events_and_storage_roundtrip() {
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping web_build browser API (need rustc + node)");
        return;
    }
    let src = r##"#Target(Web)
use core.web as web

#Target(JS)
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
    let js = fs::read_to_string(dir.join("build/app.js")).unwrap();
    let runtime = fs::read_to_string(dir.join("build/jet_dom_runtime.js")).unwrap();
    let manifest = fs::read_to_string(dir.join("build/web.manifest.json")).unwrap();
    let sources = jet::DevServer::BrowserTrace::sources_from_manifest(&manifest).unwrap();
    let source = sources.iter().find(|source| source.path == "tests/fixtures/web_api.jet").unwrap();
    assert_eq!(source.sha256, jet::SHA256::sha256_hex(src.as_bytes()));
    assert!(source.symbols.contains(&("init".into(), "fn".into())), "{source:?}");
    assert!(source.symbols.contains(&("init$handler0".into(), "handler".into())), "{source:?}");
    assert!(js.contains("jetDom.on(\"#new-task\", \"input\""), "{js}");
    assert!(js.contains("\"init$handler0\""), "{js}");
    assert!(!js.contains("__JET_INLINE_HANDLER__"), "{js}");
    assert!(runtime.contains("const handlerSymbol = String(symbol)"), "{runtime}");
    assert!(!runtime.contains("symbol || jetDomScopeName"), "{runtime}");
    assert!(runtime.contains("perfRecord(handlerSymbol, \"event\""), "{runtime}");
    let stdout = run_web_api_harness(&dir);
    assert_eq!(
        stdout, "tasks=[]\ndraft=write flagship slice\n",
        "web.on/web.value/web.storage.local should roundtrip through generated JS"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn web_trace_map_keeps_qualified_handler_identity() {
    let src = r##"#Target(Web)
use core.web as web
module handlers {
    #Target(JS)
    pub fn init() { web.on("#new-task", "input", (ev) => {}) }
}
#Target(JS)
fn init() { handlers.init() }
fn run() {}
"##;
    let dir = build_web_fixture("qualified_handler", src, "tests/fixtures/web_qualified_handler.jet");
    let js = fs::read_to_string(dir.join("build/app.js")).unwrap();
    let manifest = fs::read_to_string(dir.join("build/web.manifest.json")).unwrap();
    let sources = jet::DevServer::BrowserTrace::sources_from_manifest(&manifest).unwrap();
    assert!(js.contains("\"handlers__init$handler0\""), "{js}");
    assert!(sources.iter().any(|source| source.symbols.contains(&("handlers__init$handler0".into(), "handler".into()))), "{sources:?}");
    assert!(!sources.iter().any(|source| source.symbols.iter().any(|(name, _)| name == "init$handler0")), "qualified handler was attributed to an unqualified suffix: {sources:?}");
    let _ = fs::remove_dir_all(dir);
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
    let signature = "fn jet_export_sum_point(__jet_p_x: i64, __jet_p_y: i64) -> i64";
    let reconstruction = "let __jet_p = __jet_Point { __jet_x: __jet_p_x, __jet_y: __jet_p_y };";
    let field_read = "((__jet_p).__jet_x + (__jet_p).__jet_y)";
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
    for label in ["Altitude: 12,400 ft", "Airspeed: 410 kt", "Boosts: 0", "Fuel: 40%"] {
        assert!(
            lines.iter().any(|line| {
                line.contains(&format!("role=label aria={label:?}"))
            }),
            "styled card lost its accessible role/name for {label:?}:\n{stdout}"
        );
    }

    let html = include_str!("../examples/features/web/ui_showcase.html");
    assert!(
        html.contains(r#"<button id="boost-btn" type="button" aria-label="Boost fuel" data-motion-state="idle">"#),
        "showcase boost control must have stable button semantics"
    );
    assert!(
        !html.contains("btn.disabled"),
        "the click handler must not disable its own target while automation dispatches the click"
    );
    assert_eq!(
        html.matches("setTimeout(frame").count(),
        2,
        "motion timers must begin on click and stop at completion, not keep the page permanently busy"
    );
    assert!(
        html.contains(r#"btn.dataset.motionState = "complete""#),
        "the browser needs a bounded DOM-visible motion completion oracle"
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
    assert!(
        lines.contains(&"external-focus=true"),
        "Jet repaint stole focus from the companion HTML boost button:\n{stdout}"
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

#[test]
fn web_wasm_range_loop_bridge_roundtrip() {
    // D-WEBBACKEND1 / criterion #1: Wasm compute must lower inclusive
    // `loop i, start..end` from checked TIR (JS already could). Live
    // rustc+node proof — not emit-shape only.
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping web_build wasm range (need rustc + node)");
        return;
    }
    let src = include_str!("../examples/features/web/web_wasm_range.jet");
    let dir = build_web_fixture("wasm_range", src, "examples/features/web/web_wasm_range.jet");
    let wasm = fs::read_to_string(dir.join("build/app_wasm.rs")).unwrap();
    assert!(
        wasm.contains("for __jet_i in (0)..=(__jet_n)"),
        "inclusive range loop was not emitted:\n{wasm}"
    );
    assert!(
        wasm.contains("__jet_total = (__jet_total + __jet_i)"),
        "loop body assign was dropped:\n{wasm}"
    );
    let stdout = run_web_app(&dir);
    let expected = include_str!("../examples/features/expected/web/web_wasm_range.out");
    assert_eq!(stdout, expected);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn web_wasm_for_in_bridge_roundtrip() {
    // D-WEBBACKEND1 / criterion #1: Wasm compute must lower plain
    // `loop x, xs` ForIn from checked TIR (JS already could). Live
    // rustc+node proof — not emit-shape only. Reuses [Int] ABI; does not
    // reopen String/[Int] packing.
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping web_build wasm for-in (need rustc + node)");
        return;
    }
    let src = include_str!("../examples/features/web/web_wasm_for_in.jet");
    let dir = build_web_fixture("wasm_for_in", src, "examples/features/web/web_wasm_for_in.jet");
    let wasm = fs::read_to_string(dir.join("build/app_wasm.rs")).unwrap();
    assert!(
        wasm.contains(".iter().cloned()")
            && (wasm.contains("for __jet_x in") || wasm.contains("for x in")),
        "plain ForIn was not emitted:\n{wasm}"
    );
    assert!(
        wasm.contains("__jet_total = (__jet_total + __jet_x)")
            || wasm.contains("total = (total + x)"),
        "ForIn body assign was dropped:\n{wasm}"
    );
    let stdout = run_web_app(&dir);
    let expected = include_str!("../examples/features/expected/web/web_wasm_for_in.out");
    assert_eq!(stdout, expected);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn web_wasm_string_export_hostile_roundtrip() {
    // D-JSBIND1 / criterion #3: String export returns as packed (ptr,len) u64;
    // JS copies UTF-8 then frees. Hostile: empty, interior NUL, non-ASCII.
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping web_build wasm string (need rustc + node)");
        return;
    }
    let src = include_str!("../examples/features/web/web_wasm_string.jet");
    let dir = build_web_fixture("wasm_string", src, "examples/features/web/web_wasm_string.jet");
    let wasm = fs::read_to_string(dir.join("build/app_wasm.rs")).unwrap();
    assert!(
        wasm.contains("fn jet_abi_string_ret(s: String) -> u64"),
        "string return helper missing:\n{wasm}"
    );
    assert!(
        wasm.contains("pub extern \"C\" fn jet_abi_string_free(ptr: u32, len: u32)"),
        "string free export missing:\n{wasm}"
    );
    assert!(
        wasm.contains("-> u64 ")
            && wasm.contains("jet_abi_string_ret(jet_wasm_")
            && wasm.contains("pub extern \"C\" fn jet_export_"),
        "export must pack String as u64:\n{wasm}"
    );
    let js = fs::read_to_string(dir.join("build/app.js")).unwrap();
    assert!(
        js.contains("unmarshalAbi(raw, \"string\", wasm)"),
        "JS bridge must unmarshal String returns:\n{js}"
    );
    let runtime = fs::read_to_string(dir.join("build/jet_dom_runtime.js")).unwrap();
    assert!(
        runtime.contains("jet_abi_string_free") && runtime.contains("TextDecoder"),
        "runtime missing string ABI decode/free:\n{runtime}"
    );
    let stdout = run_web_app(&dir);
    let expected = include_str!("../examples/features/expected/web/web_wasm_string.out");
    assert_eq!(stdout, expected);
    // Explicit byte-level hostility: tab + newline survived the ABI.
    assert!(
        stdout.as_bytes().contains(&b'\t'),
        "TAB byte was lost in String roundtrip:\n{stdout:?}"
    );
    assert!(
        stdout.contains("emoji🌍"),
        "Unicode scalar was lost:\n{stdout}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn web_wasm_string_param_export_hostile_roundtrip() {
    // D-JSBIND1: String *params* into #WasmExport — JS TextEncoder + alloc,
    // packed u64, Wasm jet_abi_string_arg ownership. Hostile empty/tab/emoji.
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping web_build wasm string param (need rustc + node)");
        return;
    }
    let src = include_str!("../examples/features/web/web_wasm_string_param.jet");
    let dir = build_web_fixture(
        "wasm_string_param",
        src,
        "examples/features/web/web_wasm_string_param.jet",
    );
    let wasm = fs::read_to_string(dir.join("build/app_wasm.rs")).unwrap();
    assert!(
        wasm.contains("pub extern \"C\" fn jet_abi_string_alloc(len: u32) -> u32"),
        "string alloc export missing:\n{wasm}"
    );
    assert!(
        wasm.contains("fn jet_abi_string_arg(packed: u64) -> String"),
        "string arg helper missing:\n{wasm}"
    );
    assert!(
        wasm.contains("let __jet_s = jet_abi_string_arg(__jet_s)")
            || wasm.contains("let s = jet_abi_string_arg(s)"),
        "export wrapper must unpack String param:\n{wasm}"
    );
    assert!(
        wasm.contains("&__jet_s") || wasm.contains("&s"),
        "wrapper must pass borrowed String into jet_wasm_*:\n{wasm}"
    );
    let js = fs::read_to_string(dir.join("build/app.js")).unwrap();
    assert!(
        js.contains("marshalAbi(") && js.contains("\"string\""),
        "JS bridge must marshal String params:\n{js}"
    );
    let runtime = fs::read_to_string(dir.join("build/jet_dom_runtime.js")).unwrap();
    assert!(
        runtime.contains("jet_abi_string_alloc") && runtime.contains("TextEncoder"),
        "runtime missing string ABI encode/alloc:\n{runtime}"
    );
    let stdout = run_web_app(&dir);
    let expected = include_str!("../examples/features/expected/web/web_wasm_string_param.out");
    assert_eq!(stdout, expected);
    assert!(
        stdout.as_bytes().contains(&b'\t'),
        "TAB byte was lost in String param roundtrip:\n{stdout:?}"
    );
    assert!(
        stdout.contains("emoji🌍"),
        "Unicode scalar was lost:\n{stdout}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn web_wasm_list_int_export_hostile_roundtrip() {
    // D-JSBIND1 / criterion #3: [Int] export params+returns as packed (ptr,len)
    // u64 over little-endian i64 payload; JS BigInt64Array + free. Hostile:
    // empty, zero, signed, mixed.
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping web_build wasm list-int (need rustc + node)");
        return;
    }
    let src = include_str!("../examples/features/web/web_wasm_list.jet");
    let dir = build_web_fixture("wasm_list", src, "examples/features/web/web_wasm_list.jet");
    let wasm = fs::read_to_string(dir.join("build/app_wasm.rs")).unwrap();
    assert!(
        wasm.contains("fn jet_abi_list_i64_ret(v: Vec<i64>) -> u64"),
        "list-int return helper missing:\n{wasm}"
    );
    assert!(
        wasm.contains("fn jet_abi_list_i64_arg(packed: u64) -> Vec<i64>"),
        "list-int arg helper missing:\n{wasm}"
    );
    assert!(
        wasm.contains("pub extern \"C\" fn jet_abi_list_i64_alloc(len: u32) -> u32"),
        "list-int alloc export missing:\n{wasm}"
    );
    assert!(
        wasm.contains("pub extern \"C\" fn jet_abi_list_i64_free(ptr: u32, len: u32)"),
        "list-int free export missing:\n{wasm}"
    );
    assert!(
        wasm.contains("let __jet_xs = jet_abi_list_i64_arg(__jet_xs)")
            || wasm.contains("let xs = jet_abi_list_i64_arg(xs)"),
        "export wrapper must unpack [Int] param:\n{wasm}"
    );
    assert!(
        wasm.contains("jet_abi_list_i64_ret(jet_wasm_")
            && wasm.contains("-> u64 "),
        "export must pack [Int] return as u64:\n{wasm}"
    );
    assert!(
        wasm.contains("&__jet_xs") || wasm.contains("&xs"),
        "wrapper must pass borrowed Vec into jet_wasm_*:\n{wasm}"
    );
    let js = fs::read_to_string(dir.join("build/app.js")).unwrap();
    assert!(
        js.contains("marshalAbi(") && js.contains("\"list-int\""),
        "JS bridge must marshal [Int] params:\n{js}"
    );
    assert!(
        js.contains("unmarshalAbi(raw, \"list-int\", wasm)"),
        "JS bridge must unmarshal [Int] returns:\n{js}"
    );
    let runtime = fs::read_to_string(dir.join("build/jet_dom_runtime.js")).unwrap();
    assert!(
        runtime.contains("jet_abi_list_i64_alloc")
            && runtime.contains("jet_abi_list_i64_free")
            && runtime.contains("BigInt64Array"),
        "runtime missing list-int ABI encode/decode:\n{runtime}"
    );
    let stdout = run_web_app(&dir);
    let expected = include_str!("../examples/features/expected/web/web_wasm_list.out");
    assert_eq!(stdout, expected);
    assert!(
        stdout.contains("-1,2,-3"),
        "signed ints lost in [Int] roundtrip:\n{stdout:?}"
    );
    assert!(
        stdout.contains("42,0,-7,99"),
        "mixed ints lost in [Int] roundtrip:\n{stdout:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn web_wasm_list_string_export_hostile_roundtrip() {
    // D-JSBIND1 / criterion #3: [String] export params+returns as contiguous
    // LE [count][len][utf8]… packed u64; JS TextEncoder/Decoder + free.
    // Hostile: empty list, empty elem, tab/newline, emoji, multi.
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping web_build wasm list-string (need rustc + node)");
        return;
    }
    let src = include_str!("../examples/features/web/web_wasm_list_string.jet");
    let dir = build_web_fixture(
        "wasm_list_string",
        src,
        "examples/features/web/web_wasm_list_string.jet",
    );
    let wasm = fs::read_to_string(dir.join("build/app_wasm.rs")).unwrap();
    assert!(
        wasm.contains("fn jet_abi_list_string_ret(v: Vec<String>) -> u64"),
        "list-string return helper missing:\n{wasm}"
    );
    assert!(
        wasm.contains("fn jet_abi_list_string_arg(packed: u64) -> Vec<String>"),
        "list-string arg helper missing:\n{wasm}"
    );
    assert!(
        wasm.contains("pub extern \"C\" fn jet_abi_list_string_alloc(byte_len: u32) -> u32"),
        "list-string alloc export missing:\n{wasm}"
    );
    assert!(
        wasm.contains("pub extern \"C\" fn jet_abi_list_string_free(ptr: u32, byte_len: u32)"),
        "list-string free export missing:\n{wasm}"
    );
    assert!(
        wasm.contains("let __jet_xs = jet_abi_list_string_arg(__jet_xs)")
            || wasm.contains("let xs = jet_abi_list_string_arg(xs)"),
        "export wrapper must unpack [String] param:\n{wasm}"
    );
    assert!(
        wasm.contains("jet_abi_list_string_ret(jet_wasm_")
            && wasm.contains("-> u64 "),
        "export must pack [String] return as u64:\n{wasm}"
    );
    let js = fs::read_to_string(dir.join("build/app.js")).unwrap();
    assert!(
        js.contains("marshalAbi(") && js.contains("\"list-string\""),
        "JS bridge must marshal [String] params:\n{js}"
    );
    assert!(
        js.contains("unmarshalAbi(raw, \"list-string\", wasm)"),
        "JS bridge must unmarshal [String] returns:\n{js}"
    );
    let runtime = fs::read_to_string(dir.join("build/jet_dom_runtime.js")).unwrap();
    assert!(
        runtime.contains("jet_abi_list_string_alloc")
            && runtime.contains("jet_abi_list_string_free")
            && runtime.contains("list-string"),
        "runtime missing list-string ABI encode/decode:\n{runtime}"
    );
    let stdout = run_web_app(&dir);
    let expected = include_str!("../examples/features/expected/web/web_wasm_list_string.out");
    assert_eq!(stdout, expected);
    assert!(
        stdout.as_bytes().contains(&b'\t'),
        "TAB byte was lost in [String] roundtrip:\n{stdout:?}"
    );
    assert!(
        stdout.contains("emoji🌍"),
        "Unicode scalar was lost:\n{stdout}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn web_wasm_map_string_int_export_hostile_roundtrip() {
    // D-JSBIND1 / criterion #3: [String: Int] params+returns use a contiguous
    // LE [count][key-len][utf8][i64]... blob. Live JS -> Wasm -> JS -> Wasm
    // proof covers empty/control/Unicode keys, signed values, and ownership.
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping web_build wasm map-string-int (need rustc + node)");
        return;
    }
    let src = include_str!("../examples/features/web/web_wasm_map.jet");
    let dir = build_web_fixture("wasm_map", src, "examples/features/web/web_wasm_map.jet");
    let wasm = fs::read_to_string(dir.join("build/app_wasm.rs")).unwrap();
    assert!(
        wasm.contains("fn jet_abi_map_string_i64_ret(")
            && wasm.contains("fn jet_abi_map_string_i64_arg("),
        "map-string-int ABI helpers missing:\n{wasm}"
    );
    assert!(
        wasm.contains("pub extern \"C\" fn jet_abi_map_string_i64_alloc(byte_len: u32) -> u32")
            && wasm.contains(
                "pub extern \"C\" fn jet_abi_map_string_i64_free(ptr: u32, byte_len: u32)"
            ),
        "map-string-int ownership exports missing:\n{wasm}"
    );
    assert!(
        wasm.contains("jet_abi_map_string_i64_ret(jet_wasm_")
            && wasm.contains("jet_abi_map_string_i64_arg("),
        "export wrappers must pack/unpack [String: Int]:\n{wasm}"
    );
    let js = fs::read_to_string(dir.join("build/app.js")).unwrap();
    assert!(
        js.contains("marshalAbi(") && js.contains("\"map-string-int\""),
        "JS bridge must marshal [String: Int] params:\n{js}"
    );
    assert!(
        js.contains("unmarshalAbi(raw, \"map-string-int\", wasm)"),
        "JS bridge must unmarshal [String: Int] returns:\n{js}"
    );
    let runtime = fs::read_to_string(dir.join("build/jet_dom_runtime.js")).unwrap();
    assert!(
        runtime.contains("jet_abi_map_string_i64_alloc")
            && runtime.contains("jet_abi_map_string_i64_free")
            && runtime.contains("map-string-int"),
        "runtime missing map-string-int ABI encode/decode:\n{runtime}"
    );
    let stdout = run_web_app(&dir);
    let expected = include_str!("../examples/features/expected/web/web_wasm_map.out");
    assert_eq!(stdout, expected);

    let harness = r#"
import { marshalAbi, unmarshalAbi } from "./jet_dom_runtime.js";

function check(ok, message) {
  if (!ok) throw new Error(message);
}

let cursor = 16;
const frees = [];
const wasm = {
  memory: { buffer: new ArrayBuffer(4096) },
  jet_abi_map_string_i64_alloc(byteLen) {
    const ptr = cursor;
    cursor += byteLen;
    return ptr;
  },
  jet_abi_map_string_i64_free(ptr, byteLen) {
    frees.push([ptr, byteLen]);
  },
};
const limits = new Map([
  ["min", -9223372036854775808n],
  ["max", 9223372036854775807n],
  ["above-safe", 9007199254740993n],
  ["below-safe", -9007199254740993n],
  ["small", 7n],
]);
const packed = marshalAbi(limits, "map-string-int", wasm);
const decoded = unmarshalAbi(packed, "map-string-int", wasm);
for (const [key, value] of limits) {
  check(typeof decoded.get(key) === "bigint", `${key} was not BigInt`);
  check(decoded.get(key) === value, `${key} changed`);
}
const repacked = marshalAbi(decoded, "map-string-int", wasm);
const decodedTwice = unmarshalAbi(repacked, "map-string-int", wasm);
for (const [key, value] of limits) check(decodedTwice.get(key) === value, `${key} changed on re-marshal`);
try {
  marshalAbi(new Map([["number", 1]]), "map-string-int", wasm);
  throw new Error("Number-backed Int accepted");
} catch (error) {
  check(error instanceof TypeError, "Number-backed Int was not rejected");
}

let allocations = 0;
const bounded = {
  memory: { buffer: new ArrayBuffer(64) },
  jet_abi_map_string_i64_alloc() { allocations += 1; return 60; },
  jet_abi_map_string_i64_free(ptr, byteLen) { frees.push([ptr, byteLen]); },
};
class HiddenEntryMap extends Map {
  get size() { return 0; }
  *[Symbol.iterator]() { yield ["hidden", 1n]; }
}
try {
  marshalAbi(new HiddenEntryMap(), "map-string-int", bounded);
  throw new Error("size-zero iterator entry accepted");
} catch (error) {
  check(error instanceof TypeError && error.message.includes("size/iterator mismatch"), "size-zero iterator mismatch was not rejected");
}
check(allocations === 0, "size-zero iterator mismatch allocated");

class MissingEntryMap extends Map {
  get size() { return 2; }
  *[Symbol.iterator]() { yield ["visible", 1n]; }
}
try {
  marshalAbi(new MissingEntryMap(), "map-string-int", bounded);
  throw new Error("short iterator accepted");
} catch (error) {
  check(error instanceof TypeError && error.message.includes("size/iterator mismatch"), "short iterator mismatch was not rejected");
}
check(allocations === 0, "short iterator mismatch allocated");

class HugeMap extends Map { get size() { return 0x1_0000_0000; } }
try {
  marshalAbi(new HugeMap([["x", 1n]]), "map-string-int", bounded);
  throw new Error("count overflow accepted");
} catch (error) {
  check(error instanceof RangeError, "count overflow was not a RangeError");
}
check(allocations === 0, "count overflow allocated");

const RealTextEncoder = globalThis.TextEncoder;
globalThis.TextEncoder = class { encode() { return { length: 0x1_0000_0000 }; } };
try {
  marshalAbi(new Map([["x", 1n]]), "map-string-int", bounded);
  throw new Error("key length overflow accepted");
} catch (error) {
  check(error instanceof RangeError, "key length overflow was not a RangeError");
}
check(allocations === 0, "key length overflow allocated");

globalThis.TextEncoder = class { encode() { return { length: 0xfffffff4 }; } };
try {
  marshalAbi(new Map([["x", 1n]]), "map-string-int", bounded);
  throw new Error("blob length overflow accepted");
} catch (error) {
  check(error instanceof RangeError, "blob length overflow was not a RangeError");
}
check(allocations === 0, "blob length overflow allocated");
globalThis.TextEncoder = RealTextEncoder;

try {
  marshalAbi(new Map([["x", 1n]]), "map-string-int", bounded);
  throw new Error("bounded write unexpectedly succeeded");
} catch (error) {
  check(error instanceof RangeError, "bounded write did not fail at the memory boundary");
}
check(frees.some(([ptr]) => ptr === 60), "post-allocation write failure leaked");

const highBit = {
  memory: { buffer: new ArrayBuffer(64) },
  jet_abi_map_string_i64_alloc() { return -2147483648; },
  jet_abi_map_string_i64_free(ptr, byteLen) { frees.push([ptr, byteLen]); },
};
try {
  marshalAbi(new Map([["x", 1n]]), "map-string-int", highBit);
  throw new Error("high-bit pointer unexpectedly fit bounded memory");
} catch (error) {
  check(error instanceof RangeError, "high-bit pointer did not reach the memory boundary");
}
check(frees.some(([ptr, len]) => ptr === 0x80000000 && len > 0), "signed allocation pointer was not freed as u32");

try {
  unmarshalAbi((0x80000000n << 32n) | 4n, "map-string-int", highBit);
  throw new Error("high-bit return pointer unexpectedly fit bounded memory");
} catch (error) {
  check(error instanceof RangeError, "high-bit return pointer did not reach the memory boundary");
}
check(frees.some(([ptr, len]) => ptr === 0x80000000 && len === 4), "high-bit return pointer was not freed as u32");
console.log("ok");
"#;
    fs::write(dir.join("build/map_abi_harness.mjs"), harness).unwrap();
    let node = Command::new("node")
        .current_dir(dir.join("build"))
        .arg("map_abi_harness.mjs")
        .output()
        .unwrap();
    assert!(
        node.status.success(),
        "map ABI hostile harness failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&node.stdout),
        String::from_utf8_lossy(&node.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&node.stdout), "ok\n");
    let _ = fs::remove_dir_all(&dir);
}

const WASM_CALLBACK_HARNESS: &str = r##"
class FakeElement {
  constructor(id) {
    this.id = id;
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
    this.button = new FakeElement("go");
  }
  querySelector(sel) {
    return sel === "#go" ? this.button : null;
  }
  getElementById(id) {
    return id === "go" ? this.button : null;
  }
  createElement(tag) {
    return new FakeElement(tag);
  }
}
globalThis.document = new FakeDocument();

const { init } = await import("./app.js");
init();
document.button.dispatchEvent({ type: "click" });
"##;

fn run_node_harness(dir: &PathBuf, filename: &str, harness: &str) -> String {
    let harness_path = dir.join("build").join(filename);
    fs::write(&harness_path, harness).unwrap();
    let node = Command::new("node")
        .current_dir(dir.join("build"))
        .arg(filename)
        .output()
        .unwrap();
    assert!(
        node.status.success(),
        "node harness {filename} failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&node.stdout),
        String::from_utf8_lossy(&node.stderr)
    );
    String::from_utf8_lossy(&node.stdout).into_owned()
}

#[test]
fn web_wasm_event_callback_bridge_roundtrip() {
    // D-JSBIND1 / criterion #3: a JS event callback must call through the
    // generated Wasm bridge and preserve Int ABI semantics.
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping web_build wasm callback (need rustc + node)");
        return;
    }
    let src = include_str!("../examples/features/web/web_wasm_callback.jet");
    let dir = build_web_fixture("wasm_callback", src, "examples/features/web/web_wasm_callback.jet");
    let js = fs::read_to_string(dir.join("build/app.js")).unwrap();
    assert!(
        js.contains("await bridge_double(21)"),
        "click handler must call the Wasm bridge:\n{js}"
    );
    let stdout = run_node_harness(&dir, "wasm_callback_harness.mjs", WASM_CALLBACK_HARNESS);
    let expected = include_str!("../examples/features/expected/web/web_wasm_callback.out");
    assert_eq!(stdout, expected);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn web_abi_string_and_list_marshal_errors_are_hostile() {
    // D-JSBIND1 / criterion #3: runtime ABI errors must reject hostile inputs
    // and free allocations taken before a write failure (string + list-int).
    if !have_tool("node") {
        eprintln!("note: skipping ABI error harness (need node)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_abi_errors_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("build")).unwrap();
    fs::write(
        dir.join("build/jet_dom_runtime.js"),
        include_str!("../crates/jet-codegen/src/Prelude/DomRuntime.js"),
    )
    .unwrap();
    let harness = r#"
import { marshalAbi, unmarshalAbi } from "./jet_dom_runtime.js";

function check(ok, message) {
  if (!ok) throw new Error(message);
}

const frees = [];
let cursor = 16;
const wasm = {
  memory: { buffer: new ArrayBuffer(4096) },
  jet_abi_string_alloc(len) {
    const ptr = cursor;
    cursor += len;
    return ptr;
  },
  jet_abi_string_free(ptr, len) {
    frees.push(["string", ptr, len]);
  },
  jet_abi_list_i64_alloc(len) {
    const ptr = cursor;
    cursor += len * 8;
    return ptr;
  },
  jet_abi_list_i64_free(ptr, len) {
    frees.push(["list-int", ptr, len]);
  },
};

const bounded = {
  memory: { buffer: new ArrayBuffer(64) },
  jet_abi_string_alloc() { return 60; },
  jet_abi_string_free(ptr, len) { frees.push(["string", ptr, len]); },
  jet_abi_list_i64_alloc() { return 48; },
  jet_abi_list_i64_free(ptr, len) { frees.push(["list-int", ptr, len]); },
};

const RealTextEncoder = globalThis.TextEncoder;
globalThis.TextEncoder = class {
  encode() {
    return { length: 0xfffffff4 };
  }
};
try {
  marshalAbi("x", "string", bounded);
  throw new Error("string length overflow accepted");
} catch (error) {
  check(error instanceof TypeError, "string length overflow was not a TypeError");
}
check(!frees.some((entry) => entry[0] === "string"), "string length overflow allocated");
globalThis.TextEncoder = RealTextEncoder;

try {
  marshalAbi("12345", "string", bounded);
  throw new Error("bounded string write unexpectedly succeeded");
} catch (error) {
  check(error instanceof RangeError, "bounded string write did not fail at memory boundary");
}
check(frees.some((entry) => entry[0] === "string" && entry[1] === 60), "string write failure leaked");

const highBit = {
  memory: { buffer: new ArrayBuffer(64) },
  jet_abi_string_alloc() { return -2147483648; },
  jet_abi_string_free(ptr, len) { frees.push(["string", ptr, len]); },
  jet_abi_list_i64_alloc() { return -2147483648; },
  jet_abi_list_i64_free(ptr, len) { frees.push(["list-int", ptr, len]); },
};
try {
  marshalAbi("x", "string", highBit);
  throw new Error("high-bit string pointer unexpectedly fit bounded memory");
} catch (error) {
  check(error instanceof RangeError, "high-bit string pointer did not reach the memory boundary");
}
check(
  frees.some((entry) => entry[0] === "string" && entry[1] === 0x80000000),
  "signed string allocation pointer was not freed as u32",
);

try {
  unmarshalAbi((0x80000000n << 32n) | 4n, "string", highBit);
  throw new Error("high-bit string return pointer unexpectedly fit bounded memory");
} catch (error) {
  check(error instanceof RangeError, "high-bit string return pointer did not reach the memory boundary");
}
check(
  frees.some((entry) => entry[0] === "string" && entry[1] === 0x80000000 && entry[2] === 4),
  "high-bit string return pointer was not freed as u32",
);

try {
  unmarshalAbi((0x80000000n << 32n) | 2n, "list-int", highBit);
  throw new Error("high-bit list return pointer unexpectedly fit bounded memory");
} catch (error) {
  check(error instanceof RangeError, "high-bit list return pointer did not reach the memory boundary");
}
check(
  frees.some((entry) => entry[0] === "list-int" && entry[1] === 0x80000000 && entry[2] === 2),
  "high-bit list return pointer was not freed as u32",
);

console.log("ok");
"#;
    assert_eq!(
        run_node_harness(&dir, "abi_error_harness.mjs", harness),
        "ok\n"
    );
    let _ = fs::remove_dir_all(&dir);
}
