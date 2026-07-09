//! c134 Phase 7: `jet dev <file>.jet --target=web` — the std-only watch +
//! rebuild-on-save + live-reload dev server (Source/CmdDevWeb.rs). Starts the
//! real `jet` binary as a child process, talks to it over a plain TCP socket
//! (no new dependency — matches the server's own std-only HTTP), edits a
//! throwaway temp copy of a fixture, and confirms the rebuild lands.
//!
//! Also covers c-devserver (owner-directed 2026-07-01): `jet dev <file>`
//! when the file defines a top-level `fn dev()` — the file configures and
//! starts its OWN `core.devserver` value instead of the CLI hardcoding
//! `--target=web --port=<N>`. Same spawn/TCP/rebuild harness, driven through
//! zero CLI flags — `jet dev app.jet`, no `--target=web`.

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
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

fn unused_local_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .expect("bind an ephemeral localhost port")
        .local_addr()
        .expect("read local addr")
        .port()
}

/// Minimal blocking HTTP/1.1 GET over a raw `TcpStream` — the same shape of
/// client `jet dev --target=web`'s own std-only server (Source/CmdDevWeb.rs)
/// expects, so the test doesn't need `curl` or any HTTP crate.
fn http_get(port: u16, path: &str) -> Option<(u16, Vec<u8>)> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
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

