//! Card #2239: `jet new <name> --template web` to a served browser page.
#![allow(non_snake_case)]

mod common;

use common::Scratch;
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

const WEB_DOM_HARNESS: &str = r#"
class FakeElement {
  constructor(tag) {
    this.tagName = tag;
    this.style = {};
    this.dataset = {};
    this.children = [];
    this.textContent = "";
    this.id = "";
    this.attributes = new Map();
    this.listeners = new Map();
    this.parentNode = null;
  }
  appendChild(child) {
    child.parentNode = this;
    this.children.push(child);
    return child;
  }
  remove() {
    if (!this.parentNode) return;
    this.parentNode.children = this.parentNode.children.filter((child) => child !== this);
  }
  addEventListener(name, handler) {
    const handlers = this.listeners.get(name) ?? [];
    handlers.push(handler);
    this.listeners.set(name, handlers);
  }
  setAttribute(name, value) { this.attributes.set(name, String(value)); }
  removeAttribute(name) { this.attributes.delete(name); }
  focus() {}
}
class FakeDocument {
  constructor() {
    this.body = new FakeElement("body");
    this.body.ownerDocument = this;
    this.activeElement = null;
    this._byId = new Map();
  }
  createElement(tag) {
    const element = new FakeElement(tag);
    element.ownerDocument = this;
    return element;
  }
  getElementById(id) { return this._byId.get(id) ?? null; }
}
const document = new FakeDocument();
const appendBody = document.body.appendChild.bind(document.body);
document.body.appendChild = (element) => {
  if (element.id) document._byId.set(element.id, element);
  return appendBody(element);
};
globalThis.document = document;

const { jet_main } = await import("./app.js");
await jet_main();
const root = document.getElementById("jet-app");
const text = root?.children.map((child) => child.textContent).join("") ?? "";
if (!text.includes("hello, world")) {
  throw new Error(`expected rendered greeting, got ${JSON.stringify(text)}`);
}
console.log(text);
"#;

fn assert_web_dom_output(project: &Path) {
    let build = project.join(".jet/build");
    let harness = build.join("web_dom_harness.mjs");
    fs::write(&harness, WEB_DOM_HARNESS).expect("write Web DOM harness");
    let output = Command::new("node")
        .current_dir(&build)
        .arg("web_dom_harness.mjs")
        .output()
        .expect("spawn Web DOM harness");
    let _ = fs::remove_file(harness);
    assert!(
        output.status.success(),
        "generated Web app did not render the greeting:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_no_scaffold_diagnostic("Web DOM harness", &output.stdout, &output.stderr);
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("hello, world"),
        "generated Web DOM omitted the greeting:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

fn assert_no_scaffold_diagnostic(label: &str, stdout: &[u8], stderr: &[u8]) {
    let output = format!(
        "{}\n{}",
        String::from_utf8_lossy(stdout),
        String::from_utf8_lossy(stderr)
    );
    for code in ["E-WEB-TIR-UNSUPPORTED", "E2937", "ICE"] {
        assert!(
            !output.contains(code),
            "{label} emitted scaffold drift diagnostic {code}:\n{output}"
        );
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
        .args(["new", "web_app", "--template", "web"])
        .output()
        .expect("spawn jet new web scaffold");
    assert!(
        created.status.success(),
        "jet new --template web failed:\nstdout: {}\nstderr: {}",
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
    let manifest_facts = jet::Package::PackageFacts::parse(&manifest, "package.jet")
        .expect("web scaffold manifest must remain valid");
    let allowed_effects = manifest_facts
        .authority
        .holds
        .allow
        .as_ref()
        .expect("web scaffold authority must declare allowed effects");
    assert!(
        allowed_effects.iter().any(|effect| effect == "Browser"),
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
    assert_no_scaffold_diagnostic("web scaffold build", &build.stdout, &build.stderr);
    assert!(project.join(".jet/build/index.html").is_file());

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
    assert_no_scaffold_diagnostic("web scaffold test", &test.stdout, &test.stderr);

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
    assert!(
        page.contains("type=\"module\"") && page.contains("src=\"./app.js\""),
        "generated scaffold page must load its Web module: {page}"
    );
    assert!(
        !page.contains("jet_main"),
        "generated scaffold page must not know an export name: {page}"
    );
}

#[test]
fn jet_new_native_scaffold_builds_for_explicit_web_target() {
    if !have_tool("rustc") || !have_tool("node") {
        eprintln!("note: skipping native scaffold Web DOM proof (need rustc + node)");
        return;
    }

    let scratch = Scratch::new("web-target-native-scaffold");
    let project_root = scratch.path.clone();
    let created = Command::new(jet_bin())
        .current_dir(&project_root)
        .args(["new", "demo", "--template", "ui"])
        .output()
        .expect("spawn jet new native scaffold");
    assert!(
        created.status.success(),
        "jet new failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&created.stdout),
        String::from_utf8_lossy(&created.stderr)
    );

    let project = project_root.join("demo");
    let manifest = fs::read_to_string(project.join("package.jet")).expect("native scaffold manifest");
    let manifest_facts = jet::Package::PackageFacts::parse(&manifest, "package.jet")
        .expect("native scaffold manifest must remain valid");
    let allowed_effects = manifest_facts
        .authority
        .holds
        .allow
        .as_ref()
        .expect("native scaffold authority must declare allowed effects");
    // A new project holds only the `Exec.Args` leaf, never the `Exec` root.
    for effect in [jet::Syntax::EFFECT_LEAF_EXEC_ARGS, "Browser"] {
        assert!(
            allowed_effects.iter().any(|declared| declared == effect),
            "native scaffold authority must grant {effect}:\n{manifest}"
        );
    }
    let native = Command::new(jet_bin())
        .current_dir(&project)
        .arg("run")
        .output()
        .expect("spawn native run for scaffold");
    assert!(
        native.status.success(),
        "native scaffold run failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&native.stdout),
        String::from_utf8_lossy(&native.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&native.stdout),
        "hello, world\n",
        "native scaffold output changed"
    );
    assert_no_scaffold_diagnostic("native scaffold run", &native.stdout, &native.stderr);

    let build = Command::new(jet_bin())
        .current_dir(&project)
        .args(["build", "--target", "web"])
        .output()
        .expect("spawn explicit Web build for native scaffold");
    assert!(
        build.status.success(),
        "native scaffold Web build failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr)
    );
    assert_no_scaffold_diagnostic("native scaffold Web build", &build.stdout, &build.stderr);
    for artifact in [".jet/build/index.html", ".jet/build/app.js", ".jet/build/app.wasm"] {
        assert!(
            project.join(artifact).is_file(),
            "missing Web scaffold artifact {artifact}"
        );
    }

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
            .expect("spawn jet dev for native scaffold"),
    );
    wait_for_server(port, Duration::from_secs(30));
    let (status, _) = http_get(port, "/").expect("GET native scaffold page");
    assert_eq!(status, 200, "native scaffold page was not served");

    assert_web_dom_output(&project);
}
