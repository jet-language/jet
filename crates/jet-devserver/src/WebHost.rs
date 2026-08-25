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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use jet_driver::Diagnostics::ColorChoice;
use jet_foundation::JSON::json_escape;

use crate::{content_type_for, query_param, static_path, write_response, Request};

/// Ports tried, in order, before giving up. 8080 is the conventional static-
/// dev-server port; a small bounded scan upward covers "something else is
/// already on 8080" without hunting indefinitely (judgment call — 10 ports is
/// generous for a dev tool binding localhost).
const PORT_RANGE: std::ops::RangeInclusive<u16> = 8080..=8089;
const CANVAS_SESSION_BYTES: usize = 32;

/// How often the live-reload script in the browser polls `/__jet_dev_version`
/// The browser waits 750ms after each completed request before polling again.
/// That keeps reload responsive while leaving a real network-idle window for
/// user interactions and browser automation.
const LIVE_RELOAD_POLL_MS: u64 = 750;
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
    /// Set once, right after `bind_dev_server` — 0 until then.
    port: AtomicU64,
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
        format!(
            "         watching {} · Canvas http://localhost:{}/canvas · v verbose",
            self.watched_file,
            self.port()
        )
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
}

impl Default for CanvasHostOptions {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: None,
            transport: "http".to_string(),
            authority: "loopback".to_string(),
            audit: false,
        }
    }
}

pub struct WebHost {
    listener: Mutex<Option<TcpListener>>,
    application_listener: Mutex<Option<TcpListener>>,
    status: Arc<DevStatus>,
    debug_sessions: Arc<crate::Canvas::DebugSessions>,
    session: Arc<crate::ResidentDevSession>,
    canvas_file: String,
    canvas_only: bool,
    bind_host: String,
    session_secret: String,
}

