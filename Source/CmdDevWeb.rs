//! `jet dev <file>.jet --target=web` (c134 Phase 7): compile-once, watch,
//! rebuild-on-save, and serve the `build/` folder with browser live-reload —
//! std-only (I6: no `notify`, no HTTP-server crate, no WebSocket crate).
//!
//! This is a completely different execution model from the native `jet dev`
//! (`run_dev` in `CmdDevTools.rs`), which interprets/hot-swaps the program in
//! process. Compiling to JS/WASM has nothing to hot-swap in that sense — the
//! only thing a save can do is trigger a full recompile, so this module only
//! reuses the *mtime-poll watch pattern* (and the `file_mtime` helper itself)
//! from `run_dev`, not any of its interpreter/JIT machinery.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::exit;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use jet::ExitCodes;

use crate::CmdCompile::write_web_artifacts;
use crate::CmdDevTools::file_mtime;
use crate::{report_problems, OutputMode};

/// Ports tried, in order, before giving up. 8080 is the conventional static-
/// dev-server port; a small bounded scan upward covers "something else is
/// already on 8080" without hunting indefinitely (judgment call — 10 ports is
/// generous for a dev tool binding localhost).
const PORT_RANGE: std::ops::RangeInclusive<u16> = 8080..=8089;

/// How often the live-reload script in the browser polls `/__jet_dev_version`
/// (judgment call — 400ms is fast enough to feel instant after a save,
/// without hammering the dev server on every open tab).
const LIVE_RELOAD_POLL_MS: u64 = 400;

#[derive(Clone)]
struct DevStatusSnapshot {
    state: String,
    message: String,
    diagnostic: String,
    last_build_ms: u128,
}

struct DevStatus {
    version: AtomicU64,
    clients: AtomicU64,
    state: Mutex<DevStatusSnapshot>,
    command_receipt: Mutex<Option<String>>,
}

impl DevStatus {
    fn new() -> DevStatus {
        DevStatus {
            version: AtomicU64::new(1),
            clients: AtomicU64::new(0),
            state: Mutex::new(DevStatusSnapshot {
                state: "ready".to_string(),
                message: "initial build ready".to_string(),
                diagnostic: String::new(),
                last_build_ms: 0,
            }),
            command_receipt: Mutex::new(None),
        }
    }

    fn mark_building(&self, file: &str) {
        *self.state.lock().unwrap() = DevStatusSnapshot {
            state: "building".to_string(),
            message: format!("building {file}"),
            diagnostic: String::new(),
            last_build_ms: 0,
        };
    }

    fn mark_ready(&self, file: &str, elapsed_ms: u128, bump_version: bool) {
        if bump_version {
            self.version.fetch_add(1, Ordering::SeqCst);
        }
        *self.state.lock().unwrap() = DevStatusSnapshot {
            state: "ready".to_string(),
            message: format!("built {file} in {elapsed_ms}ms"),
            diagnostic: String::new(),
            last_build_ms: elapsed_ms,
        };
    }

    fn mark_error(&self, diagnostic: String) {
        *self.state.lock().unwrap() = DevStatusSnapshot {
            state: "error".to_string(),
            message: "build failed; serving last good output".to_string(),
            diagnostic,
            last_build_ms: 0,
        };
    }

    fn json(&self) -> String {
        let state = self.state.lock().unwrap().clone();
        format!(
            "{{\"version\":{},\"state\":\"{}\",\"message\":\"{}\",\"diagnostic\":\"{}\",\"clients\":{},\"last_build_ms\":{}}}",
            self.version.load(Ordering::SeqCst),
            json_escape(&state.state),
            json_escape(&state.message),
            json_escape(&state.diagnostic),
            self.clients.load(Ordering::SeqCst),
            state.last_build_ms
        )
    }

    fn record_command_receipt(&self, receipt: String) {
        *self.command_receipt.lock().unwrap() = Some(receipt);
    }

    fn command_receipt(&self) -> Option<String> {
        self.command_receipt.lock().unwrap().clone()
    }

