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
use std::sync::Arc;
use std::thread;
use std::time::Duration;

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
    if !rebuild_web(file, mode, verbose, false) {
        exit(ExitCodes::USER_ERROR);
    }

    let version = Arc::new(AtomicU64::new(1));
    let listener = bind_dev_server(port);
    let port = listener
        .local_addr()
        .map(|a| a.port())
        .unwrap_or(*PORT_RANGE.start());

    {
        let version = Arc::clone(&version);
        let canvas_file = file.to_string();
        thread::spawn(move || serve_forever(listener, version, canvas_file));
    }

    println!(
        "serving http://localhost:{} — watching {} … (Ctrl-C to stop)",
        port, file
    );
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
            if rebuild_web(file, mode, verbose, true) {
                version.fetch_add(1, Ordering::SeqCst);
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
fn rebuild_web(file: &str, mode: OutputMode, verbose: bool, is_rebuild: bool) -> bool {
    let src = fs::read_to_string(file).unwrap_or_default();
    let out = match jet::compile_web(file) {
        Ok(out) => out,
        Err(diags) => {
            if is_rebuild {
                eprintln!("\n— {} changed —", file);
            }
            report_problems(mode, file, &src, &diags);
            return false;
        }
    };
    let web = match &out.web {
        Some(w) => w,
        None => {
            eprintln!("error: internal compiler error: missing web codegen output");
            return false;
        }
    };

    let staging = PathBuf::from("build").join(".jet-dev-staging");
    if let Err(msg) = write_web_artifacts(file, web, verbose, &staging) {
        eprintln!("{}", msg);
        return false;
    }
    if let Err(e) = stage_and_swap(&staging, Path::new("build")) {
        eprintln!("error: couldn't finalize web build: {}", e);
        return false;
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
fn serve_forever(listener: TcpListener, version: Arc<AtomicU64>, canvas_file: String) {
    for stream in listener.incoming() {
        if let Ok(stream) = stream {
            let version = Arc::clone(&version);
            let canvas_file = canvas_file.clone();
            thread::spawn(move || {
                let _ = handle_connection(stream, &version, &canvas_file);
            });
        }
    }
}

/// Parse one GET request line + headers (no keep-alive, no request body —
/// this is a dev tool, not a production server) and serve a response.
fn handle_connection(
    stream: TcpStream,
    version: &AtomicU64,
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
    if path == "/__jet_canvas/source-control"
        || path == "/canvas/source-control"
        || path == "/panel/source-control"
    {
        if method != "GET" {
            return method_not_allowed(&mut stream);
        }
        let body = jet::Canvas::source_control_json_for_file(Path::new(canvas_file));
        return write_response(
            &mut stream,
            "200 OK",
            "application/json; charset=utf-8",
            body.as_bytes(),
        );
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
                version.fetch_add(1, Ordering::SeqCst);
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
        return match jet::Canvas::query_json_for_file(Path::new(canvas_file), &request) {
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
        let body = version.load(Ordering::SeqCst).to_string();
        return write_response(
            &mut stream,
            "200 OK",
            "text/plain; charset=utf-8",
            body.as_bytes(),
        );
    }
    serve_static(&mut stream, path)
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
  setInterval(function () {{
    fetch("/__jet_dev_version", {{ cache: "no-store" }})
      .then(function (r) {{ return r.text(); }})
      .then(function (v) {{
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