impl WebHost {
    pub fn bind(file: &str, verbose: bool, port: Option<u16>) -> Result<Self, String> {
        let listener = bind_dev_server(port)?;
        let bound_port = listener
            .local_addr()
            .map(|address| address.port())
            .unwrap_or(*PORT_RANGE.start());
        let application_listener = bind_application_server()?;
        let application_port = application_listener
            .local_addr()
            .map(|address| address.port())
            .unwrap_or(0);
        let status = Arc::new(DevStatus::new(file, verbose));
        status.set_port(bound_port);
        let session_secret = mint_session_secret()?;
        Ok(Self {
            listener: Mutex::new(Some(listener)),
            application_listener: Mutex::new(Some(application_listener)),
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
        let mut effective = options.clone();
        if effective.port.is_none() {
            effective.port = fallback_port;
        }
        let listener = bind_canvas_server(&host, effective.port)?;
        let bound_port = listener
            .local_addr()
            .map(|address| address.port())
            .unwrap_or(0);
        let application_listener = bind_application_server()?;
        let application_port = application_listener
            .local_addr()
            .map(|address| address.port())
            .unwrap_or(0);
        let status = Arc::new(DevStatus::new(file, verbose || options.audit));
        status.set_port(bound_port);
        let session_secret = mint_session_secret()?;
        Ok(Self {
            listener: Mutex::new(Some(listener)),
            application_listener: Mutex::new(Some(application_listener)),
            status,
            debug_sessions: Arc::new(crate::Canvas::DebugSessions::default()),
            session: Arc::new(crate::ResidentDevSession::new_with_canvas_host(
                file,
                &host,
                bound_port,
                application_port,
            )),
            canvas_file: file.to_string(),
            canvas_only: false,
            bind_host: host,
            session_secret,
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
        Ok(Self {
            listener: Mutex::new(Some(listener)),
            application_listener: Mutex::new(None),
            status,
            debug_sessions: Arc::new(crate::Canvas::DebugSessions::default()),
            session: Arc::new(crate::ResidentDevSession::new_with_canvas_host(
                file, &host, bound_port, 0,
            )),
            canvas_file: file.to_string(),
            canvas_only: true,
            bind_host: host,
            session_secret,
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
        let listener = self.listener.lock().unwrap().take();
        if let Some(listener) = listener {
            let status = Arc::clone(&self.status);
            let debug_sessions = Arc::clone(&self.debug_sessions);
            let session = Arc::clone(&self.session);
            let canvas_file = self.canvas_file.clone();
            let canvas_only = self.canvas_only;
            let bind_host = self.bind_host.clone();
            let session_secret = self.session_secret.clone();
            thread::spawn(move || {
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
                )
            });
        }
        if let Some(listener) = self.application_listener.lock().unwrap().take() {
            let status = Arc::clone(&self.status);
            let debug_sessions = Arc::clone(&self.debug_sessions);
            let session = Arc::clone(&self.session);
            let canvas_file = self.canvas_file.clone();
            thread::spawn(move || {
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
                )
            });
        }
        {
            let status = Arc::clone(&self.status);
            thread::spawn(move || loop {
                thread::sleep(Duration::from_millis(LIVE_RELOAD_POLL_MS));
                status.activate_requested_browser_trace();
                status.expire_clients();
            });
        }
        if self.canvas_only || !terminal_controls {
            println!("Canvas: {}", self.canvas_url());
        } else if terminal_controls && !self.status.pin {
            println!(
                "serving Canvas http://localhost:{} — app preview http://localhost:{} — watching {} … (Ctrl-C to stop)",
                self.status.port(),
                self.session_application_port(),
                self.canvas_file
            );
            println!("Canvas: {}", self.canvas_url());
            println!(
                "App preview: http://localhost:{}/",
                self.session_application_port()
            );
        }
        if terminal_controls {
            start_terminal_controls(Arc::clone(&self.status));
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

fn start_terminal_controls(status: Arc<DevStatus>) {
    if !status.pin {
        return;
    }
    let Some(raw) = jet_repl::Term::RawGuard::enable() else {
        return;
    };
    status.controls_ready.store(true, Ordering::SeqCst);
    thread::spawn(move || {
        let mut keys = jet_repl::Term::KeyReader::new(std::io::stdin());
        loop {
            match keys.read_key() {
                jet_repl::Term::Key::Char('v' | 'V') => status.toggle_verbose(),
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
    });
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
    for name in MAP_FILES {
        let src = staging.join(name);
        let dst = out_dir.join(name);
        if src.exists() {
            rename_with_retry(&src, &dst)?;
        } else if dst.exists() {
            // Release (or map-less) rebuild: drop stale maps with the swap.
            let _ = fs::remove_file(&dst);
        }
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
fn bind_dev_server(port: Option<u16>) -> Result<TcpListener, String> {
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
                    "error: couldn't bind to port {}: {}{}",
                    port, e, fix
                ))
            }
        };
    }
    let mut last_err: Option<std::io::Error> = None;
    for port in PORT_RANGE {
        match TcpListener::bind(("127.0.0.1", port)) {
            Ok(listener) => return Ok(listener),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                last_err = Some(e);
                continue;
            }
            Err(e) => {
                return Err(format!("error: couldn't start the dev server: {}", e));
            }
        }
    }
    Err(format!(
        "error: every port from {} to {} is already in use{}\n fix: free one of those ports, stop the other process using it, or pick one explicitly with --port=<N>",
        PORT_RANGE.start(),
        PORT_RANGE.end(),
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
    (matches!(
        path,
        "/__jet_dev_version"
            | "/__jet_dev_status"
            | "/__jet_dev_disconnect"
            | "/__jet_perf_browser"
            | "/__jet_canvas/live"
    )
        || (path.starts_with("/__jet_canvas/") && !path.ends_with("/app.js"))
        || (path.starts_with("/canvas/") && !path.ends_with("/app.js"))
        || (path.starts_with("/panel/") && !path.ends_with("/app.js")))
        || (path == "/" && query_param(target, "jet_panel_graph").as_deref() == Some("1"))
}

fn canvas_request_authorized(
    request: &Request,
    _target: &str,
    bind_host: &str,
    port: u16,
    session_secret: &str,
) -> bool {
    let Some(host) = request.headers.get("host") else {
        return false;
    };
    if !host_header_allowed(host, bind_host, port) {
        return false;
    }
    if let Some(origin) = request.headers.get("origin") {
        if !origin_allowed(origin, bind_host, port) {
            return false;
        }
    }
    let Some(authorization) = request.headers.get("authorization") else {
        return false;
    };
    let Some(token) = authorization.strip_prefix("Bearer ") else {
        return false;
    };
    constant_time_equal(token, session_secret)
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

fn bind_application_server() -> Result<TcpListener, String> {
    TcpListener::bind(("127.0.0.1", 0))
        .map_err(|error| format!("error: couldn't start the application listener: {error}"))
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
) {
    for stream in listener.incoming() {
        if let Ok(stream) = stream {
            let status = Arc::clone(&status);
            let debug_sessions = Arc::clone(&debug_sessions);
            let session = Arc::clone(&session);
            let canvas_file = canvas_file.clone();
            let bind_host = bind_host.clone();
            let session_secret = session_secret.clone();
            thread::spawn(move || {
                let _ = handle_connection(
                    stream,
                    &status,
                    &debug_sessions,
                    &session,
                    &canvas_file,
                    listener_kind,
                    canvas_only,
                    &bind_host,
                    &session_secret,
                );
            });
        }
    }
}

/// Parse one GET request line + headers (no keep-alive, no request body —
/// this is a dev tool, not a production server) and serve a response.
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
    let mut reader = BufReader::new(stream.try_clone()?);
    let Some(request) = Request::read(&mut reader)? else {
        return Ok(());
    };

    let mut stream = stream;
    let method = request.method.as_str();
    let target = request.target.as_str();
    let body = request.body.as_slice();

    if method != "GET" && method != "POST" {
        return write_response(
            &mut stream,
            "405 Method Not Allowed",
            "text/plain; charset=utf-8",
            b"jet dev's web server only handles GET and Canvas POST requests",
        );
    }

    let path = target.split('?').next().unwrap_or("/");
    if listener_kind == ListenerKind::Canvas
        && canvas_api_path(path, target)
        && !canvas_request_authorized(&request, target, bind_host, status.port(), session_secret)
    {
        return unauthorized(&mut stream);
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
        let code = serve_static(&mut stream, path, &nonce)?;
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
        if method != "POST" {
            return method_not_allowed(&mut stream);
        }
        let request = String::from_utf8_lossy(&body);
        session.select_output(&request);
        return write_response(
            &mut stream,
            "200 OK",
            "application/json; charset=utf-8",
            session_response(session).as_bytes(),
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
        return match crate::LiveInspect::read(pid) {
            Ok(snapshot) => write_response(
                &mut stream,
                "200 OK",
                "application/json; charset=utf-8",
                snapshot.as_bytes(),
            ),
            Err(message) => write_response(
                &mut stream,
                "404 Not Found",
                "text/plain; charset=utf-8",
                message.as_bytes(),
            ),
        };
    }
    if let Some(asset) = crate::canvas_asset(method, target, path) {
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
            Ok(body) => write_response(
                &mut stream,
                "200 OK",
                "application/json; charset=utf-8",
                with_session(body, session).as_bytes(),
            ),
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
        return match graph {
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
                session.record_debug(&request);
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
    let code = serve_static(&mut stream, path, &nonce)?;
    status.log_request(method, path, code, started.elapsed());
    Ok(())
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
    let source_path = jet_foundation::JSON::parse_json(request)
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
        .and_then(|source_id| {
            crate::Canvas::project_path_for_source_id(Path::new(canvas_file), &source_id)
        })
        .unwrap_or_else(|| PathBuf::from(canvas_file));
    fs::read_to_string(source_path)
        .map(|source| crate::Canvas::source_revision(&source))
        .unwrap_or_default()
}

/// Serve one file out of `build/`, injecting the live-reload script into
/// `index.html` on the way out. GET-only, no directory listing, no range
/// requests, no compression — a dev tool serving a few small files needs
/// none of that.
fn serve_static(stream: &mut TcpStream, path: &str, nonce: &str) -> std::io::Result<u16> {
    let file_path = match static_path(Path::new("build"), path) {
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
    if file_path.file_name().and_then(|f| f.to_str()) == Some("index.html") {
        let html = String::from_utf8_lossy(&bytes).into_owned();
        let injected = inject_live_reload(&html, nonce);
        write_response(stream, "200 OK", content_type, injected.as_bytes())?;
        return Ok(200);
    }
    write_response(stream, "200 OK", content_type, &bytes)?;
    Ok(200)
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
        inject_live_reload, mint_session_secret, origin_allowed, CanvasHostOptions, DevStatus,
        Ordering, WebHost,
    };
    use crate::Request;
    use std::collections::HashMap;

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
        assert!(out.contains("finally(function () { setTimeout(poll, 750); })"));
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
    fn web_canvas_override_keeps_program_listener_separate() {
        let options = CanvasHostOptions::default();
        let host = WebHost::bind_web_with_canvas_options("app.jet", false, None, &options)
            .expect("web Canvas host should bind independently");
        assert!(host.canvas_url().contains("/canvas?session="));
        let session = host.session.json();
        assert!(session.contains("\"canvas\":{\"host\":\"127.0.0.1\""));
        assert!(!session.contains("\"application\":{\"host\":\"127.0.0.1\",\"port\":0"));
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
        request.headers.remove("authorization");
        assert!(!canvas_request_authorized(
            &request,
            &request.target,
            "127.0.0.1",
            8123,
            &secret
        ));
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
}