    fn terminal_line(&self, port: u16, file: &str) -> String {
        let state = self.state.lock().unwrap().clone();
        format!(
            "jet dev  [{}] localhost:{} · {} clients · {} · watching {}",
            state.state,
            port,
            self.clients.load(Ordering::SeqCst),
            state.message,
            file
        )
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// `jet dev <file>.jet --target=web`: build once, serve `build/` over plain
/// HTTP with live-reload, then watch `file` and rebuild on every save.
/// `port`: `--port=<N>` if given — bind that exact port, no fallback scan
/// (an explicit choice fails loud, same convention as `PORT=3000 node ...`).
/// `None` scans `PORT_RANGE` starting at 8080.
pub(crate) fn run_dev_web(file: &str, mode: OutputMode, verbose: bool, port: Option<u16>) {
    let path = Path::new(file);
    if !path.exists() {
        eprintln!("error: can't find the file `{}`", file);
        eprintln!(
            " fix: check the spelling, or run {} from the folder that contains it",
            jet::Syntax::BINARY_NAME
        );
        exit(ExitCodes::USER_ERROR);
    }

    // The initial build must succeed — there's nothing to serve otherwise.
    // `rebuild_web` already prints diagnostics/ICE messages on failure.
    let status = Arc::new(DevStatus::new());
    if !rebuild_web(file, mode, verbose, false, Some(&status)) {
        exit(ExitCodes::USER_ERROR);
    }

    let listener = bind_dev_server(port);
    let port = listener
        .local_addr()
        .map(|a| a.port())
        .unwrap_or(*PORT_RANGE.start());

    {
        let status = Arc::clone(&status);
        let canvas_file = file.to_string();
        thread::spawn(move || serve_forever(listener, status, canvas_file));
    }

    println!(
        "serving http://localhost:{} — watching {} … (Ctrl-C to stop)",
        port, file
    );
    println!("{}", status.terminal_line(port, file));
    println!("Canvas: http://localhost:{}/canvas", port);

    // Same mtime-poll/debounce shape as `run_dev` (Source/CmdDevTools.rs),
    // reusing `file_mtime` verbatim (I6: no filesystem-notification crate).
    let mut last_mtime = file_mtime(path);
    loop {
        thread::sleep(Duration::from_millis(120));
        let now = file_mtime(path);
        if now != last_mtime {
            last_mtime = now;
            // A debounce sleep lets editors finish writing before we read.
            thread::sleep(Duration::from_millis(30));
            if rebuild_web(file, mode, verbose, true, Some(&status)) {
                eprintln!("{}", status.terminal_line(port, file));
            }
            // On failure: `rebuild_web` never touches `build/` until every
            // artifact (including the wasm rustc pass) has compiled clean —
            // see `stage_and_swap` — so the dev server keeps serving the last
            // good build, exactly as a broken mid-edit save should behave.
        }
    }
}

/// Compile `file` for the web target and, on success, atomically replace
/// `build/*` with the new output. Returns whether the rebuild succeeded.
///
/// A front-end compile error (the common case while editing) never writes to
/// `build/` at all — diagnostics are reported and the previous build stands.
/// A codegen/rustc failure (I2: always an internal compiler error, not the
/// user's fault) is reported the same way and likewise leaves `build/`
/// untouched, because the new artifacts are written to a staging directory
/// first and only moved into place once the whole set, wasm included,
/// compiled successfully.
fn rebuild_web(
    file: &str,
    mode: OutputMode,
    verbose: bool,
    is_rebuild: bool,
    status: Option<&DevStatus>,
) -> bool {
    let started = Instant::now();
    if let Some(status) = status {
        status.mark_building(file);
    }
    let src = fs::read_to_string(file).unwrap_or_default();
    let out = match jet::compile_web(file) {
        Ok(out) => out,
        Err(diags) => {
            if is_rebuild {
                eprintln!("\n— {} changed —", file);
            }
            report_problems(mode, file, &src, &diags);
            if let Some(status) = status {
                status.mark_error(jet::render_diagnostics(file, &src, &diags));
            }
            return false;
        }
    };
    let web = match &out.web {
        Some(w) => w,
        None => {
            eprintln!("error: internal compiler error: missing web codegen output");
            if let Some(status) = status {
                status.mark_error("internal compiler error: missing web codegen output".to_string());
            }
            return false;
        }
    };

    let staging = PathBuf::from("build").join(".jet-dev-staging");
    if let Err(msg) = write_web_artifacts(file, web, verbose, &staging) {
        eprintln!("{}", msg);
        if let Some(status) = status {
            status.mark_error(msg);
        }
        return false;
    }
    if let Err(e) = stage_and_swap(&staging, Path::new("build")) {
        eprintln!("error: couldn't finalize web build: {}", e);
        if let Some(status) = status {
            status.mark_error(format!("couldn't finalize web build: {e}"));
        }
        return false;
    }

    if let Some(status) = status {
        status.mark_ready(file, started.elapsed().as_millis(), is_rebuild);
    }
    if is_rebuild {
        eprintln!("[dev] rebuilt after change to {}", file);
    }
    true
}

/// Move every file `write_web_artifacts` staged into the real output
/// directory. Each `fs::rename` is atomic on the same filesystem (staging
/// lives under `build/`, so it always is), so a reader never observes a
/// half-written file; the individual renames aren't a single transaction, but
/// they only run after the entire staged set — including the wasm compile —
/// has already succeeded, so there is nothing left that can fail mid-swap
/// except a bare I/O error.
fn stage_and_swap(staging: &Path, out_dir: &Path) -> std::io::Result<()> {
    const FILES: [&str; 6] = [
        "web.manifest.json",
        "jet_dom_runtime.js",
        "app.js",
        "app_wasm.rs",
        "app.wasm",
        "index.html",
    ];
    fs::create_dir_all(out_dir)?;
    for name in FILES {
        let src = staging.join(name);
        let dst = out_dir.join(name);
        // `write_web_artifacts` already returned success for every one of
        // these paths (rustc included) before we get here, so a rename
        // failure here is a transient filesystem-visibility race, not a
        // missing file — a handful of retries with a short backoff clears it
        // without masking a genuine bug (which would fail every retry too).
        rename_with_retry(&src, &dst)?;
    }
    Ok(())
}

fn rename_with_retry(src: &Path, dst: &Path) -> std::io::Result<()> {
    let mut last_err = None;
    for attempt in 0..5 {
        if attempt > 0 {
            thread::sleep(Duration::from_millis(20 * attempt as u64));
        }
        match fs::rename(src, dst) {
            Ok(()) => return Ok(()),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap())
}

/// Bind the dev server's `TcpListener`. `Some(port)` (from `--port=<N>`)
/// binds that exact port and fails loud if it's taken — an explicit choice
/// isn't a hint. `None` scans `PORT_RANGE` for a free port.
fn bind_dev_server(port: Option<u16>) -> TcpListener {
    if let Some(port) = port {
        return match TcpListener::bind(("127.0.0.1", port)) {
            Ok(listener) => listener,
            Err(e) => {
                eprintln!("error: couldn't bind to port {}: {}", port, e);
                if e.kind() == std::io::ErrorKind::AddrInUse {
                    eprintln!(
                        " fix: stop whatever's using port {}, or pick another with --port=<N>",
                        port
                    );
                }
                exit(ExitCodes::USER_ERROR);
            }
        };
    }
    let mut last_err: Option<std::io::Error> = None;
    for port in PORT_RANGE {
        match TcpListener::bind(("127.0.0.1", port)) {
            Ok(listener) => return listener,
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                last_err = Some(e);
                continue;
            }
            Err(e) => {
                eprintln!("error: couldn't start the dev server: {}", e);
                exit(ExitCodes::USER_ERROR);
            }
        }
    }
    eprintln!(
        "error: every port from {} to {} is already in use{}",
        PORT_RANGE.start(),
        PORT_RANGE.end(),
        last_err.map(|e| format!(" ({})", e)).unwrap_or_default()
    );
    eprintln!(" fix: free one of those ports, stop the other process using it, or pick one explicitly with --port=<N>");
    exit(ExitCodes::USER_ERROR);
}

/// Accept connections forever, one thread per connection (a dev tool serving
/// a handful of small files has no need for a worker pool).
fn serve_forever(listener: TcpListener, status: Arc<DevStatus>, canvas_file: String) {
    for stream in listener.incoming() {
        if let Ok(stream) = stream {
            let status = Arc::clone(&status);
            let canvas_file = canvas_file.clone();
            thread::spawn(move || {
                status.clients.fetch_add(1, Ordering::SeqCst);
                let _ = handle_connection(stream, &status, &canvas_file);
                status.clients.fetch_sub(1, Ordering::SeqCst);
            });
        }
    }
}

/// Parse one GET request line + headers (no keep-alive, no request body —
/// this is a dev tool, not a production server) and serve a response.
fn handle_connection(
    stream: TcpStream,
    status: &DevStatus,
    canvas_file: &str,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(());
    }
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        let lower = line.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            content_length = v.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }

