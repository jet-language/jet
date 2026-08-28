//! Web dev-server state, terminal/browser parity UI, routes, live reload, and
//! last-good artifact swapping — std-only (I6: no `notify`, HTTP-server, or
//! WebSocket crate).
//!
//! This is a completely different execution model from the native `jet dev`
//! native `jet dev`, which interprets/hot-swaps the program in
//! process. Compiling to JS/WASM has nothing to hot-swap in that sense — the
//! only thing a save can do is trigger a full recompile, so this module only
//! reuses the *mtime-poll watch pattern* (and the `file_mtime` helper itself)
//! from `run_dev`, not any of its interpreter/JIT machinery.

use std::collections::HashMap;
use std::fs;
use std::io::{BufReader, IsTerminal, Write};
use std::net::{IpAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use jet_driver::Diagnostics::ColorChoice;
use jet_foundation::JSON::json_escape;

use crate::{
    content_type_for, query_param, static_path, write_response, MAX_REQUEST_BODY_BYTES, Request,
};

/// Application preview ports tried, in order, before giving up. 8080 is the
/// conventional static-dev-server port; a small bounded scan upward covers
/// "something else is already on 8080" without hunting indefinitely.
const APPLICATION_PORT_RANGE: std::ops::RangeInclusive<u16> = 8080..=8089;
const CANVAS_SESSION_BYTES: usize = 32;
const MAX_CONNECTION_THREADS: usize = 64;

/// How often the live-reload script in the browser polls `/__jet_dev_status`.
/// The watcher and in-process rebuild already fit inside the warm budget; this
/// short poll closes the remaining edit-to-visible gap without a refresh.
const LIVE_RELOAD_POLL_MS: u64 = 40;
const CLIENT_TTL_MS: u64 = LIVE_RELOAD_POLL_MS * 4;

/// D-FE-DEVSRV1 outcome D (hybrid, owner-modified 2026-07-08: pinned parity
/// header required as the terminal anchor): a one-line terminal status and a
/// browser corner strip mirror the exact same words from one shared status
/// (`header_words`/`json`'s `message` field) — they cannot drift because both
/// read the same source. On error both sides show the identical verbatim
/// diagnostic (I4): terminal frames it in a box (TTY) or prints it plain (CI
/// floor); the browser expands its strip into a full overlay. `--verbose`
/// (or `-v`) adds a request/rebuild log under the still-pinned header.
#[derive(Clone)]
struct DevStatusSnapshot {
    state: String,
    code: String,
    diagnostic: String,
    last_build_ms: u128,
}

struct DevStatus {
    version: AtomicU64,
    clients: Mutex<HashMap<String, Instant>>,
    state: Mutex<DevStatusSnapshot>,
    browser_relay: Mutex<Option<crate::BrowserTrace::Relay>>,
    browser_trace_enabled: AtomicBool,
    command_receipt: Mutex<Option<String>>,
    /// The file `jet dev` is watching — used in the `building` parity line
    /// and the `save <file> → …` verbose log lines.
    watched_file: String,
    /// Set once, right after the Canvas listener binds — 0 until then.
    port: AtomicU64,
    /// Static generator previews use the application listener only and must
    /// not advertise a Canvas URL that is not bound.
    canvas_enabled: AtomicBool,
    /// `--verbose`/`-v`: opt-in request/rebuild log under the parity header
    /// (dashboard's depth, D-FE-DEVSRV1=D "on-demand depth").
    verbose: AtomicBool,
    active: AtomicBool,
    exit_code: AtomicU64,
    /// Set only after raw-mode input succeeds. Verbose DECSTBM pinning must
    /// not start before its Ctrl-C/EOF cleanup guard exists.
    controls_ready: AtomicBool,
    /// Losing every leased browser after at least one was connected is a
    /// shared reconnect state until the next lease renewal.
    reconnecting: AtomicBool,
    /// Color and cursor control are separate capabilities. `NO_COLOR`
    /// changes the dot into a bracketed state word, but a real TTY still
    /// gets the pinned dashboard and live `v` control.
    color: bool,
    /// Redraw the parity line in place instead of appending a new one each
    /// time. This depends only on stderr being a real TTY; color is cosmetic.
    /// Non-TTY pipes use the plain append-only CI floor.
    pin: bool,
    /// Serializes every write to stderr from the status/log renderer so
    /// concurrent request threads and the rebuild-watch thread never
    /// interleave partial ANSI escape sequences.
    term_lock: Mutex<()>,
    /// How many lines the last pinned (non-verbose) redraw occupied — lets
    /// the next redraw move the cursor up and erase exactly that block
    /// (parity line, or parity line + framed diagnostic).
    last_block_lines: Mutex<usize>,
    /// Whether the verbose pinned header's scroll region has been set up yet
    /// (`\x1b[3r`, cursor parked in the scrolling log area below two pinned
    /// dashboard rows).
    header_started: Mutex<bool>,
}

impl DevStatus {
    fn new(file: &str, verbose: bool) -> DevStatus {
        let is_tty = std::io::stderr().is_terminal();
        // `NO_COLOR`/`FORCE_COLOR` resolution — the same one every other jet
        // command uses. Pinning (cursor redraw) additionally requires a real
        // TTY: `FORCE_COLOR` off-TTY still can't redraw in place.
        let color = ColorChoice::Auto.resolve(is_tty);
        DevStatus::new_with_terminal(file, verbose, is_tty, color)
    }

    fn new_with_terminal(file: &str, verbose: bool, is_tty: bool, color: bool) -> DevStatus {
        DevStatus {
            version: AtomicU64::new(1),
            clients: Mutex::new(HashMap::new()),
            state: Mutex::new(DevStatusSnapshot {
                state: "ready".to_string(),
                code: String::new(),
                diagnostic: String::new(),
                last_build_ms: 0,
            }),
            browser_relay: Mutex::new(None),
            browser_trace_enabled: AtomicBool::new(false),
            command_receipt: Mutex::new(None),
            watched_file: file.to_string(),
            port: AtomicU64::new(0),
            canvas_enabled: AtomicBool::new(true),
            verbose: AtomicBool::new(verbose),
            active: AtomicBool::new(false),
            exit_code: AtomicU64::new(0),
            controls_ready: AtomicBool::new(false),
            reconnecting: AtomicBool::new(false),
            color,
            pin: is_tty,
            term_lock: Mutex::new(()),
            last_block_lines: Mutex::new(0),
            header_started: Mutex::new(false),
        }
    }

    fn set_port(&self, port: u16) {
        self.port.store(port as u64, Ordering::SeqCst);
    }

    fn set_canvas_enabled(&self, enabled: bool) {
        self.canvas_enabled.store(enabled, Ordering::SeqCst);
    }

    fn port(&self) -> u16 {
        self.port.load(Ordering::SeqCst) as u16
    }

    fn header_text_for(&self, snap: &DevStatusSnapshot) -> (String, String) {
        if self.reconnecting.load(Ordering::SeqCst) {
            return (
                "reconnecting".to_string(),
                "waiting for connection".to_string(),
            );
        }
        header_words(
            &snap.state,
            &self.watched_file,
            &snap.code,
            self.port(),
            self.client_count(),
            snap.last_build_ms,
        )
    }

    fn format_line(&self, word: &str, rest: &str) -> String {
        if self.color {
            format_line_colored(word, rest)
        } else {
            format_line_plain(word, rest)
        }
    }

    fn dashboard_detail_line(&self) -> String {
        if self.canvas_enabled.load(Ordering::SeqCst) {
            format!(
                "         watching {} · Canvas http://localhost:{}/canvas · v verbose",
                self.watched_file,
                self.port()
            )
        } else {
            format!("         watching {} · v verbose", self.watched_file)
        }
    }

    /// Recompute the parity words from current state and redraw the
    /// terminal's live region (single source shared with `json()`'s
    /// `message` field — the browser strip renders that exact string).
    fn refresh(&self) {
        if !self.active.load(Ordering::SeqCst) {
            return;
        }
        let snap = self.state.lock().unwrap().clone();
        let (word, rest) = self.header_text_for(&snap);
        let line = self.format_line(&word, &rest);

        if self.verbose() {
            if self.pin {
                self.write_header_verbose(&line);
            } else {
                let _g = self.term_lock.lock().unwrap();
                let mut out = std::io::stderr();
                let _ = writeln!(out, "{}", line);
                let _ = out.flush();
            }
            return;
        }

        if self.pin {
            let mut lines = vec![line, self.dashboard_detail_line()];
            if snap.state == "error" {
                lines.extend(frame_lines(&snap.code, &snap.diagnostic));
            }
            self.write_block(&lines);
            return;
        }

        // CI floor: append-only, plain, no framing — the diagnostic prints
        // byte-for-byte what `render_diagnostics` produced (I4).
        let _g = self.term_lock.lock().unwrap();
        let mut out = std::io::stderr();
        let _ = writeln!(out, "{}", line);
        if snap.state == "error" {
            let _ = writeln!(out, "{}", snap.diagnostic);
        }
        let _ = out.flush();
    }

    /// Redraw a pinned (non-verbose) block in place: move the cursor up to
    /// the top of the previous block, erase to end of screen, print the new
    /// block. Cursor ends on the block's last line, no trailing newline —
    /// every caller (including the next redraw) relies on that invariant.
    fn write_block(&self, lines: &[String]) {
        let _g = self.term_lock.lock().unwrap();
        let mut out = std::io::stderr();
        let mut prev = self.last_block_lines.lock().unwrap();
        if *prev > 0 {
            if *prev > 1 {
                let _ = write!(out, "\x1b[{}A", *prev - 1);
            }
            let _ = write!(out, "\r\x1b[0J");
        }
        let _ = write!(out, "{}", lines.join("\n"));
        let _ = out.flush();
        *prev = lines.len();
    }

    /// Pin the two-row dashboard to terminal rows 1–2 via a DECSTBM scroll
    /// region (`\x1b[3r`) so `log_line` can print request/rebuild lines below
    /// it forever without touching the shared parity row or watched-target
    /// row. Std ANSI only (I6) — no terminal-
    /// size query, which is why the region's bottom is left at the display
    /// default instead of being computed.
    fn write_header_verbose(&self, line: &str) {
        let _g = self.term_lock.lock().unwrap();
        let mut out = std::io::stderr();
        // Ability check belongs inside the terminal lock. A refresh that
        // waited behind EOF/Ctrl-C must not reinstall DECSTBM after cleanup.
        if !self.controls_ready.load(Ordering::SeqCst) {
            let _ = writeln!(out, "{}", line);
            let _ = out.flush();
            return;
        }
        let mut started = self.header_started.lock().unwrap();
        let detail = self.dashboard_detail_line();
        if !*started {
            let _ = write!(out, "\x1b[2J\x1b[H{}\n{}\n\x1b[3r\x1b[3;1H", line, detail);
            *started = true;
        } else {
            // DECSC/DECRC save+restore the log cursor across the jump to row 1.
            let _ = write!(out, "\x1b7\x1b[1;1H\x1b[2K{}\n\x1b[2K{}\x1b8", line, detail);
        }
        let _ = out.flush();
    }

    /// One line in the verbose request/rebuild log, printed under the pinned
    /// header. No-op unless `--verbose`/`-v` was passed.
    fn log_line(&self, text: &str) {
        if !self.active.load(Ordering::SeqCst) || !self.verbose() {
            return;
        }
        let _g = self.term_lock.lock().unwrap();
        let mut out = std::io::stderr();
        if self.pin && self.controls_ready.load(Ordering::SeqCst) {
            let _ = write!(out, "{}\r\n", text);
        } else {
            let _ = writeln!(out, "{}", text);
        }
        let _ = out.flush();
    }

    fn log_rebuild(&self, ok: bool, detail: &str) {
        let arrow = if ok { "rebuilt" } else { "error" };
        self.log_line(&format!(
            "{}  save {}  →  {} {}",
            clock_time(),
            self.watched_file,
            arrow,
            detail
        ));
    }

    fn log_request(&self, method: &str, path: &str, code: u16, elapsed: Duration) {
        self.log_line(&format!(
            "{}  {}  {:<20} {}  {}ms",
            clock_time(),
            method,
            path,
            code,
            elapsed.as_millis()
        ));
    }

    fn log_diagnostic(&self, code: &str, diagnostic: &str) {
        if !self.active.load(Ordering::SeqCst) || !self.verbose() {
            return;
        }
        let _g = self.term_lock.lock().unwrap();
        let mut out = std::io::stderr();
        if self.pin && self.controls_ready.load(Ordering::SeqCst) {
            for line in frame_lines(code, diagnostic) {
                let _ = write!(out, "{}\r\n", line);
            }
        } else {
            let _ = writeln!(out, "{}", diagnostic);
        }
        let _ = out.flush();
    }

    fn verbose(&self) -> bool {
        self.verbose.load(Ordering::SeqCst)
    }

    /// Toggle terminal depth without changing shared parity state. Transition
    /// between redraw strategies explicitly so stale pinned blocks/scroll
    /// regions never survive a `v` keypress.
    fn toggle_verbose(&self) {
        let verbose = !self.verbose.fetch_xor(true, Ordering::SeqCst);
        if self.pin {
            let _g = self.term_lock.lock().unwrap();
            let mut out = std::io::stderr();
            if verbose {
                let mut prev = self.last_block_lines.lock().unwrap();
                if *prev > 1 {
                    let _ = write!(out, "\x1b[{}A", *prev - 1);
                }
                let _ = write!(out, "\r\x1b[0J");
                *prev = 0;
            } else {
                let _ = write!(out, "\x1b[r\x1b[2J\x1b[H");
                *self.header_started.lock().unwrap() = false;
            }
            let _ = out.flush();
        }
        self.refresh();
        self.log_line(if verbose {
            "verbose request/rebuild log enabled · press v to collapse"
        } else {
            ""
        });
    }

    fn disable_terminal_controls(&self) {
        if !self.pin {
            self.controls_ready.store(false, Ordering::SeqCst);
            return;
        }
        let _g = self.term_lock.lock().unwrap();
        // Disable and restore in one critical section. Any renderer already
        // waiting for this lock observes false before it can write.
        self.controls_ready.store(false, Ordering::SeqCst);
        let mut out = std::io::stderr();
        let mut started = self.header_started.lock().unwrap();
        if *started {
            let _ = write!(out, "\x1b[r\x1b[999;1H\r\n");
        } else {
            let _ = writeln!(out);
        }
        *started = false;
        let _ = out.flush();
    }

    fn mark_building(&self) {
        self.browser_relay.lock().unwrap().take();
        let mut state = self.state.lock().unwrap();
        let last_build_ms = state.last_build_ms;
        *state = DevStatusSnapshot {
            state: "building".to_string(),
            code: String::new(),
            diagnostic: String::new(),
            last_build_ms,
        };
        drop(state);
        self.refresh();
    }

    fn mark_ready(&self, elapsed_ms: u128, is_rebuild: bool) {
        let mut browser_relay = self.browser_relay.lock().unwrap();
        if self.browser_trace_enabled.load(Ordering::SeqCst) {
            browser_relay.take();
            *browser_relay = fs::read_to_string("build/web.manifest.json")
                .ok()
                .and_then(|manifest| crate::BrowserTrace::Relay::new(&manifest).ok());
        }
        drop(browser_relay);
        if is_rebuild {
            self.version.fetch_add(1, Ordering::SeqCst);
        }
        *self.state.lock().unwrap() = DevStatusSnapshot {
            state: "ready".to_string(),
            code: String::new(),
            diagnostic: String::new(),
            last_build_ms: elapsed_ms,
        };
        self.refresh();
        if is_rebuild {
            self.log_rebuild(true, &format_build_time(elapsed_ms));
        }
    }

    fn mark_error(&self, code: String, diagnostic: String, is_rebuild: bool) {
        let diagnostic_for_log = diagnostic.clone();
        let mut state = self.state.lock().unwrap();
        let last_build_ms = state.last_build_ms;
        *state = DevStatusSnapshot {
            state: "error".to_string(),
            code: code.clone(),
            diagnostic,
            last_build_ms,
        };
        drop(state);
        self.refresh();
        if is_rebuild {
            self.log_rebuild(false, &code);
            self.log_diagnostic(&code, &diagnostic_for_log);
        }
    }

    fn json(&self) -> String {
        let snap = self.state.lock().unwrap().clone();
        let (word, rest) = self.header_text_for(&snap);
        let message = format!("{} · {}", word, rest);
        format!(
            "{{\"version\":{},\"state\":\"{}\",\"message\":\"{}\",\"file\":\"{}\",\"code\":\"{}\",\"diagnostic\":\"{}\",\"clients\":{},\"last_build_ms\":{}}}",
            self.version.load(Ordering::SeqCst),
            json_escape(&word),
            json_escape(&message),
            json_escape(&self.watched_file),
            json_escape(&snap.code),
            json_escape(&snap.diagnostic),
            self.client_count(),
            snap.last_build_ms
        )
    }

    fn activate(&self) {
        self.active.store(true, Ordering::SeqCst);
        self.refresh();
    }

    fn client_count(&self) -> u64 {
        self.clients.lock().unwrap().len() as u64
    }

    fn note_client(&self, id: &str) {
        if id.is_empty() || id.len() > 128 {
            return;
        }
        let mut clients = self.clients.lock().unwrap();
        let changed = clients.insert(id.to_string(), Instant::now()).is_none();
        drop(clients);
        let recovered = self.reconnecting.swap(false, Ordering::SeqCst);
        if changed || recovered {
            self.refresh();
        }
    }

    fn expire_clients(&self) {
        let cutoff = Duration::from_millis(CLIENT_TTL_MS);
        let now = Instant::now();
        let mut clients = self.clients.lock().unwrap();
        let before = clients.len();
        clients.retain(|_, seen| now.saturating_duration_since(*seen) <= cutoff);
        let changed = clients.len() != before;
        let disconnected = before > 0 && clients.is_empty();
        drop(clients);
        if disconnected {
            self.reconnecting.store(true, Ordering::SeqCst);
        }
        if changed {
            self.refresh();
        }
    }

    fn drop_client(&self, id: &str) {
        let mut clients = self.clients.lock().unwrap();
        let changed = clients.remove(id).is_some();
        drop(clients);
        if changed {
            // A pagehide beacon is a clean close/reload, not a broken poll.
            self.reconnecting.store(false, Ordering::SeqCst);
            self.refresh();
        }
    }

    fn record_command_receipt(&self, receipt: String) {
        *self.command_receipt.lock().unwrap() = Some(receipt);
    }

    fn command_receipt(&self) -> Option<String> {
        self.command_receipt.lock().unwrap().clone()
    }

    fn activate_browser_trace(&self, manifest: &str) -> Result<(), String> {
        let relay = crate::BrowserTrace::Relay::new(manifest)?;
        *self.browser_relay.lock().unwrap() = Some(relay);
        self.browser_trace_enabled.store(true, Ordering::SeqCst);
        self.version.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn activate_requested_browser_trace(&self) {
        if !matches!(crate::BrowserTrace::take_request(), Ok(true)) {
            return;
        }
        if let Ok(manifest) = fs::read_to_string("build/web.manifest.json") {
            let _ = self.activate_browser_trace(&manifest);
        }
    }
}

#[derive(Clone, Debug)]
pub struct CanvasHostOptions {
    pub host: String,
    pub port: Option<u16>,
    pub transport: String,
    pub authority: String,
    pub audit: bool,
    pub output: Option<String>,
    pub target: Option<String>,
}

impl Default for CanvasHostOptions {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: None,
            transport: "http".to_string(),
            authority: "loopback".to_string(),
            audit: false,
            output: None,
            target: None,
        }
    }
}

