// c-devserver (owner-directed 2026-07-01): `core.web.devserver` — a `.jet` file's
// own `jet dev` behavior as ordinary Jet code, via a real configurable value
// (the same shape as `core.ui.null_backend()`, see Prelude/Ui.rs), instead of
// `--port=<N>`/`--target=web` CLI flags. Std-only (I6): `TcpListener`, manual
// HTTP/1.1 parsing, mtime-poll watch, `std::process::Command` to shell out to
// the real `jet` binary for each rebuild. This file is compiled INTO the
// standalone native binary produced for `fn dev()` — it never links against
// jet-parser/jet-sema/jet-codegen/jet-driver, so a rebuild always goes through
// `jet build --target=web <file>` as a subprocess, mirroring the design of
// Source/CmdDevWeb.rs's `run_dev_web` (mtime-poll watch, atomic staged
// rebuild, polling live-reload) without literally calling its internal
// functions.
//
// Everything lives inside a private module (matching Prelude/CoreLib.rs's own
// `mod jet_std { ... }` convention) because this fragment is concatenated
// into ONE Rust module with every other prelude fragment — top-level `use`
// here would collide with e.g. Prelude/Scheduler.rs's own
// `TcpStream`/`Ordering`/`Arc`/`Duration` imports.
mod jet_devserver_impl {
    use std::io::{BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    /// Ports tried, in order, before giving up when no explicit `.port(n)`
    /// was set — mirrors `Source/CmdDevWeb.rs`'s `PORT_RANGE`.
    const JET_DEVSERVER_PORT_RANGE: std::ops::RangeInclusive<u16> = 8080..=8089;

    /// Live-reload poll interval — mirrors `Source/CmdDevWeb.rs`'s
    /// `LIVE_RELOAD_POLL_MS`.
    const JET_DEVSERVER_LIVE_RELOAD_POLL_MS: u64 = 400;

    struct JetDevServerState {
        app_file: String,
        html_override: Option<String>,
        port: Option<u16>,
    }

    /// The value `core.web.devserver.for_app(...)` returns. Cheap to clone (an
    /// `Rc` handle around shared state) so every builder method can take
    /// `&self` and still hand back a `DevServer` for chaining, exactly like
    /// `core.ui.null_backend()`'s `JetNullBackend` handle.
    #[derive(Clone)]
    pub struct JetDevServer {
        state: std::rc::Rc<std::cell::RefCell<JetDevServerState>>,
    }

    pub fn jet_devserver_for_app(file: &str) -> JetDevServer {
        // Relative or absolute both work: canonicalize against the process's
        // working directory (inherited from wherever `jet dev` was invoked)
        // up front, so every later use — the watch loop, `.html()` sibling
        // resolution, the rebuild subprocess — sees one absolute path and
        // is immune to any cwd changes after this point. A path that doesn't
        // resolve is kept verbatim; `serve()` reports it with the original
        // spelling the user wrote.
        let resolved = std::fs::canonicalize(file)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| file.to_string());
        JetDevServer {
            state: std::rc::Rc::new(std::cell::RefCell::new(JetDevServerState {
                app_file: resolved,
                html_override: None,
                port: None,
            })),
        }
    }

    /// `devserver.app()` — the file `jet dev` launched, no path spelled out.
    /// `jet dev <file>` passes the canonical absolute path of `<file>` to
    /// this program in the `JET_DEV_FILE` environment variable
    /// (Source/CmdCompile.rs `run_dev_entry`); since the watched file and
    /// the file defining `fn dev()` are almost always the same file, this is
    /// the form to reach for first — `for_app(path)` is for the rarer case
    /// of watching a *different* entry file than the one that owns `dev()`.
    pub fn jet_devserver_app() -> JetDevServer {
        match std::env::var("JET_DEV_FILE") {
            Ok(file) if !file.is_empty() => jet_devserver_for_app(&file),
            _ => {
                eprintln!("error: `devserver.app()` only works under `jet dev`");
                eprintln!(
                    " why: it watches \"the file jet dev launched\", which the `jet dev` command passes in as JET_DEV_FILE — running this binary directly leaves that unset"
                );
                eprintln!(
                    " fix: run `jet dev <file.jet>`, or name the file explicitly with `devserver.for_app(\"path/to/app.jet\")`"
                );
                std::process::exit(1);
            }
        }
    }