    let mut stream = stream;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("/");

    if method != "GET" && method != "POST" {
        return write_response(
            &mut stream,
            "405 Method Not Allowed",
            "text/plain; charset=utf-8",
            b"jet dev's web server only handles GET and Canvas POST requests",
        );
    }

    let path = target.split('?').next().unwrap_or("/");
    if target == "/?jet_panel=1" {
        if method != "GET" {
            return method_not_allowed(&mut stream);
        }
        let body = jet::Canvas::canvas_html_query();
        return write_response(
            &mut stream,
            "200 OK",
            "text/html; charset=utf-8",
            body.as_bytes(),
        );
    }
    if target == "/?jet_panel_app=1" {
        if method != "GET" {
            return method_not_allowed(&mut stream);
        }
        let body = jet::Canvas::canvas_js();
        return write_response(
            &mut stream,
            "200 OK",
            "application/javascript; charset=utf-8",
            body.as_bytes(),
        );
    }
    if target == "/?jet_panel_graph=1" {
        if method != "GET" {
            return method_not_allowed(&mut stream);
        }
        return match jet::Canvas::graph_json_for_file(Path::new(canvas_file)) {
            Ok(body) => write_response(
                &mut stream,
                "200 OK",
                "application/json; charset=utf-8",
                body.as_bytes(),
            ),
            Err(diags) => {
                let src = fs::read_to_string(canvas_file).unwrap_or_default();
                let body = jet::render_diagnostics(canvas_file, &src, &diags);
                write_response(
                    &mut stream,
                    "409 Conflict",
                    "text/plain; charset=utf-8",
                    body.as_bytes(),
                )
            }
        };
    }
    if path == "/__jet_canvas"
        || path == "/__jet_canvas/"
        || path == "/canvas"
        || path == "/canvas/"
        || path == "/panel"
        || path == "/panel/"
    {
        if method != "GET" {
            return method_not_allowed(&mut stream);
        }
        let base = if path.starts_with("/panel") {
            "/panel"
        } else {
            "/canvas"
        };
        let body = jet::Canvas::canvas_html_for(base);
        return write_response(
            &mut stream,
            "200 OK",
            "text/html; charset=utf-8",
            body.as_bytes(),
        );
    }
    if path == "/__jet_canvas/app.js" || path == "/canvas/app.js" || path == "/panel/app.js" {
        if method != "GET" {
            return method_not_allowed(&mut stream);
        }
        let body = jet::Canvas::canvas_js();
        return write_response(
            &mut stream,
            "200 OK",
            "application/javascript; charset=utf-8",
            body.as_bytes(),
        );
    }
    if path == "/__jet_canvas/graph" || path == "/canvas/graph" || path == "/panel/graph" {
        if method != "GET" {
            return method_not_allowed(&mut stream);
        }
        let source_id = query_param(target, "source_id");
        return match jet::Canvas::graph_json_for_entry_source(Path::new(canvas_file), source_id.as_deref()) {
            Ok(body) => write_response(
                &mut stream,
                "200 OK",
                "application/json; charset=utf-8",
                body.as_bytes(),
            ),
            Err(body) => write_response(
                &mut stream,
                "409 Conflict",
                "application/json; charset=utf-8",
                body.as_bytes(),
            ),
        };
    }
    if path == "/__jet_canvas/project" || path == "/canvas/project" || path == "/panel/project" {
        if method != "GET" {
            return method_not_allowed(&mut stream);
        }
        let body = jet::Canvas::project_json_for_entry(Path::new(canvas_file));
        return write_response(
            &mut stream,
            "200 OK",
            "application/json; charset=utf-8",
            body.as_bytes(),
        );
    }
    if path == "/__jet_canvas/source-control"
        || path == "/canvas/source-control"
        || path == "/panel/source-control"
    {
        if method != "GET" {
            return method_not_allowed(&mut stream);
        }
        let body = jet::Canvas::source_control_json_for_entry(Path::new(canvas_file));
        return write_response(
            &mut stream,
            "200 OK",
            "application/json; charset=utf-8",
            body.as_bytes(),
        );
    }
    if path == "/__jet_canvas/core-catalog"
        || path == "/canvas/core-catalog"
        || path == "/panel/core-catalog"
    {
        if method != "GET" {
            return method_not_allowed(&mut stream);
        }
        let query = query_param(target, "query").unwrap_or_default();
        return match jet::Canvas::core_catalog_json_for_entry(Path::new(canvas_file), &query) {
            Ok(body) => write_response(
                &mut stream,
                "200 OK",
                "application/json; charset=utf-8",
                body.as_bytes(),
            ),
            Err(body) => write_response(
                &mut stream,
                "409 Conflict",
                "application/json; charset=utf-8",
                body.as_bytes(),
            ),
        };
    }
    if path == "/__jet_canvas/proof" || path == "/canvas/proof" || path == "/panel/proof" {
        if method != "GET" {
            return method_not_allowed(&mut stream);
        }
        let source_id = query_param(target, "source_id");
        let receipt = status.command_receipt();
        return match jet::Canvas::proof_json_for_entry_with_receipt(
            Path::new(canvas_file),
            source_id.as_deref(),
            receipt.as_deref(),
        ) {
            Ok(body) => write_response(
                &mut stream,
                "200 OK",
                "application/json; charset=utf-8",
                body.as_bytes(),
            ),
            Err(body) => write_response(
                &mut stream,
                "409 Conflict",
                "application/json; charset=utf-8",
                body.as_bytes(),
            ),
        };
    }
    if path == "/__jet_canvas/source" || path == "/canvas/source" || path == "/panel/source" {
        if method != "GET" {
            return method_not_allowed(&mut stream);
        }
        let source_id = query_param(target, "source_id");
        let source_path = source_id
            .as_deref()
            .and_then(|id| jet::Canvas::project_path_for_source_id(Path::new(canvas_file), id))
            .unwrap_or_else(|| PathBuf::from(canvas_file));
        return match fs::read(&source_path) {
            Ok(body) => write_response(
                &mut stream,
                "200 OK",
                "text/plain; charset=utf-8",
                &body,
            ),
            Err(e) => write_response(
                &mut stream,
                "404 Not Found",
                "text/plain; charset=utf-8",
                format!("could not read Canvas source: {e}").as_bytes(),
            ),
        };
    }
    if path == "/__jet_canvas/command" || path == "/canvas/command" || path == "/panel/command" {
        if method != "POST" {
            return method_not_allowed(&mut stream);
        }
        let request = String::from_utf8_lossy(&body);
        return match jet::Canvas::command_receipt_json_for_entry(Path::new(canvas_file), &request)
        {
            Ok(body) => {
                status.record_command_receipt(body.clone());
                write_response(
                    &mut stream,
                    "200 OK",
                    "application/json; charset=utf-8",
                    body.as_bytes(),
                )
            }
            Err(body) => write_response(
                &mut stream,
                "409 Conflict",
                "application/json; charset=utf-8",
                body.as_bytes(),
            ),
        };
    }
    if path == "/__jet_canvas/transaction"
        || path == "/canvas/transaction"
        || path == "/panel/transaction"
    {
        if method != "POST" {
            return method_not_allowed(&mut stream);
        }
                let request = String::from_utf8_lossy(&body);
        return match jet::Canvas::apply_transaction_json(Path::new(canvas_file), &request) {
            Ok(body) => {
                status.version.fetch_add(1, Ordering::SeqCst);
                write_response(
                    &mut stream,
                    "200 OK",
                    "application/json; charset=utf-8",
                    body.as_bytes(),
                )
            }
            Err(body) => write_response(
                &mut stream,
                "409 Conflict",
                "application/json; charset=utf-8",
                body.as_bytes(),
            ),
        };
    }
    if path == "/__jet_canvas/project/transaction"
        || path == "/canvas/project/transaction"
        || path == "/panel/project/transaction"
    {
        if method != "POST" {
            return method_not_allowed(&mut stream);
        }
        let request = String::from_utf8_lossy(&body);
        return match jet::Canvas::apply_project_transaction_json(Path::new(canvas_file), &request)
        {
            Ok(body) => {
                status.version.fetch_add(1, Ordering::SeqCst);
                write_response(
                    &mut stream,
                    "200 OK",
                    "application/json; charset=utf-8",
                    body.as_bytes(),
                )
            }
            Err(body) => write_response(
                &mut stream,
                "409 Conflict",
                "application/json; charset=utf-8",
                body.as_bytes(),
            ),
        };
    }
    if path == "/__jet_canvas/query" || path == "/canvas/query" || path == "/panel/query" {
        if method != "POST" {
            return method_not_allowed(&mut stream);
        }
        let request = String::from_utf8_lossy(&body);
        return match jet::Canvas::query_json_for_entry(Path::new(canvas_file), &request) {
            Ok(body) => write_response(
                &mut stream,
                "200 OK",
                "application/json; charset=utf-8",
                body.as_bytes(),
            ),
            Err(body) => write_response(
                &mut stream,
                "409 Conflict",
                "application/json; charset=utf-8",
                body.as_bytes(),
            ),
        };
    }
    if path == "/__jet_canvas/debug" || path == "/canvas/debug" || path == "/panel/debug" {
        if method != "POST" {
            return method_not_allowed(&mut stream);
        }
        let request = String::from_utf8_lossy(&body);
        return match jet::Canvas::debug_session_json_for_file(Path::new(canvas_file), &request) {
            Ok(body) => write_response(
                &mut stream,
                "200 OK",
                "application/json; charset=utf-8",
                body.as_bytes(),
            ),
            Err(body) => write_response(
                &mut stream,
                "409 Conflict",
                "application/json; charset=utf-8",
                body.as_bytes(),
            ),
        };
    }
    if path == "/__jet_dev_version" {
        if method != "GET" {
            return method_not_allowed(&mut stream);
        }
        let body = status.version.load(Ordering::SeqCst).to_string();
        return write_response(
            &mut stream,
            "200 OK",
            "text/plain; charset=utf-8",
            body.as_bytes(),
        );
    }
    if path == "/__jet_dev_status" {
        if method != "GET" {
            return method_not_allowed(&mut stream);
        }
        let body = status.json();
        return write_response(
            &mut stream,
            "200 OK",
            "application/json; charset=utf-8",
            body.as_bytes(),
        );
    }
    serve_static(&mut stream, path)
}