pub struct WebHost {
    listener: Mutex<Option<TcpListener>>,
    application_listener: Mutex<Option<TcpListener>>,
    started: AtomicBool,
    shutdown: Arc<AtomicBool>,
    server_threads: Mutex<Vec<thread::JoinHandle<()>>>,
    poll_thread: Mutex<Option<thread::JoinHandle<()>>>,
    terminal_thread: Mutex<Option<thread::JoinHandle<()>>>,
    status: Arc<DevStatus>,
    debug_sessions: Arc<crate::Canvas::DebugSessions>,
    session: Arc<crate::ResidentDevSession>,
    canvas_file: String,
    canvas_only: bool,
    bind_host: String,
    session_secret: String,
    static_root: PathBuf,
    source_asset_fallback: bool,
}

impl WebHost {
    pub fn bind(file: &str, verbose: bool, port: Option<u16>) -> Result<Self, String> {
        let application_listener = bind_application_server(port)?;
        let application_port = application_listener
            .local_addr()
            .map(|address| address.port())
            .unwrap_or(0);
        let listener = bind_canvas_server("127.0.0.1", None)?;
        let bound_port = listener
            .local_addr()
            .map(|address| address.port())
            .unwrap_or(0);
        let status = Arc::new(DevStatus::new(file, verbose));
        status.set_port(bound_port);
        let session_secret = mint_session_secret()?;
        Ok(Self {
            listener: Mutex::new(Some(listener)),
            application_listener: Mutex::new(Some(application_listener)),
            started: AtomicBool::new(false),
            shutdown: Arc::new(AtomicBool::new(false)),
            server_threads: Mutex::new(Vec::new()),
            poll_thread: Mutex::new(None),
            terminal_thread: Mutex::new(None),
            status,
            debug_sessions: Arc::new(crate::Canvas::DebugSessions::default()),
            session: Arc::new(crate::ResidentDevSession::new(
                file,
                bound_port,
                application_port,
            )),
            canvas_file: file.to_string(),
            canvas_only: false,
            bind_host: "127.0.0.1".to_string(),
            session_secret,
            static_root: PathBuf::from("build"),
            source_asset_fallback: true,
        })
    }

    /// Bind the same application preview host for a generator's static output.
    /// The output root is discovered by `jet dev`; this constructor keeps the
    /// serving and live-reload mechanism shared with web development.
    pub fn bind_static(
        file: &str,
        root: &Path,
        verbose: bool,
        port: Option<u16>,
    ) -> Result<Self, String> {
        let static_root = fs::canonicalize(root).map_err(|error| {
            format!(
                "error: couldn't open static output `{}`: {}",
                root.display(),
                error
            )
        })?;
        let application_listener = bind_application_server(port)?;
        let application_port = application_listener
            .local_addr()
            .map(|address| address.port())
            .unwrap_or(0);
        let status = Arc::new(DevStatus::new(file, verbose));
        status.set_port(application_port);
        status.set_canvas_enabled(false);
        let session_secret = mint_session_secret()?;
        Ok(Self {
            listener: Mutex::new(None),
            application_listener: Mutex::new(Some(application_listener)),
            started: AtomicBool::new(false),
            shutdown: Arc::new(AtomicBool::new(false)),
            server_threads: Mutex::new(Vec::new()),
            poll_thread: Mutex::new(None),
            terminal_thread: Mutex::new(None),
            status,
            debug_sessions: Arc::new(crate::Canvas::DebugSessions::default()),
            session: Arc::new(crate::ResidentDevSession::new(
                file,
                0,
                application_port,
            )),
            canvas_file: file.to_string(),
            canvas_only: false,
            bind_host: "127.0.0.1".to_string(),
            session_secret,
            static_root,
            source_asset_fallback: false,
        })
    }

    /// Bind Canvas as a target-neutral control host. The default uses an OS
    /// selected loopback port and never starts the application preview
    /// listener; program target/output selection remains in the caller.
    pub fn bind_canvas(file: &str, verbose: bool, port: Option<u16>) -> Result<Self, String> {
        let mut options = CanvasHostOptions::default();
        options.port = port;
        Self::bind_canvas_with_options(file, verbose, &options)
    }

    /// Bind the Canvas control plane with a separate application listener.
    /// Web-targeted development uses this form so Canvas transport settings do
    /// not replace the program's own preview/output listener.
    pub fn bind_web_with_canvas_options(
        file: &str,
        verbose: bool,
        fallback_port: Option<u16>,
        options: &CanvasHostOptions,
    ) -> Result<Self, String> {
        let host = validate_canvas_options(options)?;
        let listener = bind_canvas_server(&host, options.port)?;
        let bound_port = listener
            .local_addr()
            .map(|address| address.port())
            .unwrap_or(0);
        let application_listener = bind_application_server(fallback_port)?;
        let application_port = application_listener
            .local_addr()
            .map(|address| address.port())
            .unwrap_or(0);
        let status = Arc::new(DevStatus::new(file, verbose || options.audit));
        status.set_port(bound_port);
        let session_secret = mint_session_secret()?;
        let session = Arc::new(crate::ResidentDevSession::new_with_canvas_host(
            file,
            &host,
            bound_port,
            application_port,
        ));
        session.select_output_values(options.output.as_deref(), options.target.as_deref());
        Ok(Self {
            listener: Mutex::new(Some(listener)),
            application_listener: Mutex::new(Some(application_listener)),
            started: AtomicBool::new(false),
            shutdown: Arc::new(AtomicBool::new(false)),
            server_threads: Mutex::new(Vec::new()),
            poll_thread: Mutex::new(None),
            terminal_thread: Mutex::new(None),
            status,
            debug_sessions: Arc::new(crate::Canvas::DebugSessions::default()),
            session,
            canvas_file: file.to_string(),
            canvas_only: false,
            bind_host: host,
            session_secret,
            static_root: PathBuf::from("build"),
            source_asset_fallback: true,
        })
    }

    pub fn bind_canvas_with_options(
        file: &str,
        verbose: bool,
        options: &CanvasHostOptions,
    ) -> Result<Self, String> {
        let host = validate_canvas_options(options)?;
        let listener = bind_canvas_server(&host, options.port)?;
        let bound_port = listener
            .local_addr()
            .map(|address| address.port())
            .unwrap_or(0);
        let status = Arc::new(DevStatus::new(file, verbose || options.audit));
        status.set_port(bound_port);
        let session_secret = mint_session_secret()?;
        let session = Arc::new(crate::ResidentDevSession::new_with_canvas_host(
            file, &host, bound_port, 0,
        ));
        session.select_output_values(options.output.as_deref(), options.target.as_deref());
        Ok(Self {
            listener: Mutex::new(Some(listener)),
            application_listener: Mutex::new(None),
            started: AtomicBool::new(false),
            shutdown: Arc::new(AtomicBool::new(false)),
            server_threads: Mutex::new(Vec::new()),
            poll_thread: Mutex::new(None),
            terminal_thread: Mutex::new(None),
            status,
            debug_sessions: Arc::new(crate::Canvas::DebugSessions::default()),
            session,
            canvas_file: file.to_string(),
            canvas_only: true,
            bind_host: host,
            session_secret,
            static_root: PathBuf::from("build"),
            source_asset_fallback: false,
        })
    }