    impl JetDevServer {
        /// Set the companion HTML page — takes priority over the `.jet`
        /// file's own `#Html(...)` marker / `<stem>.html` sibling convention
        /// (both still apply, inside the `jet build --target=web`
        /// subprocess, when `.html` was never called).
        pub fn html(&self, path: String) -> JetDevServer {
            self.state.borrow_mut().html_override = Some(path);
            self.clone()
        }

        /// Set the port to bind. Out-of-range values fail loud immediately
        /// (same validation as the CLI's own `--port=<N>`), rather than
        /// silently wrapping into a bogus port.
        pub fn port(&self, n: i64) -> JetDevServer {
            if !(1..=65535).contains(&n) {
                eprintln!("error: `.port({})` isn't a valid port number", n);
                eprintln!(" fix: use a number from 1 to 65535");
                std::process::exit(1);
            }
            self.state.borrow_mut().port = Some(n as u16);
            self.clone()
        }

        /// Build once, serve `build/` with live-reload, then watch the app
        /// file and rebuild on every save. Blocks forever (Ctrl-C stops it)
        /// — the same contract as `jet dev <file> --target=web`.
        pub fn serve(&self) {
            let (app_file, html_override, port_pref) = {
                let s = self.state.borrow();
                (s.app_file.clone(), s.html_override.clone(), s.port)
            };

            if !Path::new(&app_file).exists() {
                eprintln!("error: can't find the file `{}`", app_file);
                eprintln!(" fix: check the path passed to `devserver.for_app(...)`");
                std::process::exit(1);
            }

            if !jet_devserver_rebuild(&app_file, html_override.as_deref(), false) {
                std::process::exit(1);
            }

            let version = Arc::new(AtomicU64::new(1));
            let listener = jet_devserver_bind(port_pref);
            let bound_port = listener
                .local_addr()
                .map(|a| a.port())
                .unwrap_or(*JET_DEVSERVER_PORT_RANGE.start());

            {
                let version = Arc::clone(&version);
                thread::spawn(move || jet_devserver_serve_forever(listener, version));
            }

            println!(
                "serving http://localhost:{} — watching {} … (Ctrl-C to stop)",
                bound_port, app_file
            );

            let mut last_mtime = jet_devserver_mtime(Path::new(&app_file));
            loop {
                thread::sleep(Duration::from_millis(120));
                let now = jet_devserver_mtime(Path::new(&app_file));
                if now != last_mtime {
                    last_mtime = now;
                    thread::sleep(Duration::from_millis(30));
                    if jet_devserver_rebuild(&app_file, html_override.as_deref(), true) {
                        version.fetch_add(1, Ordering::SeqCst);
                    }
                }
            }
        }
    }

    fn jet_devserver_mtime(path: &Path) -> Option<std::time::SystemTime> {
        std::fs::metadata(path).and_then(|m| m.modified()).ok()
    }

    /// Bind the dev server's `TcpListener`. `Some(port)` binds that exact
    /// port and fails loud if it's taken; `None` scans
    /// `JET_DEVSERVER_PORT_RANGE`.
    fn jet_devserver_bind(port: Option<u16>) -> TcpListener {
        if let Some(port) = port {
            return match TcpListener::bind(("127.0.0.1", port)) {
                Ok(listener) => listener,
                Err(e) => {
                    eprintln!("error: couldn't bind to port {}: {}", port, e);
                    if e.kind() == std::io::ErrorKind::AddrInUse {
                        eprintln!(" fix: stop whatever's using port {}, or pick another with `.port(n)`", port);
                    }
                    std::process::exit(1);
                }
            };
        }
        for port in JET_DEVSERVER_PORT_RANGE {
            match TcpListener::bind(("127.0.0.1", port)) {
                Ok(listener) => return listener,
                Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => continue,
                Err(e) => {
                    eprintln!("error: couldn't start the dev server: {}", e);
                    std::process::exit(1);
                }
            }
        }
        eprintln!(
            "error: every port from {} to {} is already in use",
            JET_DEVSERVER_PORT_RANGE.start(),
            JET_DEVSERVER_PORT_RANGE.end()
        );
        eprintln!(" fix: free one of those ports, or pick one explicitly with `.port(n)`");
        std::process::exit(1);
    }