fn query_param(target: &str, key: &str) -> Option<String> {
    let (_, query) = target.split_once('?')?;
    for part in query.split('&') {
        let (name, value) = part.split_once('=').unwrap_or((part, ""));
        if name == key {
            return Some(percent_decode(value));
        }
    }
    None
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(a), Some(b)) = (hex_value(bytes[i + 1]), hex_value(bytes[i + 2])) {
                out.push(a * 16 + b);
                i += 3;
                continue;
            }
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn method_not_allowed(stream: &mut TcpStream) -> std::io::Result<()> {
    write_response(
        stream,
        "405 Method Not Allowed",
        "text/plain; charset=utf-8",
        b"method not allowed",
    )
}

/// Serve one file out of `build/`, injecting the live-reload script into
/// `index.html` on the way out. GET-only, no directory listing, no range
/// requests, no compression — a dev tool serving a few small files needs
/// none of that.
fn serve_static(stream: &mut TcpStream, path: &str) -> std::io::Result<()> {
    if path.contains("..") {
        return write_response(
            stream,
            "400 Bad Request",
            "text/plain; charset=utf-8",
            b"bad path",
        );
    }
    let rel = if path == "/" {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };
    let file_path = Path::new("build").join(rel);
    let bytes = match fs::read(&file_path) {
        Ok(b) => b,
        Err(_) => {
            let body = format!("not found: {}", path);
            return write_response(
                stream,
                "404 Not Found",
                "text/plain; charset=utf-8",
                body.as_bytes(),
            );
        }
    };

    let content_type = content_type_for(&file_path);
    if file_path.file_name().and_then(|f| f.to_str()) == Some("index.html") {
        let html = String::from_utf8_lossy(&bytes).into_owned();
        let injected = inject_live_reload(&html);
        return write_response(stream, "200 OK", content_type, injected.as_bytes());
    }
    write_response(stream, "200 OK", content_type, &bytes)
}