    pub fn start(&self) {
        self.start_inner(true);
    }

    /// Start the HTTP/Canvas plane without taking terminal input. The native
    /// `jet dev --canvas` watcher owns its stdin and Ctrl-C lifecycle; both
    /// surfaces still share this one host and session.
    pub fn start_canvas(&self) {
        self.start_inner(false);
    }

    pub fn canvas_url(&self) -> String {
        format!(
            "http://{}:{}/canvas?session={}",
            url_host(&self.bind_host),
            self.status.port(),
            self.session_secret
        )
    }

    fn start_inner(&self, terminal_controls: bool) {
        if self.started.swap(true, Ordering::SeqCst) {
            return;
        }
        let has_canvas = self.listener.lock().unwrap().is_some();
        let listener = self.listener.lock().unwrap().take();
        if let Some(listener) = listener {
            let status = Arc::clone(&self.status);
            let debug_sessions = Arc::clone(&self.debug_sessions);
            let session = Arc::clone(&self.session);
            let canvas_file = self.canvas_file.clone();
            let canvas_only = self.canvas_only;
            let bind_host = self.bind_host.clone();
            let session_secret = self.session_secret.clone();
            let static_root = self.static_root.clone();
            let source_asset_fallback = self.source_asset_fallback;
            let shutdown = Arc::clone(&self.shutdown);
            let handle = thread::spawn(move || {
                serve_forever(
                    listener,
                    status,
                    debug_sessions,
                    session,
                    canvas_file,
                    ListenerKind::Canvas,
                    canvas_only,
                    bind_host,
                    session_secret,
                    shutdown,
                    static_root,
                    source_asset_fallback,
                )
            });
            self.server_threads.lock().unwrap().push(handle);
        }
        if let Some(listener) = self.application_listener.lock().unwrap().take() {
            let status = Arc::clone(&self.status);
            let debug_sessions = Arc::clone(&self.debug_sessions);
            let session = Arc::clone(&self.session);
            let canvas_file = self.canvas_file.clone();
            let static_root = self.static_root.clone();
            let source_asset_fallback = self.source_asset_fallback;
            let shutdown = Arc::clone(&self.shutdown);
            let handle = thread::spawn(move || {
                serve_forever(
                    listener,
                    status,
                    debug_sessions,
                    session,
                    canvas_file,
                    ListenerKind::Application,
                    false,
                    "127.0.0.1".to_string(),
                    String::new(),
                    shutdown,
                    static_root,
                    source_asset_fallback,
                )
            });
            self.server_threads.lock().unwrap().push(handle);
        }
        {
            let status = Arc::clone(&self.status);
            let shutdown = Arc::clone(&self.shutdown);
            let handle = thread::spawn(move || {
                while !wait_for_shutdown(&shutdown, Duration::from_millis(LIVE_RELOAD_POLL_MS)) {
                    status.activate_requested_browser_trace();
                    status.expire_clients();
                }
            });
            *self.poll_thread.lock().unwrap() = Some(handle);
        }
        if !self.canvas_only {
            println!(
                "App preview: http://localhost:{}/",
                self.session_application_port()
            );
        }
        if has_canvas {
            println!("Canvas: {}", self.canvas_url());
        }
        let _ = std::io::stdout().flush();
        if terminal_controls {
            if let Some(handle) =
                start_terminal_controls(Arc::clone(&self.status), Arc::clone(&self.shutdown))
            {
                *self.terminal_thread.lock().unwrap() = Some(handle);
            }
        }
        self.status.activate();
    }

    pub fn mark_building(&self) {
        self.status.mark_building();
        self.session.mark_building();
    }

    pub fn mark_ready(&self, elapsed_ms: u128, is_rebuild: bool) {
        self.status.mark_ready(elapsed_ms, is_rebuild);
        if let Ok(source) = fs::read_to_string(&self.canvas_file) {
            let revision = crate::Canvas::source_revision(&source);
            self.session.observe_source(&revision);
            self.session.mark_last_good(
                &revision,
                &format!("web-build-{}", self.status.version.load(Ordering::SeqCst)),
            );
        }
        self.session.mark_ready();
    }

    pub fn mark_error(&self, code: String, diagnostic: String, is_rebuild: bool) {
        self.status
            .mark_error(code.clone(), diagnostic.clone(), is_rebuild);
        if let Ok(source) = fs::read_to_string(&self.canvas_file) {
            self.session
                .observe_source(&crate::Canvas::source_revision(&source));
        }
        self.session.mark_error(&code, &diagnostic);
    }

    pub fn exit_code(&self) -> Option<i32> {
        let code = self.status.exit_code.load(Ordering::SeqCst);
        (code != 0).then_some(code as i32)
    }

    fn session_application_port(&self) -> u16 {
        self.session.application_port()
    }
}

impl Drop for WebHost {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        self.listener.lock().unwrap().take();
        self.application_listener.lock().unwrap().take();

        if let Some(handle) = self.terminal_thread.lock().unwrap().take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.poll_thread.lock().unwrap().take() {
            let _ = handle.join();
        }
        let handles = self
            .server_threads
            .lock()
            .unwrap()
            .drain(..)
            .collect::<Vec<_>>();
        for handle in handles {
            let _ = handle.join();
        }
        self.status.active.store(false, Ordering::SeqCst);
    }
}

fn wait_for_shutdown(shutdown: &AtomicBool, duration: Duration) -> bool {
    let deadline = Instant::now() + duration;
    while !shutdown.load(Ordering::SeqCst) {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        thread::sleep(remaining.min(Duration::from_millis(10)));
    }
    true
}

fn start_terminal_controls(
    status: Arc<DevStatus>,
    shutdown: Arc<AtomicBool>,
) -> Option<thread::JoinHandle<()>> {
    if !status.pin {
        return None;
    }
    let Some(raw) = jet_repl::Term::RawGuard::enable() else {
        return None;
    };
    status.controls_ready.store(true, Ordering::SeqCst);
    Some(thread::spawn(move || {
        let mut keys = jet_repl::Term::KeyReader::new(std::io::stdin());
        loop {
            if shutdown.load(Ordering::SeqCst) {
                status.disable_terminal_controls();
                drop(raw);
                return;
            }
            match keys.read_key() {
                jet_repl::Term::Key::Char('v' | 'V') => status.toggle_verbose(),
                jet_repl::Term::Key::Idle => {}
                jet_repl::Term::Key::CtrlC => {
                    status.disable_terminal_controls();
                    drop(raw);
                    status.exit_code.store(130, Ordering::SeqCst);
                    return;
                }
                jet_repl::Term::Key::Eof => {
                    status.disable_terminal_controls();
                    drop(raw);
                    return;
                }
                _ => {}
            }
        }
    }))
}

/// Pure — the parity words shared verbatim by the terminal line and the
/// browser strip's `message` field (`(state_word, rest)`; callers join them
/// with " · "). No I/O, no locking: easy to unit test every state directly.
fn header_words(
    state: &str,
    file: &str,
    code: &str,
    port: u16,
    clients: u64,
    last_build_ms: u128,
) -> (String, String) {
    let plural = if clients == 1 { "" } else { "s" };
    match state {
        "building" => (
            "building".to_string(),
            format!("{} · {} client{}", file, clients, plural),
        ),
        "error" => (
            "error".to_string(),
            format!("{} · {} client{}", code, clients, plural),
        ),
        _ => {
            let mut rest = format!("localhost:{} · {} client{}", port, clients, plural);
            if last_build_ms > 0 {
                rest.push_str(&format!(" · built {}", format_build_time(last_build_ms)));
            }
            ("ready".to_string(), rest)
        }
    }
}

fn format_build_time(ms: u128) -> String {
    format!("{:.1}s", ms as f64 / 1000.0)
}

fn format_line_colored(word: &str, rest: &str) -> String {
    let sgr = match word {
        "ready" => "32",
        "building" | "reconnecting" => "33",
        "error" => "31",
        _ => "37",
    };
    format!("jet dev  \x1b[{}m\u{25CF}\x1b[0m {} · {}", sgr, word, rest)
}

fn format_line_plain(word: &str, rest: &str) -> String {
    format!("jet dev  [{}] {}", word, rest)
}

/// Frame a verbatim diagnostic in a box for the pinned (TTY) terminal
/// surface — border/frame chars only, never a changed word or code (I4).
/// Off-TTY (`refresh`'s CI-floor branch) prints the same diagnostic
/// unframed instead of calling this at all.
fn frame_lines(code: &str, diagnostic: &str) -> Vec<String> {
    let lines: Vec<&str> = diagnostic.lines().collect();
    let content_width = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let width = content_width.max(code.len() + 2).clamp(20, 96);
    let dash_count = width.saturating_sub(code.len() + 3).max(1);
    let mut out = Vec::with_capacity(lines.len() + 2);
    out.push(format!("┌ {} {}", code, "─".repeat(dash_count)));
    for line in lines {
        out.push(format!("│ {}", line));
    }
    out.push(format!("└{}", "─".repeat(width + 2)));
    out
}

/// Wall-clock `HH:MM:SS` (UTC — std has no local-offset lookup without a
/// crate, I6) for verbose request/rebuild log lines. Cosmetic only, never
/// part of a diagnostic's verbatim text.
fn clock_time() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let sod = secs % 86400;
    format!("{:02}:{:02}:{:02}", sod / 3600, (sod % 3600) / 60, sod % 60)
}

/// Move every file `write_web_artifacts` staged into the real output
/// directory. Each `fs::rename` is atomic on the same filesystem (staging
/// lives under `build/`, so it always is), so a reader never observes a
/// half-written file; the individual renames aren't a single transaction, but
/// they only run after the entire staged set — including the wasm compile —
/// has already succeeded, so there is nothing left that can fail mid-swap
/// except a bare I/O error.
pub fn stage_and_swap(staging: &Path, out_dir: &Path) -> std::io::Result<()> {
    const FILES: [&str; 6] = [
        "web.manifest.json",
        "jet_dom_runtime.js",
        "app.js",
        "app_wasm.rs",
        "app.wasm",
        "index.html",
    ];
    const MAP_FILES: [&str; 2] = ["app.js.map", "app.wasm.map"];
    let output_root = ensure_real_output_dir(out_dir)?;
    ensure_existing_real_dir(staging, &output_root)?;
    for name in FILES {
        let src = staging.join(name);
        let dst = out_dir.join(name);
        reject_symlink_or_escape(&src, &output_root)?;
        reject_symlink_or_escape(&dst, &output_root)?;
        // `write_web_artifacts` already returned success for every one of
        // these paths (rustc included) before we get here, so a rename
        // failure here is a transient filesystem-visibility race, not a
        // missing file — a handful of retries with a short backoff clears it
        // without masking a genuine bug (which would fail every retry too).
        rename_with_retry(&src, &dst)?;
    }
    for name in MAP_FILES {
        let src = staging.join(name);
        let dst = out_dir.join(name);
        reject_symlink_or_escape(&dst, &output_root)?;
        if src.exists() {
            reject_symlink_or_escape(&src, &output_root)?;
            rename_with_retry(&src, &dst)?;
        } else if dst.exists() {
            // Release (or map-less) rebuild: drop stale maps with the swap.
            let _ = fs::remove_file(&dst);
        }
    }
    Ok(())
}

fn ensure_real_output_dir(path: &Path) -> std::io::Result<PathBuf> {
    let cwd = fs::canonicalize(".")?;
    ensure_directory_without_symlinks(path)?;
    let real = fs::canonicalize(path)?;
    if !real.starts_with(&cwd) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "web output directory escapes the working directory",
        ));
    }
    Ok(real)
}

fn ensure_existing_real_dir(path: &Path, root: &Path) -> std::io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "web staging directory must be a real directory",
        ));
    }
    let real = fs::canonicalize(path)?;
    if !real.starts_with(root) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "web staging directory escapes the output directory",
        ));
    }
    Ok(())
}

fn reject_symlink_or_escape(path: &Path, root: &Path) -> std::io::Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "web output paths must not be symlinks",
            ));
        }
        if let Some(parent) = path.parent() {
            let real_parent = fs::canonicalize(parent)?;
            if !real_parent.starts_with(root) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "web output path escapes the output directory",
                ));
            }
        }
    } else if let Some(parent) = path.parent() {
        let real_parent = fs::canonicalize(parent)?;
        if !real_parent.starts_with(root) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "web output path escapes the output directory",
            ));
        }
    }
    Ok(())
}

fn ensure_directory_without_symlinks(path: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "web output directory must not be a symlink",
        )),
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "web output path is not a directory",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                ensure_directory_without_symlinks(parent)?;
            }
            fs::create_dir(path)
        }
        Err(error) => Err(error),
    }
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