    /// Compile `app_file` for the web target by shelling out to the real
    /// `jet` binary, and on success atomically replace `build/*` with the
    /// new output.
    ///
    /// True atomicity without calling any internal compiler function: `jet
    /// build --target=web` is run with its OWN working directory pointed at
    /// a fresh staging root (`build/.jet-devserver-staging/`), passing
    /// `app_file` as an absolute path so it resolves the same regardless of
    /// that working directory. The subprocess therefore writes into
    /// `<staging>/build/*` — untouched by any previous good build — and only
    /// once that whole set (rustc-for-wasm included) has succeeded do we
    /// rename every file into the real `build/`, exactly the same "staged,
    /// then swapped" shape as `Source/CmdDevWeb.rs`'s
    /// `rebuild_web`/`stage_and_swap`.
    fn jet_devserver_rebuild(app_file: &str, html_override: Option<&str>, is_rebuild: bool) -> bool {
        let abs_file = match std::fs::canonicalize(app_file) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("error: couldn't resolve `{}`: {}", app_file, e);
                return false;
            }
        };
        let staging_root = PathBuf::from("build").join(".jet-devserver-staging");
        let _ = std::fs::remove_dir_all(&staging_root);
        if let Err(e) = std::fs::create_dir_all(&staging_root) {
            eprintln!("error: couldn't create a staging folder for the rebuild: {}", e);
            return false;
        }

        // Prefer the exact `jet` binary that launched this program (passed
        // in as JET_BIN by `jet dev`'s `run_dev_entry`) over a bare PATH
        // lookup: PATH could resolve to a different jet version, or to a
        // cwd-sensitive wrapper that breaks under the staging cwd below.
        let jet_bin = std::env::var("JET_BIN").unwrap_or_else(|_| "jet".to_string());
        let out = Command::new(&jet_bin)
            .args(["build", "--target=web"])
            .arg(&abs_file)
            .current_dir(&staging_root)
            .output();
        let out = match out {
            Ok(o) => o,
            Err(e) => {
                eprintln!("error: couldn't run `{} build --target=web`: {}", jet_bin, e);
                eprintln!(" fix: make sure `jet` is on PATH, or run this program via `jet dev` (which passes its own path in JET_BIN)");
                return false;
            }
        };
        if !out.status.success() {
            if is_rebuild {
                eprintln!("\n— {} changed —", app_file);
            }
            // `jet build` is the real front end — it already rendered its
            // own diagnostics (I2: an ICE banner, or a normal front-end
            // error). Relay them verbatim rather than re-interpreting them
            // here.
            let _ = std::io::stderr().write_all(&out.stdout);
            let _ = std::io::stderr().write_all(&out.stderr);
            let _ = std::fs::remove_dir_all(&staging_root);
            return false;
        }

        let staged_build = staging_root.join("build");
        if let Some(html_path) = html_override {
            let source_dir = Path::new(app_file).parent().unwrap_or(Path::new("."));
            let explicit = source_dir.join(html_path);
            match std::fs::read(&explicit) {
                Ok(bytes) => {
                    if let Err(e) = std::fs::write(staged_build.join("index.html"), bytes) {
                        eprintln!("error: couldn't write staged index.html: {}", e);
                        let _ = std::fs::remove_dir_all(&staging_root);
                        return false;
                    }
                }
                Err(e) => {
                    eprintln!(
                        "error: `.html(\"{}\")` names a file that doesn't exist: {} ({})",
                        html_path,
                        explicit.display(),
                        e
                    );
                    let _ = std::fs::remove_dir_all(&staging_root);
                    return false;
                }
            }
        }

        const FILES: [&str; 6] = [
            "web.manifest.json",
            "jet_dom_runtime.js",
            "app.js",
            "app_wasm.rs",
            "app.wasm",
            "index.html",
        ];
        let out_dir = Path::new("build");
        if let Err(e) = std::fs::create_dir_all(out_dir) {
            eprintln!("error: couldn't create the build/ folder: {}", e);
            let _ = std::fs::remove_dir_all(&staging_root);
            return false;
        }
        for name in FILES {
            let src = staged_build.join(name);
            let dst = out_dir.join(name);
            if let Err(e) = jet_devserver_rename_with_retry(&src, &dst) {
                eprintln!("error: couldn't finalize web build ({}): {}", name, e);
                let _ = std::fs::remove_dir_all(&staging_root);
                return false;
            }
        }
        let _ = std::fs::remove_dir_all(&staging_root);

        if is_rebuild {
            eprintln!("[dev] rebuilt after change to {}", app_file);
        }
        true
    }

    fn jet_devserver_rename_with_retry(src: &Path, dst: &Path) -> std::io::Result<()> {
        let mut last_err = None;
        for attempt in 0..5 {
            if attempt > 0 {
                thread::sleep(Duration::from_millis(20 * attempt as u64));
            }
            match std::fs::rename(src, dst) {
                Ok(()) => return Ok(()),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.unwrap())
    }

    fn jet_devserver_serve_forever(listener: TcpListener, version: Arc<AtomicU64>) {
        for stream in listener.incoming() {
            if let Ok(stream) = stream {
                let version = Arc::clone(&version);
                thread::spawn(move || {
                    let _ = jet_devserver_handle_connection(stream, &version);
                });
            }
        }
    }

    fn jet_devserver_handle_connection(stream: TcpStream, version: &AtomicU64) -> std::io::Result<()> {
        let mut reader = BufReader::new(stream.try_clone()?);
        let mut request_line = String::new();
        if reader.read_line(&mut request_line)? == 0 {
            return Ok(());
        }
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line)? == 0 || line == "\r\n" || line == "\n" {
                break;
            }
        }

        let mut stream = stream;
        let mut parts = request_line.split_whitespace();
        let method = parts.next().unwrap_or("");
        let target = parts.next().unwrap_or("/");

        if method != "GET" {
            return jet_devserver_write_response(
                &mut stream,
                "405 Method Not Allowed",
                "text/plain; charset=utf-8",
                b"jet dev's web server only handles GET",
            );
        }

        let path = target.split('?').next().unwrap_or("/");
        if path == "/__jet_dev_version" {
            let body = version.load(Ordering::SeqCst).to_string();
            return jet_devserver_write_response(
                &mut stream,
                "200 OK",
                "text/plain; charset=utf-8",
                body.as_bytes(),
            );
        }
        jet_devserver_serve_static(&mut stream, path)
    }

    fn jet_devserver_serve_static(stream: &mut TcpStream, path: &str) -> std::io::Result<()> {
        if path.contains("..") {
            return jet_devserver_write_response(
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
        let bytes = match std::fs::read(&file_path) {
            Ok(b) => b,
            Err(_) => {
                let body = format!("not found: {}", path);
                return jet_devserver_write_response(
                    stream,
                    "404 Not Found",
                    "text/plain; charset=utf-8",
                    body.as_bytes(),
                );
            }
        };

        let content_type = jet_devserver_content_type_for(&file_path);
        if file_path.file_name().and_then(|f| f.to_str()) == Some("index.html") {
            let html = String::from_utf8_lossy(&bytes).into_owned();
            let injected = jet_devserver_inject_live_reload(&html);
            return jet_devserver_write_response(stream, "200 OK", content_type, injected.as_bytes());
        }
        jet_devserver_write_response(stream, "200 OK", content_type, &bytes)
    }

    fn jet_devserver_content_type_for(path: &Path) -> &'static str {
        match path.extension().and_then(|e| e.to_str()) {
            Some("html") => "text/html; charset=utf-8",
            Some("js") => "application/javascript; charset=utf-8",
            Some("wasm") => "application/wasm",
            Some("json") => "application/json; charset=utf-8",
            Some("css") => "text/css; charset=utf-8",
            _ => "application/octet-stream",
        }
    }

    fn jet_devserver_live_reload_script() -> String {
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
            poll_ms = JET_DEVSERVER_LIVE_RELOAD_POLL_MS
        )
    }

    fn jet_devserver_inject_live_reload(html: &str) -> String {
        let script = jet_devserver_live_reload_script();
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

    fn jet_devserver_write_response(
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
}

pub use jet_devserver_impl::{jet_devserver_app, jet_devserver_for_app, JetDevServer};