fn content_type_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "application/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("json") => "application/json; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Plain polling live-reload (no WebSocket/SSE — I6 + correct scoping for a
/// dev tool): fetch `/__jet_dev_version` on an interval, reload the page the
/// first time it differs from the version seen at page load.
fn live_reload_script() -> String {
    format!(
        r#"<script>
(function () {{
  var jetDevVersion = null;
  var panel = null;
  function ensurePanel() {{
    if (panel) return panel;
    panel = document.createElement("div");
    panel.id = "jet-dev-status";
    panel.setAttribute("style", "position:fixed;right:12px;bottom:12px;z-index:2147483647;max-width:min(560px,calc(100vw - 24px));font:12px ui-monospace, SFMono-Regular, Consolas, monospace;background:#101820;color:#d6e7ff;border:1px solid #3b5675;border-radius:6px;padding:8px 10px;box-shadow:0 8px 24px rgba(0,0,0,.32)");
    document.body.appendChild(panel);
    return panel;
  }}
  function esc(s) {{ return String(s || "").replace(/[&<>]/g, function (c) {{ return {{ "&":"&amp;","<":"&lt;",">":"&gt;" }}[c]; }}); }}
  function renderStatus(s) {{
    var el = ensurePanel();
    var state = s.state || "ready";
    var diag = s.diagnostic || "";
    var head = "jet dev [" + state + "] · " + (s.clients || 0) + " clients · v" + (s.version || 0);
    if (state === "error") {{
      el.innerHTML = "<b>Build failed</b><br><span>" + esc(head) + "</span><pre style=\"white-space:pre-wrap;margin:8px 0 0;max-height:45vh;overflow:auto\">" + esc(diag) + "</pre>";
    }} else {{
      el.innerHTML = "<b>" + esc(head) + "</b><br><span>" + esc(s.message || "") + "</span>";
    }}
  }}
  setInterval(function () {{
    fetch("/__jet_dev_status", {{ cache: "no-store" }})
      .then(function (r) {{ return r.json(); }})
      .then(function (s) {{
        renderStatus(s);
        var v = String(s.version || "");
        if (jetDevVersion === null) {{ jetDevVersion = v; return; }}
        if (v !== jetDevVersion) {{ location.reload(); }}
      }})
      .catch(function () {{}});
  }}, {poll_ms});
}})();
</script>
"#,
        poll_ms = LIVE_RELOAD_POLL_MS
    )
}

fn inject_live_reload(html: &str) -> String {
    let script = live_reload_script();
    // Find the insertion point case-insensitively (HTML tags are ASCII, so a
    // lowercase search never shifts a byte offset), but splice into the
    // ORIGINAL bytes so the rest of the page is untouched.
    if let Some(idx) = html.to_ascii_lowercase().find("</body>") {
        let mut out = String::with_capacity(html.len() + script.len());
        out.push_str(&html[..idx]);
        out.push_str(&script);
        out.push_str(&html[idx..]);
        out
    } else {
        let mut out = html.to_string();
        out.push_str(&script);
        out
    }
}

fn write_response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let header = format!(
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        status,
        content_type,
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::inject_live_reload;

    #[test]
    fn injects_before_closing_body_tag() {
        let html = "<html><body><p>hi</p></BODY></html>";
        let out = inject_live_reload(html);
        assert!(out.contains("<script>"));
        assert!(out.find("<script>").unwrap() < out.find("</BODY>").unwrap());
    }

    #[test]
    fn appends_when_no_body_tag() {
        let html = "<html><p>hi</p></html>";
        let out = inject_live_reload(html);
        assert!(out.starts_with(html));
        assert!(out.contains("<script>"));
    }
}