/// Bind the application preview listener. `Some(port)` (from `--port=<N>`)
/// binds that exact port and fails loud if it's taken — an explicit choice
/// isn't a hint. `None` scans the stable application range for a free port.
fn bind_application_server(port: Option<u16>) -> Result<TcpListener, String> {
    if let Some(port) = port {
        return match TcpListener::bind(("127.0.0.1", port)) {
            Ok(listener) => Ok(listener),
            Err(e) => {
                let fix = if e.kind() == std::io::ErrorKind::AddrInUse {
                    format!(
                        "\n fix: stop whatever's using port {}, or pick another with --port=<N>",
                        port
                    )
                } else {
                    String::new()
                };
                Err(format!(
                    "error: couldn't bind application preview to port {}: {}{}",
                    port, e, fix
                ))
            }
        };
    }
    let mut last_err: Option<std::io::Error> = None;
    for port in APPLICATION_PORT_RANGE {
        match TcpListener::bind(("127.0.0.1", port)) {
            Ok(listener) => return Ok(listener),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                last_err = Some(e);
                continue;
            }
            Err(e) => {
                return Err(format!(
                    "error: couldn't start the application preview: {}",
                    e
                ));
            }
        }
    }
    Err(format!(
        "error: every application preview port from {} to {} is already in use{}\n fix: free one of those ports, stop the other process using it, or pick one explicitly with --port=<N>",
        APPLICATION_PORT_RANGE.start(),
        APPLICATION_PORT_RANGE.end(),
        last_err.map(|e| format!(" ({})", e)).unwrap_or_default()
    ))
}

fn bind_canvas_server(host: &str, port: Option<u16>) -> Result<TcpListener, String> {
    let requested = port.unwrap_or(0);
    match TcpListener::bind((host, requested)) {
        Ok(listener) => Ok(listener),
        Err(error) => {
            let fix = if error.kind() == std::io::ErrorKind::AddrInUse {
                format!(
                    "\n fix: stop whatever's using port {}, or pick another with --canvas-port=<N>",
                    requested
                )
            } else {
                String::new()
            };
            Err(format!(
                "error: couldn't bind Canvas host {}:{}: {}{}",
                host, requested, error, fix
            ))
        }
    }
}

fn validate_canvas_options(options: &CanvasHostOptions) -> Result<String, String> {
    if options.transport != "http" {
        return Err(format!(
            "error: Canvas transport `{}` is unsupported; use `http`",
            options.transport
        ));
    }
    if !matches!(options.authority.as_str(), "loopback" | "remote") {
        return Err(format!(
            "error: Canvas authority `{}` is invalid; use `loopback` or `remote`",
            options.authority
        ));
    }
    let host = normalize_bind_host(&options.host)?;
    if !is_loopback_host(&host) && options.authority != "remote" {
        return Err(format!(
            "error: Canvas host `{host}` is not loopback; set `authority = remote` explicitly"
        ));
    }
    Ok(host)
}

fn mint_session_secret() -> Result<String, String> {
    let mut bytes = [0u8; CANVAS_SESSION_BYTES];
    let mut source = fs::File::open("/dev/urandom")
        .map_err(|error| format!("could not open the system random source: {error}"))?;
    std::io::Read::read_exact(&mut source, &mut bytes)
        .map_err(|error| format!("could not read the system random source: {error}"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn normalize_bind_host(host: &str) -> Result<String, String> {
    let host = host.trim();
    let host = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host);
    if host.is_empty() || host.chars().any(char::is_whitespace) || host.contains('/') {
        return Err(format!("error: Canvas host `{host}` is invalid"));
    }
    Ok(host.to_string())
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .map(|address| address.is_loopback())
            .unwrap_or(false)
}

fn url_host(host: &str) -> String {
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V6(_)) => format!("[{host}]"),
        _ => host.to_string(),
    }
}

fn canvas_api_path(path: &str, target: &str) -> bool {
    matches!(
        path,
        "/__jet_canvas"
            | "/__jet_canvas/"
            | "/__jet_canvas/app.js"
            | "/__jet_canvas/session"
            | "/__jet_canvas/live"
            | "/__jet_canvas/graph"
            | "/__jet_canvas/project"
            | "/__jet_canvas/source-control"
            | "/__jet_canvas/core-catalog"
            | "/__jet_canvas/proof"
            | "/__jet_canvas/source"
            | "/__jet_canvas/command"
            | "/__jet_canvas/transaction"
            | "/__jet_canvas/project/transaction"
            | "/__jet_canvas/query"
            | "/__jet_canvas/debug"
            | "/canvas"
            | "/canvas/"
            | "/canvas/app.js"
            | "/canvas/session"
            | "/canvas/graph"
            | "/canvas/project"
            | "/canvas/source-control"
            | "/canvas/core-catalog"
            | "/canvas/proof"
            | "/canvas/source"
            | "/canvas/command"
            | "/canvas/transaction"
            | "/canvas/project/transaction"
            | "/canvas/query"
            | "/canvas/debug"
            | "/panel"
            | "/panel/"
            | "/panel/app.js"
            | "/panel/session"
            | "/panel/graph"
            | "/panel/project"
            | "/panel/source-control"
            | "/panel/core-catalog"
            | "/panel/proof"
            | "/panel/source"
            | "/panel/command"
            | "/panel/transaction"
            | "/panel/project/transaction"
            | "/panel/query"
            | "/panel/debug"
            | "/__jet_dev_version"
            | "/__jet_dev_status"
            | "/__jet_dev_disconnect"
            | "/__jet_perf_browser"
    )
        || (path == "/"
            && (query_param(target, "jet_panel").as_deref() == Some("1")
                || query_param(target, "jet_panel_app").as_deref() == Some("1")
                || query_param(target, "jet_panel_graph").as_deref() == Some("1")))
}

fn canvas_request_authorized(
    request: &Request,
    target: &str,
    bind_host: &str,
    port: u16,
    session_secret: &str,
) -> bool {
    if !matches!(request.method.as_str(), "GET" | "POST")
        || request.body.len() > MAX_REQUEST_BODY_BYTES
        || request.headers.contains_key("transfer-encoding")
        || (request.method == "POST" && !request.headers.contains_key("content-length"))
        || (request.method == "GET" && !request.body.is_empty())
    {
        return false;
    }
    let path = target.split('?').next().unwrap_or("");
    if !canvas_api_path(path, target) {
        return false;
    }
    let Some(host) = request.headers.get("host") else {
        return false;
    };
    if !host_header_allowed(host, bind_host, port) {
        return false;
    }
    // Browser navigation and script loads omit Origin; only the session-bound
    // bootstrap paths may use their session URL as the origin proof.
    let origin = request.headers.get("origin");
    let same_origin = request
        .headers
        .get("sec-fetch-site")
        .is_some_and(|site| site.eq_ignore_ascii_case("same-origin"));
    if let Some(origin) = origin {
        if !origin_allowed(origin, bind_host, port) {
            return false;
        }
    } else if !canvas_bootstrap_path(path, target) && !same_origin {
        return false;
    }
    let authorization = request.headers.get("authorization");
    let query_session = match unique_session_param(target) {
        Ok(session) => session,
        Err(()) => return false,
    };
    let session_valid = match (authorization, query_session.as_deref()) {
        (Some(authorization), Some(query_session)) => authorization
            .strip_prefix("Bearer ")
            .is_some_and(|token| constant_time_equal(token, session_secret))
            && constant_time_equal(query_session, session_secret),
        (Some(authorization), None) => authorization
            .strip_prefix("Bearer ")
            .is_some_and(|token| constant_time_equal(token, session_secret)),
        (None, Some(query_session)) => constant_time_equal(query_session, session_secret),
        (None, None) => false,
    };
    session_valid && (origin.is_some() || canvas_bootstrap_path(path, target) || same_origin)
}

fn unique_session_param(target: &str) -> Result<Option<String>, ()> {
    let Some((_, query)) = target.split_once('?') else {
        return Ok(None);
    };
    let mut found = false;
    for part in query.split('&') {
        let name = part.split_once('=').map(|(name, _)| name).unwrap_or(part);
        if name == "session" {
            if found {
                return Err(());
            }
            found = true;
        }
    }
    Ok(query_param(target, "session"))
}

fn canvas_bootstrap_path(path: &str, target: &str) -> bool {
    matches!(
        path,
        "/__jet_canvas"
            | "/__jet_canvas/"
            | "/__jet_canvas/app.js"
            | "/canvas"
            | "/canvas/"
            | "/canvas/app.js"
            | "/panel"
            | "/panel/"
            | "/panel/app.js"
    ) || matches!(
        target,
        "/?jet_panel=1" | "/?jet_panel_app=1"
    )
}

fn host_header_allowed(value: &str, bind_host: &str, port: u16) -> bool {
    let value = value.trim().to_ascii_lowercase();
    if value == format!("{}:{}", url_host(bind_host), port).to_ascii_lowercase() {
        return true;
    }
    if !is_loopback_host(bind_host) {
        return false;
    }
    ["localhost", "127.0.0.1", "[::1]"]
        .iter()
        .any(|alias| value == format!("{alias}:{port}"))
}

fn origin_allowed(value: &str, bind_host: &str, port: u16) -> bool {
    let value = value.trim().to_ascii_lowercase();
    let expected = format!("http://{}:{}", url_host(bind_host), port).to_ascii_lowercase();
    if value == expected {
        return true;
    }
    is_loopback_host(bind_host)
        && ["localhost", "127.0.0.1", "[::1]"]
            .iter()
            .any(|alias| value == format!("http://{alias}:{port}"))
}

fn constant_time_equal(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    let mut difference = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        difference |= left.get(index).copied().unwrap_or(0) as usize
            ^ right.get(index).copied().unwrap_or(0) as usize;
    }
    difference == 0
}