fn http_post(port: u16, path: &str, body: &str) -> Option<(u16, Vec<u8>)> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let req = format!(
        "POST {} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        path,
        body.len(),
        body
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

fn json_field(haystack: &str, field: &str) -> String {
    let key = format!("\"{field}\":\"");
    let start = haystack.find(&key).expect("json field") + key.len();
    let rest = &haystack[start..];
    rest[..rest.find('"').expect("json field terminator")].to_string()
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

// ============================================================================
// Section: `--target=web` entry mode (was tests/web_dev.rs)
// ============================================================================

const FIXTURE_SRC: &str = r#"use core.ui as ui

fn run() {
    backend :: ui.null_backend()
    node :: ui.node("hello", 100.0, 20.0)
    constraint :: ui.constraint(0.0, 0.0, 200.0, 100.0)
    size :: backend.measure(node, constraint)
    print(size.width)
    print(size.height)
}
"#;

/// Same source with one literal changed, so the compiled `app.js` differs
/// byte-for-byte from `FIXTURE_SRC`'s — proof a rebuild actually re-ran the
/// compiler rather than re-serving a cached response.
const FIXTURE_SRC_EDITED: &str = r#"use core.ui as ui

fn run() {
    backend :: ui.null_backend()
    node :: ui.node("hello there", 100.0, 20.0)
    constraint :: ui.constraint(0.0, 0.0, 200.0, 100.0)
    size :: backend.measure(node, constraint)
    print(size.width)
    print(size.height)
}
"#;

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
    let (status, status_body) =
        http_get(port, "/__jet_dev_status").expect("GET /__jet_dev_status failed");
    assert_eq!(status, 200);
    let status_text = String::from_utf8_lossy(&status_body);
    assert!(status_text.contains("\"state\":\"ready\""), "{status_text}");
    assert!(status_text.contains("\"clients\":"), "{status_text}");

    // index.html should carry the injected live-reload poller.
    let (status, html_body) = http_get(port, "/").expect("GET / failed");
    assert_eq!(status, 200);
    let html = String::from_utf8_lossy(&html_body);
    assert!(
        html.contains("__jet_dev_status") && html.contains("Build failed"),
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

#[test]
fn jet_dev_web_exposes_canvas_panel_and_graph() {
    if !have_tool("rustc") {
        eprintln!("note: skipping jet_dev_web_exposes_canvas_panel_and_graph (need rustc)");
        return;
    }

    let dir = std::env::temp_dir().join(format!("jet_dev_canvas_{}", std::process::id()));
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

    let (status, html) = http_get(port, "/canvas").expect("GET Canvas panel");
    assert_eq!(status, 200);
    let html = String::from_utf8_lossy(&html);
    assert!(html.contains("<canvas id=\"jet-canvas-view\""));
    assert!(html.contains("<canvas id=\"minimap\""));
    assert!(html.contains("id=\"graph-list\""));
    assert!(html.contains("id=\"project-panel\""));
    assert!(html.contains("id=\"project-rail\""));
    assert!(html.contains("id=\"project-mode\""));
    assert!(html.contains("id=\"variables-panel\""));
    assert!(html.contains("id=\"variables-list\""));
    assert!(html.contains("id=\"variable-count\""));
    assert!(html.contains("id=\"status-panel\""));
    assert!(html.contains("id=\"status-summary\""));
    assert!(html.contains("id=\"status-count\""));
    assert!(html.contains("id=\"package-summary\""));
    assert!(html.contains("id=\"dependency-summary\""));
    assert!(html.contains("id=\"dev-summary\""));
    assert!(html.contains("id=\"diagnostics-summary\""));
    assert!(html.contains("id=\"trust-summary\""));
    assert!(html.contains("id=\"canvas-dock\""));
    assert!(html.contains("id=\"graph-strip\""));
    assert!(html.contains("id=\"wire-status\""));
    assert!(html.contains("id=\"graph-overview\""));
    assert!(html.contains("id=\"left-drawer\""));
    assert!(html.contains("id=\"right-drawer\""));
    assert!(html.contains("id=\"dock-graphs\""));
    assert!(html.contains("id=\"dock-details\""));
    assert!(html.contains("id=\"details\""));
    assert!(html.contains("id=\"undo-edit\""));
    assert!(html.contains("id=\"redo-edit\""));
    assert!(html.contains("id=\"org-align\""));
    assert!(html.contains("id=\"org-tidy\""));
    assert!(html.contains("id=\"bookmark-add\""));
    assert!(html.contains("id=\"bookmark-jump\""));
    assert!(html.contains("id=\"core-catalog\""));
    assert!(html.contains("id=\"favorite-action\""));
    assert!(html.contains("id=\"run-current\""));
    assert!(html.contains("id=\"run-hud\""));
    assert!(html.contains("id=\"first-run-tour\""));
    assert!(html.contains("id=\"tour-dismiss\""));
    assert!(html.contains("id=\"graph-back\""));
    assert!(html.contains("id=\"graph-forward\""));
    assert!(html.contains("id=\"source-diff\""));
    assert!(html.contains("id=\"edit-source\""));
    assert!(html.contains("id=\"apply-source-edit\""));
    assert!(html.contains("id=\"cancel-source-edit\""));
    assert!(html.contains("id=\"view-toggle\""));
    assert!(html.contains("id=\"lens-switch\""));
    assert!(html.contains("id=\"view-code\""));
    assert!(html.contains("id=\"view-split\""));
    assert!(html.contains("id=\"view-graph\""));
    assert!(html.contains("id=\"detail-toggles\""));
    assert!(html.contains("data-detail-toggle=\"types\""));
    assert!(html.contains("data-detail-toggle=\"diagnostics\""));
    assert!(html.contains("data-detail-toggle=\"effects\""));
    assert!(html.contains("data-detail-toggle=\"debug\""));
    assert!(html.contains("data-detail-toggle=\"package\""));
    assert!(html.contains("id=\"developer-mode\""));
    assert!(html.contains("id=\"toolbar-search\""));
    assert!(html.contains("id=\"toolbar-zoom\""));
    assert!(html.contains("class=\"toolbar-group\""));
    assert!(html.contains("class=\"toolbar-menu\""));
    assert!(html.contains("class=\"icon-button\""));
    assert!(html.contains("<svg viewBox=\"0 0 24 24\""));
    assert!(html.contains("id=\"source-view\""));
    assert!(html.contains("id=\"source-editor\""));
    assert!(html.contains("id=\"context-menu\""));
    assert!(html.contains("grid-template-rows: auto minmax(0, 1fr) 28px"));
    assert!(html.contains("flex-wrap: nowrap"));
    assert!(html
        .contains("grid-template-columns: minmax(156px, 15vw) minmax(0, 1fr) minmax(238px, 20vw)"));
    assert!(html.contains("@media (max-width: 900px)"));
    assert!(html.contains(".side.is-drawer-open"));
    assert!(html.contains("grid-template-columns: minmax(0, 1fr)"));
    assert!(html.contains("canvas_ui=blueprint23"));
    assert!(html.contains("rel=\"icon\" href=\"data:image/svg+xml"));
    assert!(html.contains("body:not(.is-dev-mode) #graph-strip { display: none; }"));
    assert!(html.contains("body:not(.is-dev-mode) #wire-status { display: none; }"));
    assert!(html.contains("body:not(.is-dev-mode) .dev-only"));
    assert!(html.contains("body.is-debug-active .debug-controls { display: flex; }"));
    assert!(html.contains("body:not(.is-dev-mode) #jump { display: none; }"));
    assert!(html.contains("@media (prefers-reduced-motion: reduce)"));
    assert!(html.contains("#run-hud.is-running"));
    assert!(html.contains("#first-run-tour.is-open"));
    assert!(html.contains("id=\"graph-count\""));
    assert!(html.contains("<summary><span>Functions</span>"));
    assert!(html.contains("<h2>My Canvas</h2>"));
    assert!(html.contains("<summary><span>Files</span>"));
    assert!(html.contains(".project-section"));
    assert!(html.contains(".variable-item"));
    assert!(html.contains(".status-card"));
    assert!(html.contains(".lens-switch"));
    assert!(html.contains(".detail-toggles"));
    assert!(html.contains(".type-detail"));
    assert!(html.contains(".pin-port.is-exec"));
    assert!(html.contains(".pin-port.is-fallible"));
    assert!(html.contains("#stage.is-code"));
    assert!(html.contains("#stage.is-split"));
    assert!(html.contains("#graph-overview { display: none; }"));
    assert!(html.contains(".graph-overview-title"));
    assert!(html.contains(".graph-stat"));
    assert!(html.contains(".graph-tab-title"));
    assert!(html.contains(".graph-tab-count"));
    assert!(html.contains(".details-hero"));
    assert!(html.contains(".pin-card"));
    assert!(html.contains("id=\"scm-state\""));
    assert!(html.contains("id=\"canvas-search\""));
    assert!(html.contains("id=\"search-results\""));
    assert!(!html.contains("id=\"palette-list\""));
    assert!(!html.contains("id=\"node-ribbon\""));
    assert!(!html.contains("id=\"node-action-strip\""));
    assert!(!html.contains("id=\"dock-palette\""));
    assert!(!html.contains("id=\"palette-panel\""));
    assert!(html.contains("id=\"proof-panel\""));
    assert!(html.contains("id=\"proof-rail\""));
    assert!(html.contains("id=\"proof-state\""));
    assert!(html.contains("/canvas/app.js"));
    assert!(!html.contains("source-truth"));
    assert!(!html.contains("Source truth"));
    assert!(!html.contains(">Trust<"));

    let (status, js) = http_get(port, "/canvas/app.js").expect("GET Canvas JS");
    assert_eq!(status, 200);
    let js = String::from_utf8_lossy(&js);
    assert!(js.contains("window.__jetCanvasNonblankPixels"));
    assert!(js.contains("window.__jetCanvasHitMap"));
    assert!(js.contains("ctx.fillRect"));
    assert!(js.contains("const graphUrl"));
    assert!(js.contains("fetch(graphRequestUrl"));
    assert!(js.contains("latestProject"));
    assert!(js.contains("function loadProject"));
    assert!(js.contains("function syncProjectRail"));
    assert!(js.contains("function syncVariablesList"));
    assert!(js.contains("function renderVariableDetails"));
    assert!(js.contains("function signatureWithVariable"));
    assert!(js.contains("__jetCanvasProjectRail"));
    assert!(js.contains("__jetCanvasVariablesSidebar"));
    assert!(js.contains("__jetCanvasWorkspacePanels"));
    assert!(js.contains("data-project-file"));
    assert!(js.contains("function graphRequestUrl"));
    assert!(js.contains("source_id="));
    assert!(js.contains("function fitGraph"));
    assert!(js.contains("Math.min(1.05"));
    assert!(js.contains("const layoutScale = { x: 1.08, y: 1.08 }"));
    assert!(js.contains("recordHit = true"));
    assert!(js.contains("drawNode(graph, node, inlineByNode, false)"));
    assert!(js.contains("function postTransaction"));
    assert!(js.contains("const queryUrl"));
    assert!(js.contains("sourceControlUrl"));
    assert!(js.contains("loadSourceControl"));
    assert!(js.contains("showSourceDiff"));
    assert!(js.contains("proofUrl"));
    assert!(js.contains("loadProofRail"));
    assert!(js.contains("__JET_CANVAS_PROOF__"));
    assert!(js.contains("__jetCanvasProofRail"));
    assert!(js.contains("function postQuery"));
    assert!(js.contains("loadCanvasActions"));
    assert!(js.contains("openCoreCatalogPalette"));
    assert!(js.contains("__JET_CANVAS_CORE_CATALOG__"));
    assert!(js.contains("preview_canvas_action"));
    assert!(js.contains("jet.canvas.action"));
    assert!(js.contains("checked-tir+jit"));
    assert!(js.contains("source_to_graph"));
    assert!(js.contains("function setSourceHash"));
    assert!(js.contains("window.history.replaceState"));
    assert!(js.contains("preview_rename"));
    assert!(js.contains("find-references"));
    assert!(js.contains("rename_function"));
    assert!(js.contains("edit_function_signature"));
    assert!(js.contains("create_function"));
    assert!(js.contains("Apply signature"));
    assert!(js.contains("signature-board"));
    assert!(js.contains("pin-editor-row"));
    assert!(js.contains("functionParamRow"));
    assert!(js.contains("function pinCardHtml"));
    assert!(js.contains("pinPortHtml"));
    assert!(js.contains("function-return-type"));
    assert!(js.contains("add-function-output"));
    assert!(js.contains("set-function-output"));
    assert!(js.contains("remove-function-output"));
    assert!(js.contains("handleFunctionPinButton"));
    assert!(js.contains("button.id === \"remove-function-output\" ? \"Void\""));
    assert!(js.contains("function applyFunctionPins"));
    assert!(js.contains("function nextParamName"));
    assert!(js.contains("function syncReturnEditorPreview"));
    assert!(js.contains("function-return-type-chip"));
    assert!(js.contains("Output pin ready"));
    assert!(js.contains("<h2>Events</h2>"));
    assert!(js.contains("undoStack"));
    assert!(js.contains("redoStack"));
    assert!(js.contains("editorState"));
    assert!(js.contains("jet.canvas.editor:"));
    assert!(js.contains("function alignSelectedNodes"));
    assert!(js.contains("function tidyGraphLayout"));
    assert!(js.contains("function addRerouteKnot"));
    assert!(js.contains("function bookmarkCurrentGraph"));
    assert!(js.contains("function jumpBookmark"));
    assert!(js.contains("function toggleFavoriteAction"));
    assert!(js.contains("function runCurrentGraph"));
    assert!(js.contains("function renderCommandAuthority"));
    assert!(js.contains("executeCommandAuthority"));
    assert!(js.contains("commandUrl"));
    assert!(js.contains("__JET_CANVAS_COMMAND__"));
    assert!(js.contains("__jetCanvasRunLoop"));
    assert!(js.contains("authority_required"));
    assert!(!js.contains("last run projected into debug overlay"));
    assert!(js.contains("function visibleGraphNodes"));
    assert!(js.contains("__jetCanvasVirtualizationStats"));
    assert!(js.contains("wireStyle = \"bezier\""));
    assert!(!js.contains("wireStyle === \"straight\""));
    assert!(js.contains("view.zoom < .38"));
    assert!(js.contains("function typeExplanation"));
    assert!(js.contains("Favorite pinned"));
    assert!(js.contains("Bookmark saved"));
    assert!(js.contains("Graph tidied"));
    assert!(js.contains("replace_source"));
    assert!(js.contains("insert_branch"));
    assert!(js.contains("insert_switch"));
    assert!(js.contains("insert_loop"));
    assert!(js.contains("insert_fallible_rail"));
    assert!(js.contains("insert_call"));
    assert!(js.contains("selectedNodeIds"));
    assert!(js.contains("graphBackStack"));
    assert!(js.contains("graphForwardStack"));
    assert!(js.contains("function switchGraph"));
    assert!(js.contains("function updateGraphNav"));
    assert!(js.contains("Back to "));
    assert!(js.contains("Forward to "));
    assert!(js.contains("mode: \"marquee\""));
    assert!(js.contains("mode: \"node\""));
    assert!(js.contains("contextmenu"));
    assert!(js.contains("function renderActionPalette"));
    assert!(js.contains("action-palette-search"));
    assert!(js.contains("Canvas actions"));
    assert!(js.contains("All nodes · ${matches.length}/${contextMenuState.actions.length}"));
    assert!(!js.contains("right-click built-ins, functions, source actions"));
    assert!(js.contains("ArrowRight"));
    assert!(js.contains("function graphActionItems"));
    assert!(js.contains("function openGraphActionPalette"));
    assert!(js.contains("Align top"));
    assert!(js.contains("Auto tidy"));
    assert!(js.contains("Add reroute knot"));
    assert!(js.contains("Bookmark graph"));
    assert!(js.contains("Run graph"));
    assert!(!js.contains("paletteSearch.focus"));
    assert!(js.contains("__jetCanvasPinAuthoring"));
    assert!(js.contains("compatiblePin"));
    assert!(js.contains("function connectionPlan"));
    assert!(js.contains("function completeConnection"));
    assert!(js.contains("pendingPin"));
    assert!(js.contains("function setPendingPin"));
    assert!(js.contains("__jetCanvasPendingPin"));
    assert!(js.contains("__jetCanvasFrontendFamily"));
    assert!(js.contains("family: \"modified_hybrid\""));
    assert!(js.contains("codeSplitGraphLens: true"));
    assert!(js.contains("workbenchProjectViewer: true"));
    assert!(js.contains("dragPinCompatibleMenu: true"));
    assert!(js.contains("hoverOnlyTypes: true"));
    assert!(js.contains("getterCapsules: true"));
    assert!(js.contains("embeddedVariables: true"));
    assert!(js.contains("graphiteDetailToggles"));
    assert!(js.contains("detailToggles"));
    assert!(js.contains("function syncDetailToggles"));
    assert!(js.contains("data-detail-toggle"));
    assert!(js.contains("data-view-mode"));
    assert!(js.contains("__jetCanvasLensMode"));
    assert!(js.contains("Select destination socket"));
    assert!(js.contains("function drawCompatibleDropTargets"));
    assert!(js.contains("function drawConnectionBadge"));
    assert!(js.contains("function syncWireStatus"));
    assert!(!js.contains("function syncNodeRibbon"));
    assert!(!js.contains("const toolColors"));
    assert!(!js.contains("nodeToolGlyph"));
    assert!(js.contains("__jetCanvasLastConnectionPlan"));
    assert!(js.contains("const hitR = Math.max(12, 18 * view.zoom)"));
    assert!(js.contains("bestDistance"));
    assert!(js.contains("Drop on an input socket"));
    assert!(js.contains("Type mismatch"));
    assert!(js.contains("compatibleActionType"));
    assert!(js.contains("const TYPE_COLOR_MAP"));
    assert!(js.contains("Bool: \"#c0392b\""));
    assert!(js.contains("Int: \"#2ec4b6\""));
    assert!(js.contains("Float: \"#9acd32\""));
    assert!(js.contains("String: \"#c678dd\""));
    assert!(js.contains("Void: \"#6b7280\""));
    assert!(js.contains("const NODE_ARCHETYPE_STYLES"));
    assert!(js.contains("function drawArchetypeHeader"));
    assert!(js.contains("function nodeStyle"));
    assert!(js.contains("function nodeKindLabel"));
    assert!(js.contains("function shouldDrawNodeBadge"));
    assert!(js.contains("function nodeSize"));
    assert!(js.contains("function drawTypeLegend"));
    assert!(js.contains("if (!detailToggles.types || compactCanvasMode()"));
    assert!(js.contains("[\"Bool\", \"Bool\"]"));
    assert!(js.contains("developerMode && !!node && !!node.kind"));
    assert!(!js.contains("nodeKindLabel(node, graph).toUpperCase()"));
    assert!(!js.contains("node.kind === \"constant\" ? \"LIT\" : \"GET\""));
    assert!(!js.contains("style.glyph, x + 33"));
    assert!(js.contains("function reflowGraph"));
    assert!(js.contains("autoNodeOffsets"));
    assert!(js.contains("Math.max(.42"));
    assert!(js.contains(
        "const topInset = compact ? (developerMode ? 154 : 52) : (developerMode ? 108 : 32)"
    ));
    assert!(js.contains("const leftInset = 22"));
    assert!(js.contains("requestAnimationFrame(fitGraph)"));
    assert!(js.contains("function compactCanvasMode"));
    assert!(js.contains("function setDrawer"));
    assert!(!js.contains("dockPalette.addEventListener"));
    assert!(js.contains("is-drawer-open"));
    assert!(js.contains("function syncGraphStrip"));
    assert!(js.contains("function syncGraphOverview"));
    assert!(js.contains("data-sidebar-graph"));
    assert!(js.contains("function nodeContextActions"));
    assert!(js.contains("__jetCanvasGraphOverview"));
    assert!(js.contains("__jetCanvasGraphSwitcherReady"));
    assert!(js.contains("__jetCanvasSelectedGraphId"));
    assert!(js.contains("data-graph-tab"));
    assert!(js.contains("button.type = \"button\""));
    assert!(js.contains("ev.stopPropagation()"));
    assert!(js.contains("graph-tab"));
    assert!(js.contains("function isExecPin"));
    assert!(js.contains("function pinName"));
    assert!(js.contains("function exactPinType"));
    assert!(js.contains("function drawPinLabel"));
    assert!(js.contains("function drawPinHoverTooltip"));
    assert!(js.contains("drawPinHoverTooltip(hoverPin)"));
    assert!(js.contains("function drawSocketRow"));
    assert!(js.contains("function drawPinDefaultEditor"));
    assert!(js.contains("function drawInlineExprChip"));
    assert!(js.contains("function isLiteralDefault"));
    assert!(js.contains("function applyPinDefaultEditor"));
    assert!(js.contains("__jetCanvasPinDefaultEditors"));
    assert!(js.contains("__jetCanvasInlineExprChips"));
    assert!(js.contains("op: \"edit_inline_expr\""));
    assert!(js.contains("connectedPinIds.has(pin.pin_id)"));
    assert!(!js.contains("function drawLaneTag"));
    assert!(js.contains("function drawWireTypeBadge"));
    assert!(js.contains("function drawWireArrow"));
    assert!(js.contains("function drawWire"));
    assert!(js.contains("function bezierPoint"));
    assert!(js.contains("function bezierControls"));
    assert!(js.contains("ctx.lineTo(x + r * 1.25, y)"));
    assert!(js.contains("selectedWire"));
    assert!(js.contains("const NODE_HEADER_H = 26"));
    assert!(js.contains("function measureNodeLayout"));
    assert!(js.contains("ctx.measureText"));
    assert!(js.contains("__jetCanvasMeasuredNodeSizing"));
    assert!(js.contains("pin.pattern_source"));
    assert!(js.contains("contextMenuOpenedAt"));
    assert!(js.contains("drag = { mode: \"pin\", pin: endpoint.pin"));
    assert!(!js.contains("node.kind === \"branch\" || node.kind === \"dispatch\" ? 296 : 232"));
    assert!(js.contains("function graphForFunctionName"));
    assert!(js.contains("function openFunctionGraph"));
    assert!(js.contains("Open function"));
    assert!(js.contains("open-callee-graph"));
    assert!(js.contains("variable_get"));
    assert!(js.contains("constant"));
    assert!(js.contains("Set variable"));
    assert!(js.contains("__jetCanvasBindingTypeAccent"));
    assert!(js.contains("label: \"Value\""));
    assert!(js.contains("label: \"Literal\""));
    assert!(js.contains("function isGetterCapsule"));
    assert!(js.contains("function simpleEmbeddedValue"));
    assert!(js.contains("__jetCanvasGetterCapsules"));
    assert!(js.contains("__jetCanvasEmbeddedVariables"));
    assert!(!js.contains("Refused: E0204"));
    assert!(js.contains("functionsForPin"));
    assert!(js.contains("function actionInsertsNode"));
    assert!(js.contains("openPinMenu"));
    assert!(js.contains("Create function accepting"));
    assert!(js.contains("function paletteCategoryForAction"));
    assert!(js.contains("function paletteActionGlyph"));
    assert!(js.contains("function variableActionsForGraph"));
    assert!(js.contains("title: name"));
    assert!(!js.contains("title: \"get \" + name"));
    assert!(js.contains("function loadCoreCatalogActions"));
    assert!(js.contains("__jetCanvasVariablePalette"));
    assert!(js.contains("__jetCanvasCoreCatalogPalette"));
    assert!(js.contains("category === \"Core\" ? 1000"));
    assert!(js.contains("module_path: action.module_path"));
    assert!(js.contains("kind: \"variable_get\""));
    assert!(js.contains("kind: \"variable_set\""));
    assert!(js.contains("\"Add connected node\""));
    assert!(js.contains("op: \"insert_call\""));
    assert!(js.contains("const callee = String(module.path || \"core\") + \".\" + member.name"));
    assert!(js.contains("default_args: member.default_args || [\"1\"]"));
    assert!(!js.contains("<span class=\"tag\">${escapeHtml(category)}</span>"));
    assert!(js.contains("function defaultArgsForAction"));
    assert!(js.contains("function fuzzyScoreText"));
    assert!(js.contains("__jetCanvasFuzzyScore"));
    assert!(js.contains("actionFuzzyScore(action, query)"));
    assert!(js.contains("function restoreNodePositions"));
    assert!(js.contains("function rememberSelectedNodePositions"));
    assert!(js.contains("if (hasSavedNodePositions(graph)) return"));
    assert!(js.contains("if (drag && drag.mode === \"node\") return"));
    assert!(js.contains("function hitWireEndpointAt"));
    assert!(js.contains("wireEndpointHit.push"));
    assert!(js.contains("wire_inline_expr_id"));
    assert!(js.contains("wire_origin_pin_id"));
    assert!(js.contains("wire_target_pin"));
    assert!(js.contains("const projectFunctions = (doc.project_functions || [])"));
    assert!(js.contains("project_function"));
    assert!(js.contains("action-category"));
    assert!(js.contains("move_link"));
    assert!(js.contains("viewMode"));
    assert!(js.contains("setViewMode"));
    assert!(js.contains("setSourceEditMode"));
    assert!(js.contains("applySourceEditBuffer"));
    assert!(js.contains("source_edit: true"));
    assert!(js.contains("function setDeveloperMode"));
    assert!(js.contains("storedFlag(\"jet.canvas.developerMode\")"));
    assert!(js.contains("developerModeButton.addEventListener"));
    assert!(js.contains("document.body.classList.toggle(\"is-dev-mode\""));
    assert!(js.contains("toolbarSearch.addEventListener"));
    assert!(js.contains("document.body.classList.toggle(\"is-debug-active\""));
    assert!(js.contains("Apply pins"));
    assert!(js.contains("Visible conversion function"));
    assert!(js.contains("Wire refused"));
    assert!(js.contains("promote_to_binding"));
    assert!(js.contains("insert_visible_conversion"));
    assert!(js.contains("Promote to binding"));
    assert!(js.contains("__jetCanvasDebugOverlay"));
    assert!(js.contains("debugUrl"));
    assert!(js.contains("breakpoint_spans"));
    assert!(js.contains("active_node_id"));
    assert!(js.contains("Debug overlay stopped"));
    assert!(js.contains("create_comment_region"));
    assert!(js.contains("edit_comment_region"));
    assert!(js.contains("Apply comment"));
    assert!(js.contains("preview_extract_inline_expr"));
    assert!(js.contains("extract_inline_expr"));
    assert!(js.contains("Extract function"));

    let (status, graph) = http_get(port, "/canvas/graph").expect("GET Canvas graph");
    assert_eq!(status, 200);
    let graph = String::from_utf8_lossy(&graph);
    assert!(graph.contains("\"protocol\":\"jet.canvas.graph\""));
    assert!(graph.contains("\"schema_version\":1"));
    assert!(graph.contains("\"nodes\""));
    assert!(graph.contains("\"pins\""));
    assert!(graph.contains("\"layout_hints\""));
    assert!(graph.contains("\"source_span\""));
    assert!(graph.contains("\"source_text\""));

    let src = fs::read_to_string(&src_path).unwrap();
    let revision = jet::Canvas::source_revision(&src);
    let req = format!(
        "{{\"schema_version\":1,\"op\":\"noop\",\"revision\":\"{}\"}}",
        revision
    );
    let (status, body) = http_post(port, "/canvas/transaction", &req).expect("POST Canvas noop");
    assert_eq!(status, 200);
    assert!(String::from_utf8_lossy(&body).contains("\"protocol\":\"jet.canvas.edit\""));

    let query = format!(
        "{{\"schema_version\":1,\"op\":\"find\",\"revision\":\"{}\",\"query\":\"run\"}}",
        revision
    );
    let (status, body) = http_post(port, "/canvas/query", &query).expect("POST Canvas query");
    assert_eq!(status, 200);
    assert!(String::from_utf8_lossy(&body).contains("\"protocol\":\"jet.canvas.query\""));

    let (status, body) =
        http_get(port, "/canvas/source-control").expect("GET Canvas source control");
    assert_eq!(status, 200);
    assert!(String::from_utf8_lossy(&body).contains("\"protocol\":\"jet.canvas.source_control\""));

    let (status, body) = http_get(port, "/canvas/proof").expect("GET Canvas proof");
    assert_eq!(status, 200);
    let proof = String::from_utf8_lossy(&body);
    assert!(proof.contains("\"protocol\":\"jet.canvas.proof\""), "{proof}");
    assert!(proof.contains("\"schema_version\":1"), "{proof}");
    assert!(proof.contains("\"check\":{\"state\":\"ok\""), "{proof}");
    assert!(proof.contains("\"command_receipts\":{\"state\":\"missing\""), "{proof}");

    let command_req = format!(
        "{{\"schema_version\":1,\"action_id\":\"canvas.command:check\",\"revision\":\"{}\"}}",
        revision
    );
    let (status, receipt) =
        http_post(port, "/canvas/command", &command_req).expect("POST Canvas command");
    let receipt = String::from_utf8_lossy(&receipt);
    assert_eq!(status, 200, "command receipt response: {receipt}");
    assert!(
        receipt.contains("\"protocol\":\"jet.canvas.command_receipt\""),
        "{receipt}"
    );
    assert!(receipt.contains("\"action_id\":\"canvas.command:check\""), "{receipt}");
    assert!(receipt.contains("\"command\":[\"jet\",\"check\",\"app.jet\"]"), "{receipt}");
    assert!(receipt.contains("\"success\":true"), "{receipt}");

    let (status, body) = http_get(port, "/canvas/proof").expect("GET Canvas proof after command");
    assert_eq!(status, 200);
    let proof = String::from_utf8_lossy(&body);
    assert!(proof.contains("\"command_receipts\":{\"state\":\"current\""), "{proof}");
    assert!(proof.contains("\"protocol\":\"jet.canvas.command_receipt\""), "{proof}");
    assert!(proof.contains("\"proof\":{\"state\":\"current\",\"stale\":false"), "{proof}");

    fs::write(
        dir.join("pkg.jet"),
        "payload: {\n    name: \"canvas_app\",\n    version: \"0.1.0\",\n}\n",
    )
    .unwrap();
    fs::write(dir.join("helper.jet"), "fn run() {\n    print(\"helper\")\n}\n").unwrap();
    let (status, project) = http_get(port, "/canvas/project").expect("GET Canvas project");
    assert_eq!(status, 200);
    let project = String::from_utf8_lossy(&project);
    assert!(project.contains("\"protocol\":\"jet.canvas.project\""));
    assert!(project.contains("\"schema_version\":1"));
    assert!(project.contains("\"project_revision\":\"sha256-"));
    assert!(project.contains("\"entry\""));
    assert!(project.contains("\"state_policy\""));

    let (status, helper_graph) =
        http_get(port, "/canvas/graph?source_id=helper.jet").expect("GET helper graph");
    assert_eq!(
        status,
        200,
        "helper graph response: {}",
        String::from_utf8_lossy(&helper_graph)
    );
    let helper_graph = String::from_utf8_lossy(&helper_graph);
    assert!(helper_graph.contains("helper.jet"), "{helper_graph}");
    assert!(helper_graph.contains("print(\\\"helper\\\")"), "{helper_graph}");

    let (status, helper_proof) =
        http_get(port, "/canvas/proof?source_id=helper.jet").expect("GET helper proof");
    assert_eq!(status, 200);
    let helper_proof = String::from_utf8_lossy(&helper_proof);
    assert!(helper_proof.contains("\"protocol\":\"jet.canvas.proof\""), "{helper_proof}");
    assert!(helper_proof.contains("\"source_id\":\"helper.jet\""), "{helper_proof}");

    let helper_src = fs::read_to_string(dir.join("helper.jet")).unwrap();
    let helper_revision = jet::Canvas::source_revision(&helper_src);
    let helper_query = format!(
        "{{\"schema_version\":1,\"op\":\"find\",\"source_id\":\"helper.jet\",\"revision\":\"{}\",\"query\":\"run\"}}",
        helper_revision
    );
    let (status, helper_query_body) =
        http_post(port, "/canvas/query", &helper_query).expect("POST helper query");
    assert_eq!(status, 200);
    assert!(String::from_utf8_lossy(&helper_query_body).contains("\"protocol\":\"jet.canvas.query\""));

    let (status, catalog) =
        http_get(port, "/canvas/core-catalog?query=http").expect("GET Canvas core catalog");
    assert_eq!(status, 200);
    let catalog = String::from_utf8_lossy(&catalog);
    assert!(catalog.contains("\"op\":\"core_catalog\""), "{catalog}");
    assert!(catalog.contains("\"catalog_schema_version\":1"), "{catalog}");
    assert!(catalog.contains("\"authority\":[\"canvas.catalog:core.read\"]"), "{catalog}");
    assert!(catalog.contains("\"path\":\"core.http\""), "{catalog}");
    assert!(catalog.contains("\"source\":\"docs/reference/core-library.md\""), "{catalog}");

    let project_revision = json_field(&project, "project_revision");
    let manifest_src = fs::read_to_string(dir.join("pkg.jet")).unwrap();
    let manifest_revision = jet::Canvas::source_revision(&manifest_src);
    let project_req = format!(
        "{{\"schema_version\":1,\"op\":\"add_dependency\",\"preview\":true,\"project_revision\":\"{}\",\"files\":[{{\"path\":\"pkg.jet\",\"revision\":\"{}\"}}],\"manifest\":\"pkg.jet\",\"name\":\"logging\",\"spec\":\"0.1.0\"}}",
        project_revision, manifest_revision
    );
    let (status, body) =
        http_post(port, "/canvas/project/transaction", &project_req).expect("POST project tx");
    assert_eq!(status, 200);
    let body = String::from_utf8_lossy(&body);
    assert!(body.contains("\"protocol\":\"jet.canvas.project.edit\""), "{body}");
    assert!(body.contains("\"writes\":\"preview_only\""), "{body}");
    assert!(body.contains("+    logging: \\\"0.1.0\\\","), "{body}");
    assert!(!fs::read_to_string(dir.join("pkg.jet")).unwrap().contains("logging"));

    drop(guard);
    let _ = fs::remove_dir_all(&dir);
}

// ============================================================================
// Section: user-defined `fn dev()` entry mode (was tests/web_dev_fn.rs)
// ============================================================================

fn fn_fixture_src(port: u16, greeting: &str) -> String {
    format!(
        r#"use core.devserver as devserver

fn dev() {{
    server :: devserver.for_app("app.jet")
    server.port({port})
    server.serve()
}}

fn run() {{
    print("{greeting}")
}}
"#
    )
}

/// Polls for the server to come up on the port the fixture's own `dev()` passes
/// to `.port(...)` — never a blind `sleep`.
fn wait_for_server_up(port: u16, timeout: Duration) {
    let start = Instant::now();
    loop {
        if matches!(http_get(port, "/__jet_dev_version"), Some((200, _))) {
            return;
        }
        if start.elapsed() > timeout {
            panic!(
                "jet dev's own core.devserver never came up on port {}",
                port
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn jet_dev_runs_user_defined_dev_fn_and_rebuilds_on_save() {
    if !have_tool("rustc") {
        eprintln!(
            "note: skipping jet_dev_runs_user_defined_dev_fn_and_rebuilds_on_save (need rustc)"
        );
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
    let port = unused_local_port();
    fs::write(&src_path, fn_fixture_src(port, "hello, world")).unwrap();

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

    fs::write(&src_path, fn_fixture_src(port, "hello there")).unwrap();
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
    let port = unused_local_port();
    fs::write(
        &src_path,
        format!(
            r#"use core.devserver as devserver

fn dev() {{
    server :: devserver.app()
    server.port({port})
    server.serve()
}}

fn run() {{
    print("hello, world")
}}
"#
        ),
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