fn unauthorized(stream: &mut TcpStream) -> std::io::Result<()> {
    write_response(
        stream,
        "401 Unauthorized",
        "text/plain; charset=utf-8",
        b"Canvas session, host, or origin rejected",
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ListenerKind {
    Canvas,
    Application,
}

/// Accept connections forever, one thread per connection (a dev tool serving
/// a handful of small files has no need for a worker pool).
fn serve_forever(
    listener: TcpListener,
    status: Arc<DevStatus>,
    debug_sessions: Arc<crate::Canvas::DebugSessions>,
    session: Arc<crate::ResidentDevSession>,
    canvas_file: String,
    listener_kind: ListenerKind,
    canvas_only: bool,
    bind_host: String,
    session_secret: String,
    shutdown: Arc<AtomicBool>,
    static_root: PathBuf,
    source_asset_fallback: bool,
) {
    if listener.set_nonblocking(true).is_err() {
        return;
    }
    let active_connections = Arc::new(AtomicUsize::new(0));
    while !shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                if !try_acquire_connection(&active_connections) {
                    drop(stream);
                    continue;
                }
                let status = Arc::clone(&status);
                let debug_sessions = Arc::clone(&debug_sessions);
                let session = Arc::clone(&session);
                let canvas_file = canvas_file.clone();
                let bind_host = bind_host.clone();
                let session_secret = session_secret.clone();
                let static_root = static_root.clone();
                let active_connections = Arc::clone(&active_connections);
                thread::spawn(move || {
                    let _ = handle_connection_with_root(
                        stream,
                        &status,
                        &debug_sessions,
                        &session,
                        &canvas_file,
                        listener_kind,
                        canvas_only,
                        &bind_host,
                        &session_secret,
                        &static_root,
                        source_asset_fallback,
                    );
                    active_connections.fetch_sub(1, Ordering::AcqRel);
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                let _ = wait_for_shutdown(&shutdown, Duration::from_millis(10));
            }
            Err(_) => return,
        }
    }
}

fn try_acquire_connection(active: &AtomicUsize) -> bool {
    let mut current = active.load(Ordering::Acquire);
    loop {
        if current >= MAX_CONNECTION_THREADS {
            return false;
        }
        match active.compare_exchange(
            current,
            current + 1,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return true,
            Err(next) => current = next,
        }
    }
}

/// Parse one GET request line + headers (no keep-alive, no request body —
/// this is a dev tool, not a production server) and serve a response.
#[cfg(test)]
fn handle_connection(
    stream: TcpStream,
    status: &DevStatus,
    debug_sessions: &crate::Canvas::DebugSessions,
    session: &crate::ResidentDevSession,
    canvas_file: &str,
    listener_kind: ListenerKind,
    canvas_only: bool,
    bind_host: &str,
    session_secret: &str,
) -> std::io::Result<()> {
    handle_connection_with_root(
        stream,
        status,
        debug_sessions,
        session,
        canvas_file,
        listener_kind,
        canvas_only,
        bind_host,
        session_secret,
        Path::new("build"),
        true,
    )
}

fn handle_connection_with_root(
    stream: TcpStream,
    status: &DevStatus,
    debug_sessions: &crate::Canvas::DebugSessions,
    session: &crate::ResidentDevSession,
    canvas_file: &str,
    listener_kind: ListenerKind,
    canvas_only: bool,
    bind_host: &str,
    session_secret: &str,
    static_root: &Path,
    source_asset_fallback: bool,
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let Some(request) = Request::read(&mut reader)? else {
        return Ok(());
    };

    let mut stream = stream;
    let method = request.method.as_str();
    let target = request.target.as_str();
    let body = request.body.as_slice();
    let path = target.split('?').next().unwrap_or("/");

    if listener_kind == ListenerKind::Canvas
        && !canvas_request_authorized(&request, target, bind_host, status.port(), session_secret)
    {
        return unauthorized(&mut stream);
    }

    if method != "GET" && method != "POST" {
        return write_response(
            &mut stream,
            "405 Method Not Allowed",
            "text/plain; charset=utf-8",
            b"jet dev's web server only handles GET and Canvas POST requests",
        );
    }

    if let Some(client) = query_param(target, "client_id") {
        session.note_client(&client);
    }
    if listener_kind == ListenerKind::Application && !is_application_control_path(path) {
        if method != "GET" {
            return method_not_allowed(&mut stream);
        }
        let started = Instant::now();
        let nonce = status
            .browser_relay
            .lock()
            .unwrap()
            .as_ref()
            .map(|relay| relay.nonce().to_string())
            .unwrap_or_default();
        let code = serve_static_from_root(
            &mut stream,
            static_root,
            path,
            &nonce,
            canvas_file,
            source_asset_fallback,
        )?;
        status.log_request(method, path, code, started.elapsed());
        return Ok(());
    }
    if path == "/__jet_canvas/session" || path == "/canvas/session" || path == "/panel/session" {
        let client = query_param(target, "client_id").unwrap_or_default();
        if !client.is_empty() {
            session.note_client(&client);
        }
        if method == "GET" {
            let body = session_response(session);
            return write_response(
                &mut stream,
                "200 OK",
                "application/json; charset=utf-8",
                body.as_bytes(),
            );
        }
        if method != "GET" {
            return method_not_allowed(&mut stream);
        }
        let body = session_response(session);
        return write_response(
            &mut stream,
            "200 OK",
            "application/json; charset=utf-8",
            body.as_bytes(),
        );
    }
    if path == "/__jet_canvas/live" {
        if method != "GET" {
            return method_not_allowed(&mut stream);
        }
        let pid = query_param(target, "pid")
            .and_then(|value| value.parse::<u32>().ok())
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing live pid")
            })?;
        let source_id = query_param(target, "source_id").unwrap_or_default();
        return match crate::LiveInspect::read(pid) {
            Ok(snapshot) => {
                let revision = current_revision_for_source_id(
                    canvas_file,
                    (!source_id.is_empty()).then_some(source_id.as_str()),
                );
                session.remember_last_good_view("runtime", &source_id, &revision, &snapshot);
                write_response(
                    &mut stream,
                    "200 OK",
                    "application/json; charset=utf-8",
                    snapshot.as_bytes(),
                )
            }
            Err(message) => match session.last_good_view("runtime", &source_id) {
                Some((_revision, snapshot)) => write_response(
                    &mut stream,
                    "200 OK",
                    "application/json; charset=utf-8",
                    snapshot.as_bytes(),
                ),
                None => write_response(
                    &mut stream,
                    "404 Not Found",
                    "text/plain; charset=utf-8",
                    message.as_bytes(),
                ),
            },
        };
    }
    if let Some(asset) = crate::canvas_asset(method, target, path) {
        let asset = inject_canvas_session(asset, session_secret);
        return write_response(
            &mut stream,
            asset.status,
            asset.content_type,
            asset.body.as_bytes(),
        );
    }
    if target == "/?jet_panel_graph=1" {
        if method != "GET" {
            return method_not_allowed(&mut stream);
        }
        return match crate::Canvas::graph_json_for_file(Path::new(canvas_file)) {
            Ok(body) => {
                session.select_project_source_from_payload(&body);
                let source_id = source_id_from_payload(&body);
                let revision = current_revision_for_source_id(
                    canvas_file,
                    (!source_id.is_empty()).then_some(source_id.as_str()),
                );
                session.remember_last_good_view("graph", &source_id, &revision, &body);
                write_response(
                    &mut stream,
                    "200 OK",
                    "application/json; charset=utf-8",
                    with_session(body, session).as_bytes(),
                )
            }
            Err(diags) => {
                let body = with_session(
                    crate::Canvas::graph_json_error_for_file(Path::new(canvas_file), &diags),
                    session,
                );
                write_response(
                    &mut stream,
                    "409 Conflict",
                    "application/json; charset=utf-8",
                    body.as_bytes(),
                )
            }
        };
    }
    if path == "/__jet_canvas/graph" || path == "/canvas/graph" || path == "/panel/graph" {
        if method != "GET" {
            return method_not_allowed(&mut stream);
        }
        let source_id = query_param(target, "source_id");
        let graph = match query_param(target, "pid") {
            Some(pid) => crate::Canvas::graph_json_for_entry_source_with_live_pid(
                Path::new(canvas_file),
                source_id.as_deref(),
                pid.parse().unwrap_or(0),
            ),
            None => crate::Canvas::graph_json_for_entry_source(
                Path::new(canvas_file),
                source_id.as_deref(),
            ),
        };
        if let Some(source_id) = source_id.as_deref() {
            session.select_project_source(source_id);
        }
        return match graph {
            Ok(body) => {
                session.select_project_source_from_payload(&body);
                let source_id = source_id
                    .clone()
                    .unwrap_or_else(|| source_id_from_payload(&body));
                let revision = current_revision_for_source_id(
                    canvas_file,
                    (!source_id.is_empty()).then_some(source_id.as_str()),
                );
                session.remember_last_good_view(
                    "graph",
                    &source_id,
                    &revision,
                    &body,
                );
                write_response(
                    &mut stream,
                    "200 OK",
                    "application/json; charset=utf-8",
                    with_session(body, session).as_bytes(),
                )
            }
            Err(body) => write_response(
                &mut stream,
                "409 Conflict",
                "application/json; charset=utf-8",
                with_session(body, session).as_bytes(),
            ),
        };
    }
    if path == "/__jet_canvas/project" || path == "/canvas/project" || path == "/panel/project" {
        if method != "GET" {
            return method_not_allowed(&mut stream);
        }
        let body = with_session(
            crate::Canvas::project_json_for_entry(Path::new(canvas_file)),
            session,
        );
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
        let body = with_session(
            crate::Canvas::source_control_json_for_entry(Path::new(canvas_file)),
            session,
        );
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
        return match crate::Canvas::core_catalog_json_for_entry(Path::new(canvas_file), &query) {
            Ok(body) => write_response(
                &mut stream,
                "200 OK",
                "application/json; charset=utf-8",
                with_session(body, session).as_bytes(),
            ),
            Err(body) => write_response(
                &mut stream,
                "409 Conflict",
                "application/json; charset=utf-8",
                with_session(body, session).as_bytes(),
            ),
        };
    }
    if path == "/__jet_canvas/proof" || path == "/canvas/proof" || path == "/panel/proof" {
        if method != "GET" {
            return method_not_allowed(&mut stream);
        }
        let source_id = query_param(target, "source_id");
        let receipt = status.command_receipt();
        return match crate::Canvas::proof_json_for_entry_with_receipt(
            Path::new(canvas_file),
            source_id.as_deref(),
            receipt.as_deref(),
        ) {
            Ok(body) => write_response(
                &mut stream,
                "200 OK",
                "application/json; charset=utf-8",
                with_session(body, session).as_bytes(),
            ),
            Err(body) => write_response(
                &mut stream,
                "409 Conflict",
                "application/json; charset=utf-8",
                with_session(body, session).as_bytes(),
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
            .and_then(|id| crate::Canvas::project_path_for_source_id(Path::new(canvas_file), id))
            .unwrap_or_else(|| PathBuf::from(canvas_file));
        return match fs::read(&source_path) {
            Ok(body) => write_response(&mut stream, "200 OK", "text/plain; charset=utf-8", &body),
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
        return match crate::Canvas::command_receipt_json_for_entry(Path::new(canvas_file), &request)
        {
            Ok(body) => {
                status.record_command_receipt(body.clone());
                session.record_command(&request);
                write_response(
                    &mut stream,
                    "200 OK",
                    "application/json; charset=utf-8",
                    with_session(body, session).as_bytes(),
                )
            }
            Err(body) => write_response(
                &mut stream,
                "409 Conflict",
                "application/json; charset=utf-8",
                with_session(body, session).as_bytes(),
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
        let _transaction = session.lock_source_transaction();
        return match crate::Canvas::apply_transaction_json(Path::new(canvas_file), &request) {
            Ok(body) => {
                status.version.fetch_add(1, Ordering::SeqCst);
                let revision = current_revision_for_request(canvas_file, &request);
                session.accept_transaction(&request, &revision);
                write_response(
                    &mut stream,
                    "200 OK",
                    "application/json; charset=utf-8",
                    with_session(body, session).as_bytes(),
                )
            }
            Err(body) => {
                let revision = current_revision_for_request(canvas_file, &request);
                session.refuse_transaction(&request, &revision);
                write_response(
                    &mut stream,
                    "409 Conflict",
                    "application/json; charset=utf-8",
                    with_session(body, session).as_bytes(),
                )
            }
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
        let _transaction = session.lock_source_transaction();
        return match crate::Canvas::apply_project_transaction_json(Path::new(canvas_file), &request)
        {
            Ok(body) => {
                status.version.fetch_add(1, Ordering::SeqCst);
                let revision = current_revision_for_request(canvas_file, &request);
                session.accept_transaction(&request, &revision);
                write_response(
                    &mut stream,
                    "200 OK",
                    "application/json; charset=utf-8",
                    with_session(body, session).as_bytes(),
                )
            }
            Err(body) => {
                let revision = current_revision_for_request(canvas_file, &request);
                session.refuse_transaction(&request, &revision);
                write_response(
                    &mut stream,
                    "409 Conflict",
                    "application/json; charset=utf-8",
                    with_session(body, session).as_bytes(),
                )
            }
        };
    }
    if path == "/__jet_canvas/query" || path == "/canvas/query" || path == "/panel/query" {
        if method != "POST" {
            return method_not_allowed(&mut stream);
        }
        let request = String::from_utf8_lossy(&body);
        return match crate::Canvas::query_json_for_entry(Path::new(canvas_file), &request) {
            Ok(body) => write_response(
                &mut stream,
                "200 OK",
                "application/json; charset=utf-8",
                with_session(body, session).as_bytes(),
            ),
            Err(body) => write_response(
                &mut stream,
                "409 Conflict",
                "application/json; charset=utf-8",
                with_session(body, session).as_bytes(),
            ),
        };
    }
    if path == "/__jet_canvas/debug" || path == "/canvas/debug" || path == "/panel/debug" {
        if method != "POST" {
            return method_not_allowed(&mut stream);
        }
        let request = String::from_utf8_lossy(&body);
        return match crate::Canvas::debug_session_json_for_entry_with_sessions(
            Path::new(canvas_file),
            &request,
            debug_sessions,
        ) {
            Ok(body) => {
                session.record_debug_response(&request, &body);
                write_response(
                    &mut stream,
                    "200 OK",
                    "application/json; charset=utf-8",
                    with_session(body, session).as_bytes(),
                )
            }
            Err(body) => write_response(
                &mut stream,
                "409 Conflict",
                "application/json; charset=utf-8",
                with_session(body, session).as_bytes(),
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
        if let Some(client) = query_param(target, "client") {
            status.note_client(&client);
            session.note_client(&client);
        }
        let mut body = status.json();
        if body.ends_with('}') {
            body.pop();
            body.push_str(",\"session\":");
            body.push_str(&session.json());
            body.push('}');
        }
        return write_response(
            &mut stream,
            "200 OK",
            "application/json; charset=utf-8",
            body.as_bytes(),
        );
    }
    if path == "/__jet_dev_disconnect" {
        if method != "POST" {
            return method_not_allowed(&mut stream);
        }
        if let Some(client) = query_param(target, "client") {
            status.drop_client(&client);
            session.drop_client(&client);
        }
        return write_response(&mut stream, "200 OK", "text/plain; charset=utf-8", b"ok");
    }
    if path == "/__jet_perf_browser" {
        if method != "POST" {
            return method_not_allowed(&mut stream);
        }
        let relay = status.browser_relay.lock().unwrap();
        let Some(relay) = relay.as_ref() else {
            return write_response(
                &mut stream,
                "503 Service Unavailable",
                "text/plain; charset=utf-8",
                b"browser trace relay unavailable",
            );
        };
        if query_param(target, "nonce").as_deref() != Some(relay.nonce()) {
            return write_response(
                &mut stream,
                "403 Forbidden",
                "text/plain; charset=utf-8",
                b"stale or foreign browser trace session",
            );
        }
        return match relay.record(&body) {
            Ok(()) => write_response(
                &mut stream,
                "204 No Content",
                "text/plain; charset=utf-8",
                b"",
            ),
            Err(crate::BrowserTrace::RecordError::Oversized) => write_response(
                &mut stream,
                "413 Payload Too Large",
                "text/plain; charset=utf-8",
                b"browser trace envelope exceeds 512 bytes",
            ),
            Err(crate::BrowserTrace::RecordError::Malformed) => write_response(
                &mut stream,
                "400 Bad Request",
                "text/plain; charset=utf-8",
                b"browser trace envelope is malformed",
            ),
            Err(crate::BrowserTrace::RecordError::Unavailable) => write_response(
                &mut stream,
                "503 Service Unavailable",
                "text/plain; charset=utf-8",
                b"browser trace relay unavailable",
            ),
        };
    }
    if canvas_only {
        return write_response(
            &mut stream,
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"Canvas control host does not serve program assets",
        );
    }
    // Only page/asset GETs are worth a request-log line (D-FE-DEVSRV1=D's
    // verbose log shows `GET / 200 2ms`, not the 400ms `/__jet_dev_version`
    // poll noise) — those are handled above and already returned.
    let started = Instant::now();
    let nonce = status
        .browser_relay
        .lock()
        .unwrap()
        .as_ref()
        .map(|relay| relay.nonce().to_string())
        .unwrap_or_default();
    let code = serve_static_from_root(
        &mut stream,
        static_root,
        path,
        &nonce,
        canvas_file,
        source_asset_fallback,
    )?;
    status.log_request(method, path, code, started.elapsed());
    Ok(())
}

fn inject_canvas_session(
    mut asset: crate::CanvasAsset,
    session_secret: &str,
) -> crate::CanvasAsset {
    if asset.content_type == "text/html; charset=utf-8" {
        if let Some(index) = asset.body.find("app.js?") {
            let insert_at = index + "app.js?".len();
            asset
                .body
                .insert_str(insert_at, &format!("session={session_secret}&"));
        }
    }
    asset
}

fn method_not_allowed(stream: &mut TcpStream) -> std::io::Result<()> {
    write_response(
        stream,
        "405 Method Not Allowed",
        "text/plain; charset=utf-8",
        b"method not allowed",
    )
}

fn is_application_control_path(path: &str) -> bool {
    matches!(
        path,
        "/__jet_dev_status"
            | "/__jet_dev_version"
            | "/__jet_dev_disconnect"
            | "/__jet_perf_browser"
    )
}

fn session_response(session: &crate::ResidentDevSession) -> String {
    format!(
        "{{\"protocol\":\"jet.canvas.session\",\"schema_version\":1,\"session\":{}}}",
        session.json()
    )
}

fn with_session(body: String, session: &crate::ResidentDevSession) -> String {
    let marker = "\"canvas\":{";
    let Some(index) = body.find(marker) else {
        return body;
    };
    let insert_at = index + marker.len();
    let mut decorated = String::with_capacity(body.len() + session.json().len() + 16);
    decorated.push_str(&body[..insert_at]);
    decorated.push_str("\"session\":");
    decorated.push_str(&session.json());
    decorated.push(',');
    decorated.push_str(&body[insert_at..]);
    decorated
}

fn current_revision_for_request(canvas_file: &str, request: &str) -> String {
    let source_id = jet_foundation::JSON::parse_json(request)
        .ok()
        .and_then(|value| match value {
            jet_foundation::JSON::JSONValue::Object(object) => {
                object.get("source_id").and_then(|value| match value {
                    jet_foundation::JSON::JSONValue::String(source_id) => Some(source_id.clone()),
                    _ => None,
                })
            }
            _ => None,
        })
        .unwrap_or_default();
    current_revision_for_source_id(
        canvas_file,
        (!source_id.is_empty()).then_some(source_id.as_str()),
    )
}

fn source_id_from_payload(payload: &str) -> String {
    let Ok(value) = jet_foundation::JSON::parse_json(payload) else {
        return String::new();
    };

    fn find(value: &jet_foundation::JSON::JSONValue) -> Option<String> {
        match value {
            jet_foundation::JSON::JSONValue::Object(object) => {
                if let Some(jet_foundation::JSON::JSONValue::String(source_id)) =
                    object.get("source_id")
                {
                    return Some(source_id.clone());
                }
                object.values().find_map(find)
            }
            jet_foundation::JSON::JSONValue::Array(values) => values.iter().find_map(find),
            _ => None,
        }
    }

    find(&value).unwrap_or_default()
}

fn current_revision_for_source_id(canvas_file: &str, source_id: Option<&str>) -> String {
    let source_path = source_id
        .and_then(|source_id| {
            crate::Canvas::project_path_for_source_id(Path::new(canvas_file), source_id)
        })
        .unwrap_or_else(|| PathBuf::from(canvas_file));
    fs::read_to_string(source_path)
        .map(|source| crate::Canvas::source_revision(&source))
        .unwrap_or_default()
}

fn serve_static_from_root(
    stream: &mut TcpStream,
    root: &Path,
    path: &str,
    nonce: &str,
    canvas_file: &str,
    source_asset_fallback: bool,
) -> std::io::Result<u16> {
    let root_path = match static_path(root, path) {
        Ok(path) => path,
        Err(()) => {
            write_response(
                stream,
                "400 Bad Request",
                "text/plain; charset=utf-8",
                b"bad path",
            )?;
            return Ok(400);
        }
    };
    let file_path = if root_path.is_file() {
        root_path
    } else if source_asset_fallback {
        source_asset_path(canvas_file, path).unwrap_or(root_path)
    } else {
        root_path
    };
    let bytes = match fs::read(&file_path) {
        Ok(b) => b,
        Err(_) => {
            let body = format!("not found: {}", path);
            write_response(
                stream,
                "404 Not Found",
                "text/plain; charset=utf-8",
                body.as_bytes(),
            )?;
            return Ok(404);
        }
    };

    let content_type = content_type_for(&file_path);
    let is_html = file_path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("html"));
    if is_html {
        let html = String::from_utf8_lossy(&bytes).into_owned();
        let injected = inject_live_reload(&html, nonce);
        write_response(stream, "200 OK", content_type, injected.as_bytes())?;
        return Ok(200);
    }
    write_response(stream, "200 OK", content_type, &bytes)?;
    Ok(200)
}

fn source_asset_path(canvas_file: &str, request_path: &str) -> Option<PathBuf> {
    let source = Path::new(canvas_file);
    let source_dir = source
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut roots = vec![source_dir.to_path_buf()];
    if let Ok(source_text) = fs::read_to_string(source) {
        let mut offset = 0;
        while let Some(relative) = source_text[offset..].find("HTML(") {
            let start = offset + relative + "HTML(".len();
            let Some(rest) = source_text.get(start..) else {
                break;
            };
            let rest = rest.trim_start();
            let quote = rest.as_bytes().first().copied();
            if !matches!(quote, Some(b'\'' | b'"')) {
                offset = start;
                continue;
            }
            let rest = &rest[1..];
            let Some(end) = rest.find(char::from(quote.unwrap())) else {
                offset = start;
                continue;
            };
            let shell = source_dir.join(&rest[..end]);
            if let Some(shell_dir) = shell.parent() {
                roots.push(shell_dir.to_path_buf());
            }
            offset = start;
        }
    }
    roots.sort();
    roots.dedup();
    for root in roots {
        let Ok(candidate) = static_path(&root, request_path) else {
            continue;
        };
        if candidate == source
            || candidate
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("jet"))
        {
            continue;
        }
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Plain polling live-reload (no WebSocket/SSE — I6 + correct scoping for a
/// dev tool): fetch `/__jet_dev_status` on an interval, reload the page the
/// first time `version` differs from the value seen at page load.
///
/// D-FE-DEVSRV1=D (hybrid): the corner strip is a collapsed pill mirroring
/// the exact same `message` string the terminal's parity line renders —
/// same poll, same words, cannot drift. On `state:"error"` the strip expands
/// into a full-viewport dimmed overlay framing the verbatim diagnostic
/// (I4 — `s.diagnostic` is written via `textContent`, never re-escaped or
/// reworded); a clean rebuild (browser strip is dumb — it just re-renders
/// whatever the poll says) collapses it back to the pill.
fn live_reload_script(nonce: &str) -> String {
    let perf_script = if nonce.is_empty() {
        String::new()
    } else {
        format!(
            r#"  var jetPerfNonce = "{nonce}";
  self.__jetPerfNow = function () {{ return performance.now(); }};
  self.__jetPerfRecord = function (symbol, eventClass, startMs) {{
    if (!symbol || typeof performance === "undefined") return;
    var endMs = performance.now();
    var body = new URLSearchParams({{
      class: String(eventClass), symbol: String(symbol),
      start_ns: String(Math.max(0, Math.floor(startMs * 1000000))),
      duration_ns: String(Math.max(0, Math.floor((endMs - startMs) * 1000000))),
      clock_ns: String(Math.max(0, Math.floor(endMs * 1000000)))
    }});
    try {{ navigator.sendBeacon("/__jet_perf_browser?nonce=" + jetPerfNonce, body); }} catch (_) {{}}
  }};
"#
        )
    };
    format!(
        r##"<script>
(function () {{
{perf_script}
  var jetDevVersion = null, reconnectAttempt = 0;
  var jetDevClient = null;
  try {{ jetDevClient = sessionStorage.getItem("jet-dev-client"); }} catch (_) {{}}
  if (!jetDevClient) {{
    jetDevClient = (self.crypto && crypto.randomUUID) ? crypto.randomUUID() :
      String(Date.now()) + "-" + Math.random().toString(16).slice(2);
    try {{ sessionStorage.setItem("jet-dev-client", jetDevClient); }} catch (_) {{}}
  }}
  addEventListener("pagehide", function () {{
    try {{ navigator.sendBeacon("/__jet_dev_disconnect?client=" + encodeURIComponent(jetDevClient)); }} catch (_) {{}}
  }});
  var pill = null, shade = null, overlay = null, overlayTitle = null, overlayBody = null, overlayFooter = null;
  var dismissedDiagnostic = null;
  function ensureUi() {{
    if (pill) return;
    var style = document.createElement("style");
    style.textContent =
      "#jet-dev-pill{{position:fixed;left:12px;bottom:12px;z-index:2147483647;" +
      "font:12px ui-monospace,SFMono-Regular,Consolas,monospace;background:#101820;" +
      "color:#d6e7ff;border:1px solid #3b5675;border-radius:999px;padding:4px 10px;" +
      "box-shadow:0 4px 16px rgba(0,0,0,.35)}}" +
      "#jet-dev-pill .dot{{display:inline-block;width:8px;height:8px;border-radius:50%;margin-right:6px}}" +
      "#jet-dev-shade{{position:fixed;inset:0;z-index:2147483645;display:none;pointer-events:none;" +
      "background:rgba(11,17,25,.18);backdrop-filter:grayscale(.4) opacity(.72)}}" +
      "#jet-dev-overlay{{position:fixed;inset:0;z-index:2147483646;background:rgba(6,10,16,.82);" +
      "display:none;align-items:flex-start;justify-content:center;padding:24px}}" +
      "#jet-dev-overlay .box{{max-width:min(760px,92vw);max-height:82vh;overflow:auto;" +
      "background:#101820;color:#f2f6fb;border:1px solid #3b5675;border-radius:8px;" +
      "box-shadow:0 24px 64px rgba(0,0,0,.5)}}" +
      "#jet-dev-overlay .box h3{{margin:0;padding:10px 14px;border-bottom:1px solid #263a52;" +
      "font:600 13px ui-monospace,SFMono-Regular,Consolas,monospace;color:#ff8a80}}" +
      "#jet-dev-overlay .box pre{{margin:0;padding:14px;white-space:pre-wrap;" +
      "font:12px ui-monospace,SFMono-Regular,Consolas,monospace;line-height:1.5}}" +
      "#jet-dev-overlay .box footer{{padding:8px 14px;border-top:1px solid #263a52;" +
      "font:12px ui-monospace,SFMono-Regular,Consolas,monospace;color:#9fb4cc}}";
    document.head.appendChild(style);
    pill = document.createElement("div");
    pill.id = "jet-dev-pill";
    pill.innerHTML = "<span class=\"dot\"></span><span class=\"label\"></span>";
    document.body.appendChild(pill);
    shade = document.createElement("div");
    shade.id = "jet-dev-shade";
    document.body.appendChild(shade);
    overlay = document.createElement("div");
    overlay.id = "jet-dev-overlay";
    overlay.innerHTML = "<div class=\"box\"><h3>Build failed</h3><pre></pre><footer></footer></div>";
    document.body.appendChild(overlay);
    overlayTitle = overlay.querySelector("h3");
    overlayBody = overlay.querySelector("pre");
    overlayFooter = overlay.querySelector("footer");
    document.addEventListener("keydown", function (event) {{
      if (event.key === "Escape" && overlay.style.display !== "none") {{
        dismissedDiagnostic = overlayBody.textContent;
        overlay.style.display = "none";
      }}
    }});
  }}
  function dotColor(state) {{
    if (state === "error") return "#FF5C5C";
    if (state === "building" || state === "reconnecting") return "#FFB454";
    return "#58D68D";
  }}
  function renderStatus(s) {{
    ensureUi();
    var state = s.state || "ready";
    pill.querySelector(".dot").style.background = dotColor(state);
    // `.textContent` — never re-escape or reword the shared parity string
    // (I8: one status mechanism, two entrypoints reading the same words).
    pill.querySelector(".label").textContent = s.message || state;
    shade.style.display = (state === "building" || state === "reconnecting") ? "block" : "none";
    if (state === "error") {{
      // The overlay's own translucent backdrop (rgba(6,10,16,.82)) is what
      // reads as "app, dimmed" — it sits over the still-live last-good page.
      overlay.style.display = dismissedDiagnostic === (s.diagnostic || "") ? "none" : "flex";
      // Verbatim diagnostic (I4): exactly what the terminal's framed box
      // and `render_diagnostics` produced, byte-for-byte via `textContent`.
      overlayBody.textContent = s.diagnostic || "";
      overlayTitle.textContent = "Build failed — " + (s.file || "Jet source");
      overlayFooter.textContent = (s.message || state) + " — clears on the next clean build";
    }} else {{
      overlay.style.display = "none";
      dismissedDiagnostic = null;
    }}
  }}
  function poll() {{
    fetch("/__jet_dev_status?client=" + encodeURIComponent(jetDevClient), {{ cache: "no-store" }})
      .then(function (r) {{ return r.json(); }})
      .then(function (s) {{
        var recoveredConnection = reconnectAttempt > 0;
        reconnectAttempt = 0;
        renderStatus(s);
        var v = String(s.version || "");
        if (jetDevVersion === null) {{
          jetDevVersion = v;
          if (recoveredConnection) location.reload();
          return;
        }}
        // Never bumps on an error build (`DevStatus::mark_error` doesn't
        // touch `version`) — a reload only ever follows the next clean build,
        // which is also what auto-collapses the overlay above.
        // A recovered connection also reloads even when a restarted server's
        // version counter happens to equal the previous process's counter.
        if (recoveredConnection || v !== jetDevVersion) {{ jetDevVersion = v; location.reload(); }}
      }})
      .catch(function () {{
        reconnectAttempt += 1;
        renderStatus({{ state: "reconnecting", message: "reconnecting · waiting for connection" }});
      }})
      .finally(function () {{ setTimeout(poll, {poll_ms}); }});
  }}
  setTimeout(poll, 0);
}})();
</script>
"##,
        poll_ms = LIVE_RELOAD_POLL_MS,
        perf_script = perf_script
    )
}

fn inject_live_reload(html: &str, nonce: &str) -> String {
    let script = live_reload_script(nonce);
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

#[cfg(test)]
mod tests {
    use super::{
        canvas_api_path, canvas_request_authorized, constant_time_equal, format_build_time,
        format_line_colored, format_line_plain, frame_lines, header_words, host_header_allowed,
        handle_connection, inject_canvas_session, inject_live_reload, mint_session_secret,
        origin_allowed, stage_and_swap, APPLICATION_PORT_RANGE, CanvasHostOptions, DevStatus,
        ListenerKind, Ordering, WebHost, bind_application_server,
    };
    use crate::Request;
    use std::collections::HashMap;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::Duration;

    #[cfg(unix)]
    #[test]
    fn stage_and_swap_rejects_symlinked_destination() {
        use std::os::unix::fs::symlink;

        let root = std::env::current_dir()
            .unwrap()
            .join(format!(".jet-webhost-symlink-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let output = root.join("build");
        let staging = output.join(".staging");
        let outside = root.join("outside.js");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(&staging.join("web.manifest.json"), "staged").unwrap();
        std::fs::write(&outside, "must survive").unwrap();
        symlink(&outside, output.join("web.manifest.json")).unwrap();

        assert!(
            stage_and_swap(&staging, &output).is_err(),
            "web finalization must not replace a symlinked output"
        );
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "must survive");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn injects_before_closing_body_tag() {
        let html = "<html><body><p>hi</p></BODY></html>";
        let out = inject_live_reload(html, "nonce");
        assert!(out.contains("<script>"));
        assert!(out.find("<script>").unwrap() < out.find("</BODY>").unwrap());
    }

    #[test]
    fn appends_when_no_body_tag() {
        let html = "<html><p>hi</p></html>";
        let out = inject_live_reload(html, "nonce");
        assert!(out.starts_with(html));
        assert!(out.contains("<script>"));
    }

    #[test]
    fn injected_script_shows_verbatim_overlay_and_parity_pill() {
        let html = "<html><body></body></html>";
        let out = inject_live_reload(html, "nonce");
        // The browser overlay's title is a static literal, not derived from
        // the diagnostic — the diagnostic body is written via `textContent`
        // (never re-escaped/re-worded, I4).
        assert!(out.contains("Build failed"));
        assert!(out.contains("__jet_dev_status"));
        assert!(out.contains("var jetPerfNonce = \"nonce\""));
        assert!(out.contains("/__jet_perf_browser?nonce="));
        assert!(out.contains("overlayBody.textContent = s.diagnostic"));
        assert!(out.contains("pill.querySelector(\".label\").textContent = s.message"));
        assert!(out.contains("state === \"building\" || state === \"reconnecting\""));
        assert!(out.contains("event.key === \"Escape\""));
        assert!(out.contains("dismissedDiagnostic === (s.diagnostic || \"\")"));
        assert!(out.contains("location.reload()"));
        assert!(out.contains("recoveredConnection || v !== jetDevVersion"));
        assert!(out.contains("reconnecting · waiting for connection"));
        assert!(!out.contains("setInterval("));
        assert!(out.contains("finally(function () { setTimeout(poll, 40); })"));
        assert!(out.contains("position:fixed;left:12px;bottom:12px"));
        assert!(out.contains("display:none;align-items:flex-start"));
    }

    #[test]
    fn live_verbose_toggle_changes_depth_without_changing_status() {
        let status = DevStatus::new("app.jet", false);
        assert!(!status.verbose());
        let before = status.json();
        status.toggle_verbose();
        assert!(status.verbose());
        assert_eq!(status.json(), before);
        status.toggle_verbose();
        assert!(!status.verbose());
        assert_eq!(status.json(), before);
    }

    #[test]
    fn dashboard_detail_keeps_watched_target_and_canvas_route_pinned() {
        let status = DevStatus::new("app.jet", false);
        status.set_port(8123);
        assert_eq!(
            status.dashboard_detail_line(),
            "         watching app.jet · Canvas http://localhost:8123/canvas · v verbose"
        );
    }

    #[test]
    fn browser_client_registry_counts_tabs_not_poll_connections() {
        let status = DevStatus::new("app.jet", false);
        status.note_client("tab-a");
        status.note_client("tab-a");
        assert_eq!(status.client_count(), 1);
        status.note_client("tab-b");
        assert_eq!(status.client_count(), 2);
        status.note_client("");
        status.note_client(&"x".repeat(129));
        assert_eq!(status.client_count(), 2);
    }

    #[test]
    fn expired_browser_lease_drives_shared_reconnect_then_ready() {
        let status = DevStatus::new("app.jet", false);
        status.set_port(8123);
        status.note_client("tab-a");
        status.clients.lock().unwrap().insert(
            "tab-a".to_string(),
            std::time::Instant::now() - std::time::Duration::from_millis(super::CLIENT_TTL_MS + 1),
        );
        status.expire_clients();
        let reconnecting = status.json();
        assert!(reconnecting.contains("\"state\":\"reconnecting\""));
        assert!(reconnecting.contains("reconnecting · waiting for connection"));

        status.note_client("tab-a");
        let ready = status.json();
        assert!(ready.contains("\"state\":\"ready\""));
        assert!(ready.contains("ready · localhost:8123 · 1 client"));
    }

    #[test]
    fn reconnect_overrides_ready_building_and_error_without_losing_snapshot() {
        let status = DevStatus::new("app.jet", false);
        status.set_port(8123);
        status.mark_ready(425, false);
        status.reconnecting.store(true, Ordering::SeqCst);

        let ready = status.json();
        assert!(ready.contains("\"state\":\"reconnecting\""));
        assert!(ready.contains("\"last_build_ms\":425"));

        status.mark_building();
        let building = status.json();
        assert!(building.contains("\"state\":\"reconnecting\""));
        assert!(building.contains("\"last_build_ms\":425"));

        status.mark_error(
            "E0102".to_string(),
            "Error [E0102]: missing".to_string(),
            true,
        );
        let error = status.json();
        assert!(error.contains("\"state\":\"reconnecting\""));
        assert!(error.contains("\"code\":\"E0102\""));
        assert!(error.contains("Error [E0102]: missing"));
        assert!(error.contains("\"last_build_ms\":425"));

        status.reconnecting.store(false, Ordering::SeqCst);
        let recovered = status.json();
        assert!(recovered.contains("\"state\":\"error\""));
        assert!(recovered.contains("\"code\":\"E0102\""));
    }

    #[test]
    fn blocked_verbose_refresh_cannot_reinstall_region_after_disable() {
        let status =
            std::sync::Arc::new(DevStatus::new_with_terminal("app.jet", true, true, false));
        status.controls_ready.store(true, Ordering::SeqCst);
        let terminal_guard = status.term_lock.lock().unwrap();
        let waiter = std::sync::Arc::clone(&status);
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            waiter.write_header_verbose("jet dev  [ready] localhost:8123 · 1 client");
        });
        started_rx.recv().unwrap();

        // This is the disable+restore critical section: capability flips
        // while the stale renderer is blocked on the same terminal lock.
        status.controls_ready.store(false, Ordering::SeqCst);
        *status.header_started.lock().unwrap() = false;
        drop(terminal_guard);
        handle.join().unwrap();

        assert!(!status.controls_ready.load(Ordering::SeqCst));
        assert!(!*status.header_started.lock().unwrap());
    }

    // --- D-FE-DEVSRV1=D: parity words shared verbatim by terminal + browser ---

    #[test]
    fn header_words_ready_includes_port_clients_and_build_time() {
        let (word, rest) = header_words("ready", "app.jet", "", 8080, 2, 420);
        assert_eq!(word, "ready");
        assert_eq!(rest, "localhost:8080 · 2 clients · built 0.4s");
    }

    #[test]
    fn header_words_ready_omits_build_time_before_first_build() {
        let (word, rest) = header_words("ready", "app.jet", "", 8080, 0, 0);
        assert_eq!(word, "ready");
        assert_eq!(rest, "localhost:8080 · 0 clients");
    }

    #[test]
    fn header_words_building_shows_watched_file_not_port() {
        let (word, rest) = header_words("building", "app.jet", "", 8080, 1, 0);
        assert_eq!(word, "building");
        assert_eq!(rest, "app.jet · 1 client");
    }

    #[test]
    fn header_words_error_shows_diagnostic_code_not_port() {
        let (word, rest) = header_words("error", "app.jet", "E0102", 8080, 2, 0);
        assert_eq!(word, "error");
        assert_eq!(rest, "E0102 · 2 clients");
    }

    #[test]
    fn header_words_singular_client_count() {
        let (_, rest) = header_words("ready", "app.jet", "", 8080, 1, 0);
        assert!(rest.ends_with("1 client"), "{rest}");
        assert!(!rest.contains("1 clients"), "{rest}");
    }

    #[test]
    fn format_build_time_renders_seconds_with_one_decimal() {
        assert_eq!(format_build_time(420), "0.4s");
        assert_eq!(format_build_time(1500), "1.5s");
        assert_eq!(format_build_time(0), "0.0s");
    }

    #[test]
    fn format_line_colored_carries_a_dot_and_the_full_parity_words() {
        let line = format_line_colored("ready", "localhost:8080 · 2 clients · built 0.4s");
        assert!(line.starts_with("jet dev  "));
        assert!(line.contains('\u{25CF}'), "{line}");
        assert!(line.contains("ready · localhost:8080 · 2 clients · built 0.4s"));
    }

    #[test]
    fn format_line_plain_uses_bracketed_state_word_no_color() {
        let line = format_line_plain("error", "E0102 · 2 clients");
        assert_eq!(line, "jet dev  [error] E0102 · 2 clients");
        assert!(
            !line.contains('\x1b'),
            "NO_COLOR/CI floor must carry no ANSI: {line}"
        );
    }

    #[test]
    fn frame_lines_keeps_diagnostic_words_verbatim() {
        let diagnostic = "Error [E0102]: nothing named `nonexistent_function_xyz` exists here\n  8 | nonexistent_function_xyz()\n    | ^^^^^^^^^^^^^^^^^^^^^^^^\nWhy: only defined names can be called\nFix: define it first";
        let framed = frame_lines("E0102", diagnostic);
        // Every diagnostic line survives byte-for-byte inside the frame —
        // only a "│ " border is added, never a reworded/retruncated line (I4).
        for line in diagnostic.lines() {
            assert!(
                framed.iter().any(|f| f == &format!("│ {}", line)),
                "missing verbatim line {:?} in {:#?}",
                line,
                framed
            );
        }
        assert!(framed.first().unwrap().starts_with("┌ E0102 "));
        assert!(framed.last().unwrap().starts_with('└'));
    }

    #[test]
    fn frame_lines_top_border_names_the_diagnostic_code() {
        let framed = frame_lines("E0204", "one line");
        assert!(framed[0].contains("E0204"));
    }

    #[test]
    fn canvas_default_is_loopback_ephemeral_and_session_bearing() {
        let options = CanvasHostOptions::default();
        assert_eq!(options.host, "127.0.0.1");
        assert_eq!(options.port, None);
        assert_eq!(options.transport, "http");
        assert_eq!(options.authority, "loopback");
        let host = WebHost::bind_canvas("app.jet", false, None).unwrap();
        let url = host.canvas_url();
        assert!(url.starts_with("http://127.0.0.1:"), "{url}");
        assert!(url.contains("/canvas?session="), "{url}");
        assert!(
            url.len() > 80,
            "session secret should not be a short marker: {url}"
        );
        let address = host
            .listener
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .local_addr()
            .unwrap();
        assert!(
            address.ip().is_loopback(),
            "default Canvas address: {address}"
        );
        assert!(
            !address.ip().is_unspecified(),
            "default Canvas address: {address}"
        );
    }

    #[test]
    fn application_default_scans_stable_range_and_skips_held_port() {
        let first = bind_application_server(None).expect("first application preview port");
        let first_port = first.local_addr().unwrap().port();
        let second = bind_application_server(None).expect("next application preview port");
        let second_port = second.local_addr().unwrap().port();

        assert!(APPLICATION_PORT_RANGE.contains(&first_port));
        assert!(APPLICATION_PORT_RANGE.contains(&second_port));
        assert!(
            second_port > first_port,
            "application port scan did not skip the held port: {first_port}, {second_port}"
        );
    }

    #[test]
    fn canvas_sessions_get_distinct_ephemeral_ports_and_secrets() {
        let first = WebHost::bind_canvas("app.jet", false, None).unwrap();
        let second = WebHost::bind_canvas("app.jet", false, None).unwrap();

        assert_ne!(first.status.port(), second.status.port());
        assert_ne!(first.session_secret, second.session_secret);
    }

    #[test]
    fn canvas_explicit_collision_and_invalid_overrides_fail_loudly() {
        let first = WebHost::bind_canvas("app.jet", false, None).unwrap();
        let port = first.status.port();
        let mut options = CanvasHostOptions::default();
        options.port = Some(port);
        let collision = WebHost::bind_canvas_with_options("app.jet", false, &options)
            .err()
            .expect("a held explicit port must collide");
        assert!(
            collision.contains("couldn't bind Canvas host"),
            "{collision}"
        );

        options.transport = "udp".to_string();
        let transport = WebHost::bind_canvas_with_options("app.jet", false, &options)
            .err()
            .expect("unsupported transport must fail");
        assert!(transport.contains("transport"), "{transport}");

        options.transport = "http".to_string();
        options.authority = "public".to_string();
        let authority = WebHost::bind_canvas_with_options("app.jet", false, &options)
            .err()
            .expect("unknown authority must fail");
        assert!(authority.contains("authority"), "{authority}");
    }

    #[test]
    fn canvas_non_loopback_requires_explicit_remote_authority() {
        let mut options = CanvasHostOptions::default();
        options.host = "0.0.0.0".to_string();
        let error = WebHost::bind_canvas_with_options("app.jet", false, &options)
            .err()
            .expect("non-loopback Canvas must require explicit authority");
        assert!(error.contains("authority = remote"), "{error}");

        options.authority = "remote".to_string();
        let host = WebHost::bind_canvas_with_options("app.jet", false, &options)
            .expect("remote Canvas must bind only after explicit authority");
        let address = host
            .listener
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .local_addr()
            .unwrap();
        assert!(
            address.ip().is_unspecified(),
            "remote Canvas address: {address}"
        );
    }

    #[test]
    fn started_canvas_session_releases_its_port_on_shutdown() {
        let host = WebHost::bind_canvas("app.jet", false, None).unwrap();
        let port = host.status.port();
        host.start_canvas();
        drop(host);

        WebHost::bind_canvas("app.jet", false, Some(port))
            .expect("a shut down Canvas session must release its port");
    }

    #[test]
    fn concurrent_canvas_clients_share_checked_revision_boundary() {
        let root = std::env::temp_dir().join(format!(
            "jet-devserver-concurrent-canvas-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("main.jet");
        std::fs::write(&path, "fn run() {\n    total := 1\n    print(total)\n}\n").unwrap();
        let before = std::fs::read_to_string(&path).unwrap();
        let revision = crate::Canvas::source_revision(&before);
        let host = WebHost::bind_canvas(path.to_str().unwrap(), false, None).unwrap();
        let port = host.status.port();
        let secret = host.session_secret.clone();
        host.start_canvas();

        let barrier = Arc::new(Barrier::new(3));
        let responses = [
            ("client-a", "first"),
            ("client-b", "second"),
        ]
        .into_iter()
        .map(|(client, name)| {
            let barrier = Arc::clone(&barrier);
            let secret = secret.clone();
            let body = format!(
                "{{\"schema_version\":1,\"op\":\"rename_binding\",\"revision\":\"{revision}\",\"from\":\"total\",\"to\":\"{name}\",\"client_id\":\"{client}\"}}"
            );
            thread::spawn(move || {
                barrier.wait();
                post_canvas_transaction(port, &secret, client, &body)
            })
        })
        .collect::<Vec<_>>();
        barrier.wait();
        let responses = responses
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>();

        assert_eq!(
            responses
                .iter()
                .filter(|response| response.starts_with("HTTP/1.1 200 OK"))
                .count(),
            1,
            "exactly one client must publish the shared revision"
        );
        let loser = responses
            .iter()
            .find(|response| response.starts_with("HTTP/1.1 409 Conflict"))
            .expect("one client must receive a conflict");
        let current = std::fs::read_to_string(&path).unwrap();
        let current_revision = crate::Canvas::source_revision(&current);
        assert!(loser.contains("\"kind\":\"conflict\""), "{loser}");
        assert!(
            loser.contains(&format!("\"current_revision\":\"{current_revision}\"")),
            "{loser}"
        );
        let session = host.session.json();
        assert!(
            session.contains(&format!("\"accepted_revision\":\"{current_revision}\"")),
            "{session}"
        );
        assert!(session.contains("\"status\":\"accepted\""));
        assert!(session.contains("\"status\":\"refused\""));
        drop(host);
        let _ = std::fs::remove_dir_all(root);
    }

    fn post_canvas_transaction(port: u16, secret: &str, client: &str, body: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let request = format!(
            "POST /canvas/transaction?client_id={client} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://127.0.0.1:{port}\r\nAuthorization: Bearer {secret}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap();
        String::from_utf8(response).unwrap()
    }

    #[test]
    fn web_canvas_override_keeps_program_listener_separate() {
        let mut options = CanvasHostOptions::default();
        options.output = Some("service@local#debug".to_string());
        options.target = Some("board.browser".to_string());
        options.audit = true;
        let host = WebHost::bind_web_with_canvas_options("app.jet", false, None, &options)
            .expect("web Canvas host should bind independently");
        assert!(host.canvas_url().contains("/canvas?session="));
        assert!(host.status.verbose.load(Ordering::Relaxed));
        let session = host.session.json();
        assert!(session.contains("\"run\":{\"output\":\"service@local#debug\",\"target\":\"board.browser\"}"));
        assert!(session.contains("\"canvas\":{\"host\":\"127.0.0.1\""));
        assert!(!session.contains("\"application\":{\"host\":\"127.0.0.1\",\"port\":0"));
    }

    #[test]
    fn canvas_session_post_cannot_change_program_selection() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let status = Arc::new(DevStatus::new_with_terminal("app.jet", false, false, false));
        status.set_port(port);
        let debug_sessions = crate::Canvas::DebugSessions::default();
        let session = Arc::new(crate::ResidentDevSession::new("app.jet", port, 0));
        session.select_output_values(Some("cli-output"), Some("cli-target"));
        let secret = mint_session_secret().unwrap();
        let server = thread::spawn({
            let session = Arc::clone(&session);
            let secret = secret.clone();
            move || {
                let (stream, _) = listener.accept().unwrap();
                handle_connection(
                    stream,
                    &status,
                    &debug_sessions,
                    &session,
                    "app.jet",
                    ListenerKind::Canvas,
                    true,
                    "127.0.0.1",
                    &secret,
                )
                .unwrap();
            }
        });
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let body = r#"{"op":"select_output","output":"web","target":"browser"}"#;
        let request = format!(
            "POST /canvas/session HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://127.0.0.1:{port}\r\nAuthorization: Bearer {secret}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        std::io::Write::write_all(&mut client, request.as_bytes()).unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        server.join().unwrap();

        assert!(response.starts_with("HTTP/1.1 405 Method Not Allowed"), "{response}");
        assert!(session
            .json()
            .contains("\"run\":{\"output\":\"cli-output\",\"target\":\"cli-target\"}"));
    }

    #[test]
    fn canvas_request_gate_requires_secret_and_strict_request_context() {
        let secret = mint_session_secret().unwrap();
        let mut headers = HashMap::new();
        headers.insert("host".to_string(), "localhost:8123".to_string());
        headers.insert("origin".to_string(), "http://localhost:8123".to_string());
        headers.insert("authorization".to_string(), format!("Bearer {secret}"));
        let mut request = Request {
            method: "GET".to_string(),
            target: "/canvas/graph".to_string(),
            headers,
            body: Vec::new(),
        };
        assert!(canvas_api_path("/canvas/graph", "/canvas/graph"));
        assert!(canvas_api_path("/__jet_dev_status", "/__jet_dev_status"));
        assert!(canvas_request_authorized(
            &request,
            &request.target,
            "127.0.0.1",
            8123,
            &secret
        ));
        request.target = format!("/canvas/graph?session={secret}&session=wrong");
        assert!(!canvas_request_authorized(
            &request,
            &request.target,
            "127.0.0.1",
            8123,
            &secret
        ));
        request.target = "/canvas/graph".to_string();
        request.headers.remove("authorization");
        assert!(!canvas_request_authorized(
            &request,
            &request.target,
            "127.0.0.1",
            8123,
            &secret
        ));
        request.headers.insert(
            "authorization".to_string(),
            format!("Bearer {secret}"),
        );
        request.headers.remove("origin");
        assert!(!canvas_request_authorized(
            &request,
            &request.target,
            "127.0.0.1",
            8123,
            &secret
        ));
        request.headers.insert(
            "sec-fetch-site".to_string(),
            "same-origin".to_string(),
        );
        assert!(canvas_request_authorized(
            &request,
            &request.target,
            "127.0.0.1",
            8123,
            &secret
        ));
        request.target = format!("/canvas/graph?session={secret}");
        assert!(!canvas_request_authorized(
            &request,
            &request.target,
            "127.0.0.1",
            8123,
            &secret
        ));
        request.headers.insert(
            "origin".to_string(),
            "http://localhost:8123".to_string(),
        );
        request.headers.insert(
            "authorization".to_string(),
            format!("Basic {secret}"),
        );
        assert!(!canvas_request_authorized(
            &request,
            &request.target,
            "127.0.0.1",
            8123,
            &secret
        ));
        request.headers.insert(
            "authorization".to_string(),
            format!("Bearer {secret}"),
        );
        request.method = "PUT".to_string();
        assert!(!canvas_request_authorized(
            &request,
            &request.target,
            "127.0.0.1",
            8123,
            &secret
        ));
        request.method = "GET".to_string();
        request.target = "/canvas/not-a-route".to_string();
        assert!(!canvas_request_authorized(
            &request,
            &request.target,
            "127.0.0.1",
            8123,
            &secret
        ));
        request.target = "/canvas/graph".to_string();
        request.body = vec![0; crate::MAX_REQUEST_BODY_BYTES + 1];
        assert!(!canvas_request_authorized(
            &request,
            &request.target,
            "127.0.0.1",
            8123,
            &secret
        ));
        request.body.clear();
        assert!(!canvas_request_authorized(
            &request,
            &request.target,
            "127.0.0.1",
            8124,
            &secret
        ));
        assert!(!host_header_allowed("evil.test:8123", "127.0.0.1", 8123));
        assert!(!origin_allowed("http://evil.test:8123", "127.0.0.1", 8123));
        assert!(!constant_time_equal(&secret, "wrong"));
    }

    #[test]
    fn canvas_page_and_bootstrap_reject_missing_session() {
        let secret = "canvas-test-secret";
        let response = canvas_response("/canvas", secret);
        assert!(
            response.starts_with("HTTP/1.1 401 Unauthorized"),
            "{response}"
        );

        let response = canvas_response("/canvas/app.js", secret);
        assert!(
            response.starts_with("HTTP/1.1 401 Unauthorized"),
            "{response}"
        );

        let response = canvas_response(&format!("/canvas?session={secret}"), secret);
        assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
        assert!(
            response.contains(&format!("app.js?session={secret}&")),
            "{response}"
        );

        let response = canvas_response(&format!("/canvas/app.js?session={secret}"), secret);
        assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    }

    fn canvas_response(target: &str, secret: &str) -> String {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        let raw =
            format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
        std::io::Write::write_all(&mut client, raw.as_bytes()).unwrap();

        let status = DevStatus::new_with_terminal("app.jet", false, false, false);
        status.set_port(port);
        let session =
            crate::ResidentDevSession::new_with_canvas_host("app.jet", "127.0.0.1", port, 0);
        let debug_sessions = crate::Canvas::DebugSessions::default();
        handle_connection(
            server,
            &status,
            &debug_sessions,
            &session,
            "app.jet",
            ListenerKind::Canvas,
            true,
            "127.0.0.1",
            secret,
        )
        .unwrap();

        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        response
    }

    #[test]
    fn canvas_session_injection_only_changes_html_bootstrap() {
        let html = crate::canvas_asset("GET", "/canvas", "/canvas").unwrap();
        let html = inject_canvas_session(html, "secret");
        assert!(html.body.contains("app.js?session=secret&"));

        let js = crate::canvas_asset("GET", "/canvas/app.js", "/canvas/app.js").unwrap();
        let js = inject_canvas_session(js, "secret");
        assert!(!js.body.contains("session=secret"));
    }
}
