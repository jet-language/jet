//! D-DBG3 step 2 (dap-debugger): drives a real `lldb` process over piped
//! stdin/stdout (I6 — no debugger library, no MI/JSON crate; std `process` pipes
//! only). This is the ONLY module that shells out to `lldb`; every caller (the
//! native `(jet)` prompt and the DAP adapter) goes through [`Inferior`] and never
//! spawns lldb itself.
//!
//! lldb is a runtime TOOL dependency on the user's machine, not an I6 compiler
//! crate (same posture as nixpkgs for native/system deps) — [`Inferior::available`]
//! gates every call site so a missing lldb degrades to a clear message pointing at
//! the step-1 interpreter debugger, never a panic.
//!
//! Two lldb quirks this module works around, both confirmed against a live
//! lldb (not just documentation — see the sidequest doc for the trace):
//! 1. Driven over a plain pipe (no TTY), lldb never re-prints a bare `(lldb) `
//!    "ready" prompt between commands — it only echoes `(lldb) <the command
//!    just read>` as a transcript marker. Waiting for a bare prompt to
//!    reappear hangs forever. Every command is instead followed by a bogus
//!    sentinel command (its rejection is deterministic and unique); reads run
//!    until the sentinel's own echo appears.
//! 2. A resuming command's (`run`/`continue`/step) own stop banner is printed
//!    by an asynchronous event listener that can race with — and lose to — the
//!    NEXT command's synchronous reply, so parsing the resume command's own
//!    returned text for frame info is unreliable. [`Self::resume_and_locate`]
//!    instead sends the resume command immediately followed by `bt` in the
//!    SAME write; `bt` is synchronous and lldb's command queue guarantees it
//!    runs only once the resume has fully settled (stopped or exited), so its
//!    reply is always accurate.
//! 3. By default the debuggee INHERITS lldb's own stdout/stderr, so a Jet
//!    `print()` can land byte-interleaved into the middle of lldb's own
//!    sentinel echo (confirmed live: a `total is 6` program print tore the
//!    sentinel in half, `__` + `total is 6\n` + the rest, hanging the sentinel
//!    search). `Self::spawn` redirects the debuggee's stdout/stderr to their
//!    own temp files via `settings set target.output-path`/`error-path`
//!    BEFORE the first `run`, so lldb's control channel and the debuggee's
//!    program output never share a byte stream; [`Self::drain_program_output`]
//!    reads what's new since the last check.
//!
//! Phase 1 targets lldb on Linux/macOS (the plan's stated scope); gdb and Windows
//! are follow-ups, not silently pretended here.

use jet_foundation::Names::mangle;
use jet_foundation::{Syntax, SHA256};
use std::fs::OpenOptions;
use std::io::{self, Read, Seek, SeekFrom, Write};
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};
/// Bogus command sent after every real one. lldb echoes then rejects it
/// (`error: '<sentinel>' is not a valid command.`, to stderr, which we null —
/// see the module doc point 1); its echo appearing on stdout is the ONLY
/// reliable "the real command's output is fully flushed" signal over a pipe.
const SENTINEL: &str = "__jet_dbg_sentinel_9f3c__";

/// How long to wait for the sentinel to come back before giving up (a dead
/// lldb, or a resume command that never stops) — generous, since a debuggee
/// can legitimately run for a while before hitting a breakpoint or exiting.
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// After the sentinel for a resuming command, how long to wait for the
/// asynchronous exit notification that can lag behind it (see
/// `resume_and_locate`'s doc) — short, since it's confirmed to arrive within
/// milliseconds live; this is not a "maybe it'll show up eventually" timeout.
const EXIT_GRACE_WINDOW: Duration = Duration::from_millis(300);

/// One resolved stop location: the function lldb reports plus its raw Rust
/// file:line. Callers translate `rust_line` through [`super::LineMap::LineMap`]
/// to get the Jet line (I2) — this struct never claims to be Jet-terms itself.
pub(crate) struct RawFrame {
    pub func: String,
    pub rust_file: String,
    pub rust_line: usize,
}

pub(crate) struct Breakpoint {
    pub id: usize,
    pub resolved: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ThreadInfo {
    pub id: u32,
    pub name: String,
}

/// What a resuming command (`run`/`continue`/a step) settled into — see the
/// module doc point 2 for why this is derived from a follow-up `bt`, never
/// from the resume command's own text.
pub(crate) enum ResumeResult {
    /// Stopped at a frame; the raw `bt` reply (parse with
    /// [`Inferior::parse_top_frame`]/[`Inferior::parse_frames`]).
    Stopped(String),
    /// The debuggee ran to completion.
    Exited {
        status: Option<i32>,
        signal: Option<String>,
    },
}

/// Identity held across the attach handshake. A PID alone is not an
/// authority: it can be reused, point at a replaced executable, or belong to
/// another session. Linux's open `/proc/<pid>` directory keeps the post-attach
/// checks anchored to the process that passed preflight.
#[cfg(target_os = "linux")]
struct TargetIdentity {
    pid: u32,
    proc_dir: std::fs::File,
    executable: PathBuf,
    executable_hash: String,
    uid: u32,
    session: u64,
    start_time: u64,
}

#[cfg(not(target_os = "linux"))]
struct TargetIdentity;

pub(crate) struct Inferior {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<u8>,
    _reader: Option<std::thread::JoinHandle<()>>,
    stdout_path: PathBuf,
    stderr_path: PathBuf,
    /// Bytes already drained from each redirected file (see module doc point 3).
    stdout_pos: u64,
    stderr_pos: u64,
    attached: Option<TargetIdentity>,
    detached: bool,
    debuggee_exited: bool,
    closed: bool,
}

impl Inferior {
    /// Whether `lldb` is on PATH. Call this before ever constructing an
    /// `Inferior` — every entry point into the native backend gates on it.
    pub(crate) fn available() -> bool {
        Command::new("lldb")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Launch `lldb` against `binary` (built with debug symbols) and consume its
    /// startup banner. The process is NOT running yet — call [`Self::resume_and_locate`]
    /// with `"run"` to start it.
    ///
    /// Loads rustc's OWN Rust pretty-printer setup (the same two files
    /// `rust-lldb` loads) instead of shelling out to `rust-lldb` itself — same
    /// effect, one fewer process hop, and `--no-lldbinit` still suppresses any
    /// user config. TWO files, confirmed live both are required: importing
    /// just `lldb_lookup.py` (`command script import`) defines the summary
    /// FUNCTIONS but registers none of them — `frame variable` still printed a
    /// `String` as its raw multi-field allocator layout. `lldb_commands`'s
    /// `type summary add ... --category Rust` + `type category enable Rust`
    /// lines are what actually turn them on.
    pub(crate) fn spawn(binary: &Path) -> std::io::Result<Inferior> {
        if !binary.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("debug binary does not exist: {}", binary.display()),
            ));
        }
        let mut cmd = Command::new("lldb");
        cmd.arg("--no-lldbinit");
        if let Some((script, commands)) = rust_pretty_printer_files() {
            if let Ok(script) = checked_lldb_quote(&script.to_string_lossy()) {
                cmd.arg("--one-line-before-file")
                    .arg(format!("command script import {script}"))
                    .arg("--source-before-file")
                    .arg(commands);
            }
        }
        let mut child = cmd
            .arg("--")
            .arg(binary)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = child.stdin.take().expect("piped stdin");
        let mut stdout: ChildStdout = child.stdout.take().expect("piped stdout");
        // A background reader thread turns the blocking stdout pipe into a
        // channel of bytes, so `read_until_sentinel` can wait WITH a timeout —
        // plain `Read` has no timeout in std.
        let (tx, rx) = std::sync::mpsc::channel::<u8>();
        let reader = std::thread::spawn(move || {
            let mut byte = [0u8; 1];
            loop {
                match stdout.read(&mut byte) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if tx.send(byte[0]).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        // c144/D-DBG3 step 2: separate the debuggee's own stdout/stderr from
        // lldb's control channel BEFORE the first `run` (module doc point 3) —
        // one temp file per stream, unique per session (pid + a `spawn`-time
        // counter so two sessions in the same process, e.g. in tests, never share).
        static SESSION_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = SESSION_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp = std::env::temp_dir();
        let stdout_path = capture_path(&tmp, n, "stdout");
        if let Err(error) = create_capture_file(&stdout_path) {
            cleanup_failed_start(child, stdin, rx, reader, [stdout_path.as_path()].as_slice());
            return Err(error);
        }
        let stderr_path = capture_path(&tmp, n, "stderr");
        if let Err(error) = create_capture_file(&stderr_path) {
            cleanup_failed_start(
                child,
                stdin,
                rx,
                reader,
                [stdout_path.as_path(), stderr_path.as_path()].as_slice(),
            );
            return Err(error);
        }

        let mut inf = Inferior {
            child,
            stdin,
            rx,
            _reader: Some(reader),
            stdout_path,
            stderr_path,
            stdout_pos: 0,
            stderr_pos: 0,
            attached: None,
            detached: false,
            debuggee_exited: false,
            closed: false,
        };
        inf.write_lines(&[])?;
        let out_path = match checked_lldb_quote(&inf.stdout_path.to_string_lossy()) {
            Ok(path) => path,
            Err(error) => {
                inf.shutdown();
                return Err(error);
            }
        };
        let err_path = match checked_lldb_quote(&inf.stderr_path.to_string_lossy()) {
            Ok(path) => path,
            Err(error) => {
                inf.shutdown();
                return Err(error);
            }
        };
        let out_setting = format!("settings set target.output-path {out_path}");
        let err_setting = format!("settings set target.error-path {err_path}");
        inf.write_lines(&[&out_setting, &err_setting])?;
        Ok(inf)
    }

    /// Attach only to a local same-user process running the requested binary.
    /// Remote and cross-executable attach stay outside the local DAP profile.
    pub(crate) fn attach(binary: &Path, pid: u32) -> std::io::Result<Inferior> {
        if pid == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "attach processId must be positive",
            ));
        }
        let identity = verify_attach_target(binary, pid)?;
        let mut cmd = Command::new("lldb");
        cmd.arg("--no-lldbinit");
        if let Some((script, commands)) = rust_pretty_printer_files() {
            if let Ok(script) = checked_lldb_quote(&script.to_string_lossy()) {
                cmd.arg("--one-line-before-file")
                    .arg(format!("command script import {script}"))
                    .arg("--source-before-file")
                    .arg(commands);
            }
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = child.stdin.take().expect("piped stdin");
        let mut stdout: ChildStdout = child.stdout.take().expect("piped stdout");
        let (tx, rx) = std::sync::mpsc::channel::<u8>();
        let reader = std::thread::spawn(move || {
            let mut byte = [0u8; 1];
            loop {
                match stdout.read(&mut byte) {
                    Ok(0) | Err(_) => break,
                    Ok(_) if tx.send(byte[0]).is_err() => break,
                    Ok(_) => {}
                }
            }
        });
        static SESSION_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = SESSION_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp = std::env::temp_dir();
        let stdout_path = capture_path(&tmp, n, "stdout");
        if let Err(error) = create_capture_file(&stdout_path) {
            cleanup_failed_start(child, stdin, rx, reader, [stdout_path.as_path()].as_slice());
            return Err(error);
        }
        let stderr_path = capture_path(&tmp, n, "stderr");
        if let Err(error) = create_capture_file(&stderr_path) {
            cleanup_failed_start(
                child,
                stdin,
                rx,
                reader,
                [stdout_path.as_path(), stderr_path.as_path()].as_slice(),
            );
            return Err(error);
        }
        let mut inf = Inferior {
            child,
            stdin,
            rx,
            _reader: Some(reader),
            stdout_path,
            stderr_path,
            stdout_pos: 0,
            stderr_pos: 0,
            attached: Some(identity),
            detached: false,
            debuggee_exited: false,
            closed: false,
        };
        if let Err(error) = inf.write_lines(&[]) {
            inf.shutdown();
            return Err(error);
        }
        let out_setting = format!(
            "settings set target.output-path {}",
            checked_lldb_quote(&inf.stdout_path.to_string_lossy())?
        );
        let err_setting = format!(
            "settings set target.error-path {}",
            checked_lldb_quote(&inf.stderr_path.to_string_lossy())?
        );
        if let Err(error) = inf.write_lines(&[&out_setting, &err_setting]) {
            inf.shutdown();
            return Err(error);
        }
        let output = match inf.cmd(&format!("process attach --pid {pid}")) {
            Ok(output) => output,
            Err(error) => {
                inf.shutdown();
                return Err(error);
            }
        };
        if output.contains("error:") || output.contains("cannot attach") {
            let error = io::Error::new(
                io::ErrorKind::PermissionDenied,
                clean_lldb_error(&output, "process attach was denied"),
            );
            inf.shutdown();
            return Err(error);
        }
        if let Err(error) = inf
            .attached
            .as_ref()
            .map(TargetIdentity::verify)
            .transpose()
        {
            inf.shutdown();
            return Err(error);
        }
        Ok(inf)
    }

    /// New bytes written to the debuggee's redirected stdout/stderr since the
    /// last call (module doc point 3) — call after every [`Self::resume_and_locate`]
    /// so a Jet `print()`/`eprint()` shows up in the `(jet)` transcript /
    /// DAP `output` event the same run it happened.
    pub(crate) fn drain_program_output(&mut self) -> (String, String) {
        (
            Self::drain_file(&self.stdout_path, &mut self.stdout_pos),
            Self::drain_file(&self.stderr_path, &mut self.stderr_pos),
        )
    }

    fn drain_file(path: &Path, pos: &mut u64) -> String {
        let Ok(mut f) = std::fs::File::open(path) else {
            return String::new();
        };
        if f.seek(SeekFrom::Start(*pos)).is_err() {
            return String::new();
        }
        let mut buf = String::new();
        if f.read_to_string(&mut buf).is_ok() {
            *pos += buf.len() as u64;
        }
        buf
    }

    /// Write `lines` (each a full lldb command) then the sentinel, and read
    /// until the sentinel's own echo appears. Bounded by [`READ_TIMEOUT`], so a
    /// dead/hung lldb returns whatever was captured instead of hanging the
    /// session forever.
    fn write_lines(&mut self, lines: &[&str]) -> std::io::Result<String> {
        for l in lines {
            writeln!(self.stdin, "{}", l)?;
        }
        writeln!(self.stdin, "{}", SENTINEL)?;
        self.stdin.flush()?;
        let mut buf: Vec<u8> = Vec::new();
        let deadline = Instant::now() + READ_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "lldb did not finish the debugger command before timeout",
                ));
            }
            let byte = match self.rx.recv_timeout(remaining) {
                Ok(b) => b,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "lldb did not finish the debugger command before timeout",
                    ));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "lldb closed the debugger control channel",
                    ));
                }
            };
            buf.push(byte);
            if sentinel_echo_at_end(&buf) {
                break; // The sentinel's echo is on stdout; its rejection went
                       // to stderr, which is null — nothing more to consume.
            }
        }
        // Drop the sentinel's own echo from the returned text (the trailing
        // `\n(lldb) __jet_dbg_sentinel_9f3c__` line) so callers see only the
        // real command(s)' output.
        let text = String::from_utf8_lossy(&buf).into_owned();
        let cut = text
            .rfind(&format!("(lldb) {}", SENTINEL))
            .unwrap_or(text.len());
        Ok(text[..cut].to_string())
    }

    /// Send one lldb command; return everything it printed. For a RESUMING
    /// command (`run`/`continue`/a step) use [`Self::resume_and_locate`]
    /// instead — see the module doc point 2.
    pub(crate) fn cmd(&mut self, line: &str) -> std::io::Result<String> {
        self.write_lines(&[line])
    }

    /// Drain whatever arrives within [`EXIT_GRACE_WINDOW`] with no new input
    /// sent — used ONLY by `resume_and_locate` to catch a delayed exit
    /// notification (see its doc). Nothing is lost if the window is too
    /// short: this just widens the window `resume_and_locate` checks
    /// `full.contains("exited with status")` over.
    fn drain_grace_window(&mut self) -> String {
        let mut extra: Vec<u8> = Vec::new();
        while let Ok(b) = self.rx.recv_timeout(EXIT_GRACE_WINDOW) {
            extra.push(b);
        }
        String::from_utf8_lossy(&extra).into_owned()
    }

    /// Send a resuming command (`run`, `continue`, a step) immediately
    /// followed by `bt`, in the SAME write, and derive the outcome from `bt`'s
    /// reply — never from the resume command's own text (module doc point 2).
    ///
    /// A THIRD race, distinct from point 2: when the debuggee EXITS (rather
    /// than stopping), `bt` doesn't need to wait for anything — it fails fast
    /// ("no running process") — so it can print (and the sentinel can be seen)
    /// BEFORE lldb's separate async event listener gets around to printing
    /// `Process N exited with status = …`. Confirmed live: that line can
    /// arrive AFTER the sentinel's own echo. So after the sentinel, this waits
    /// one short grace window for anything still queued before deciding.
    pub(crate) fn resume_and_locate(&mut self, resume_cmd: &str) -> std::io::Result<ResumeResult> {
        let mut full = self.write_lines(&[resume_cmd, "bt"])?;
        full.push_str(&self.drain_grace_window());
        if full.contains("exited with") {
            self.debuggee_exited = true;
            return Ok(ResumeResult::Exited {
                status: parse_exit_status(&full),
                signal: parse_exit_signal(&full),
            });
        }
        // Everything after the LAST `(lldb) bt` echo is `bt`'s own reply —
        // guaranteed to run (and thus print) only once the resume has fully
        // settled, since lldb's command queue is strictly in-order.
        let bt_reply = full
            .rsplit("(lldb) bt\n")
            .next()
            .unwrap_or(full.as_str())
            .to_string();
        Ok(ResumeResult::Stopped(bt_reply))
    }

    /// `breakpoint set -f <file> -l <line>` — set on the RUST file/line (already
    /// translated from a Jet line by the caller via `LineMap::rust_line_for`).
    pub(crate) fn set_breakpoint(
        &mut self,
        rust_file: &str,
        rust_line: usize,
    ) -> std::io::Result<Breakpoint> {
        let out = self.cmd(&format!(
            "breakpoint set -f {} -l {}",
            checked_lldb_quote(rust_file)?,
            rust_line
        ))?;
        parse_breakpoint(&out).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "lldb did not return a breakpoint id",
            )
        })
    }

    pub(crate) fn delete_breakpoint(&mut self, id: usize) -> std::io::Result<()> {
        self.cmd(&format!("breakpoint delete {}", id))?;
        Ok(())
    }

    pub(crate) fn configure_launch(
        &mut self,
        args: &[String],
        cwd: Option<&str>,
        env: &[(String, String)],
    ) -> std::io::Result<()> {
        if !args.is_empty() {
            let rendered = args
                .iter()
                .map(|arg| checked_lldb_quote(arg))
                .collect::<io::Result<Vec<_>>>()?
                .join(" ");
            self.cmd(&format!("settings set target.run-args {rendered}"))?;
        }
        if let Some(cwd) = cwd {
            self.cmd(&format!(
                "settings set target.process.cwd {}",
                checked_lldb_quote(cwd)?
            ))?;
        }
        let mut assignments = Vec::with_capacity(env.len());
        for (key, value) in env {
            if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("invalid launch environment key `{key}`"),
                ));
            }
            assignments.push(checked_lldb_quote(&format!("{key}={value}"))?);
        }
        if !assignments.is_empty() {
            self.cmd(&format!(
                "settings set target.env-vars {}",
                assignments.join(" ")
            ))?;
        }
        Ok(())
    }

    /// `frame variable` — every local in the current frame, `(type) name = value`
    /// per line (parsed by [`Self::parse_typed_locals`]).
    pub(crate) fn locals(&mut self) -> std::io::Result<String> {
        self.cmd("frame variable")
    }

    /// `frame variable <name>` — a single local by its JET name (translated to
    /// the mangled Rust name lldb needs — see [`jet_local_to_rust`]).
    pub(crate) fn print_var(&mut self, jet_name: &str) -> std::io::Result<String> {
        self.frame_variable(&Self::jet_local_to_rust(jet_name))
    }

    pub(crate) fn frame_variable(&mut self, expression: &str) -> std::io::Result<String> {
        if !valid_frame_expression(expression) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "debugger expression is not a read-only local path",
            ));
        }
        self.cmd(&format!("frame variable {expression}"))
    }

    /// `bt` — the full native call stack (parsed by [`Self::parse_frames`]).
    /// Safe to call standalone (not after a resume) since the debuggee is
    /// already stopped by the time the caller asks.
    pub(crate) fn backtrace(&mut self) -> std::io::Result<String> {
        self.cmd("bt")
    }

    pub(crate) fn threads(&mut self) -> std::io::Result<Vec<ThreadInfo>> {
        Ok(parse_threads(&self.cmd("thread list")?))
    }

    pub(crate) fn select_thread(&mut self, id: u32) -> std::io::Result<()> {
        let output = self.cmd(&format!("thread select {id}"))?;
        if output.contains("error:") || output.contains("no thread") {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                clean_lldb_error(&output, "thread is no longer available"),
            ));
        }
        Ok(())
    }

    pub(crate) fn select_frame(&mut self, index: usize) -> std::io::Result<()> {
        let output = self.cmd(&format!("frame select {index}"))?;
        if output.contains("error:") {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                clean_lldb_error(&output, "frame is no longer available"),
            ));
        }
        Ok(())
    }

    pub(crate) fn interrupt(&mut self) -> std::io::Result<String> {
        self.cmd("process interrupt")?;
        self.backtrace()
    }

    pub(crate) fn detach(&mut self) -> std::io::Result<()> {
        if self.debuggee_exited {
            self.attached = None;
            self.detached = true;
            return Ok(());
        }
        if self.child.try_wait()?.is_some() {
            if let Some(identity) = &self.attached {
                match identity.verify() {
                    // The target may have exited just before disconnect.  In
                    // that case there is no running process left to detach.
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        self.attached = None;
                        self.detached = true;
                        return Ok(());
                    }
                    Err(error) => return Err(error),
                    Ok(()) => {}
                }
            }
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "lldb closed before detach was proven",
            ));
        }
        if let Some(identity) = &self.attached {
            match identity.verify() {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    self.attached = None;
                    self.detached = true;
                    return Ok(());
                }
                Err(error) => return Err(error),
            }
        }
        let output = self.cmd("process detach")?;
        if lldb_reported_error(&output) && !output.contains("no process") {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "lldb could not detach from the debuggee",
            ));
        }
        if let Some(identity) = &self.attached {
            match identity.verify_running() {
                Ok(()) => {}
                // The debuggee exiting between detach and this check is the
                // normal race, not a detach failure.
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        self.attached = None;
        self.detached = true;
        Ok(())
    }

    /// Explicit DAP termination. `shutdown` otherwise detaches an attached
    /// process by default, so this path must kill the debuggee first.
    pub(crate) fn terminate_debuggee(&mut self) -> std::io::Result<()> {
        if self.debuggee_exited {
            self.attached = None;
            self.detached = true;
            return Ok(());
        }
        if self.child.try_wait()?.is_some() {
            if let Some(identity) = &self.attached {
                match identity.verify() {
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        self.attached = None;
                        self.detached = true;
                        return Ok(());
                    }
                    Err(error) => return Err(error),
                    Ok(()) => {}
                }
            }
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "lldb closed before termination was proven",
            ));
        }
        if let Some(identity) = &self.attached {
            identity.verify()?;
        }
        let output = self.cmd("process kill")?;
        if lldb_reported_error(&output) && !output.contains("no process") {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "lldb could not terminate the debuggee",
            ));
        }
        if let Some(identity) = &self.attached {
            match identity.verify() {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
                Ok(()) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "lldb did not prove that the debuggee terminated",
                    ))
                }
            }
        }
        self.attached = None;
        self.detached = true;
        Ok(())
    }

    /// End the session: detach attached targets, terminate launched targets,
    /// then reap lldb and remove both capture files. The command path is
    /// bounded; a wedged debugger cannot leave a child behind.
    pub(crate) fn quit(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        let running = self.child.try_wait().ok().flatten().is_none();
        if running {
            let command = if self.attached.is_some() {
                "process detach"
            } else if self.debuggee_exited {
                "quit"
            } else if !self.detached {
                "process kill"
            } else {
                "quit"
            };
            let _ = writeln!(self.stdin, "{command}");
            if command != "quit" {
                let _ = writeln!(self.stdin, "quit");
            }
            let _ = self.stdin.flush();
        }
        let deadline = Instant::now() + Duration::from_millis(500);
        while self.child.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        if let Some(reader) = self._reader.take() {
            let _ = reader.join();
        }
        let _ = std::fs::remove_file(&self.stdout_path);
        let _ = std::fs::remove_file(&self.stderr_path);
    }

    /// Extract the topmost frame from a `bt` reply. lldb's frame line has the
    /// stable shape (across recent lldb releases): `[* ]frame #0: 0x... <binary>`
    /// <func> [+ <off>] at <file>:<line>:<col>`.
    pub(crate) fn parse_top_frame(lldb_output: &str) -> Option<RawFrame> {
        lldb_output
            .lines()
            .find_map(|l| parse_frame_line(l.trim_start()))
    }

    /// Every `frame #N: …` line in a `bt` reply, innermost first.
    pub(crate) fn parse_frames(lldb_output: &str) -> Vec<RawFrame> {
        lldb_output
            .lines()
            .filter_map(|l| parse_frame_line(l.trim_start()))
            .collect()
    }

    /// Parse `frame variable`'s `(type) name = value` lines into `(type, name,
    /// value)` triples, in the order lldb printed them. A composite value
    /// (`String`/struct/enum without a one-line summary provider) spans
    /// MULTIPLE lines, opening a `{` that doesn't close until a later line
    /// (confirmed live) — this tracks brace depth and joins the continuation
    /// lines into one compact value rather than truncating at the first `{`.
    pub(crate) fn parse_typed_locals(lldb_output: &str) -> Vec<(String, String, String)> {
        let mut pairs = Vec::new();
        let mut lines = lldb_output.lines();
        while let Some(line) = lines.next() {
            let (type_name, after_ty) = if let Some(rest) = line.strip_prefix('(') {
                let Some(close) = rest.find(')') else {
                    continue;
                };
                (&rest[..close], rest[close + 1..].trim_start())
            } else {
                ("", line.trim_start())
            };
            let Some((name, first_value)) = after_ty.split_once(" = ") else {
                continue;
            };
            let first_value = first_value.trim();
            // rustc's `String`/`&str` summary provider (`lldb_commands`) prints
            // a readable `"text"` quoted literal FOLLOWED by the raw synthetic
            // children (`{ [0] = 't' [1] = 'e' … }`) on the same or next lines —
            // confirmed live. A complete quoted literal IS the value; the
            // per-byte children dump is noise a Jet debugger never shows (I2).
            if let Some(quoted) = complete_quoted_literal(first_value) {
                pairs.push((
                    type_name.to_string(),
                    name.trim().to_string(),
                    quoted.to_string(),
                ));
                continue;
            }
            let mut value = first_value.to_string();
            let mut depth = value.matches('{').count() as i32 - value.matches('}').count() as i32;
            while depth > 0 {
                let Some(next) = lines.next() else { break };
                let trimmed = next.trim();
                depth += trimmed.matches('{').count() as i32 - trimmed.matches('}').count() as i32;
                value.push(' ');
                value.push_str(trimmed);
            }
            pairs.push((type_name.to_string(), name.trim().to_string(), value));
        }
        pairs
    }

    /// Parse the immediate children of one stopped value. LLDB prints a
    /// composite as the root assignment followed by indented `(type) name =
    /// value` rows. The DAP adapter issues another bounded query when a child
    /// is expanded, so deeper rows stay behind that reference.
    pub(crate) fn parse_variable_children(lldb_output: &str) -> Vec<(String, String, String)> {
        let mut lines = lldb_output.lines();
        let Some(root) = lines.next() else {
            return Vec::new();
        };
        let root_indent = root.len() - root.trim_start().len();
        let Some(root_value) = root.split_once(" = ").map(|(_, value)| value.trim()) else {
            return Vec::new();
        };
        if !root_value.starts_with('{') {
            return Vec::new();
        }
        let mut depth =
            root_value.matches('{').count() as i32 - root_value.matches('}').count() as i32;
        let mut children = Vec::new();
        for line in lines {
            let trimmed = line.trim();
            if depth <= 0 {
                break;
            }
            let indent = line.len() - line.trim_start().len();
            if depth == 1 && indent > root_indent {
                if let Some((type_name, name, value)) = parse_typed_line(line) {
                    children.push((type_name.to_string(), name.to_string(), value.to_string()));
                }
            }
            depth += trimmed.matches('{').count() as i32 - trimmed.matches('}').count() as i32;
        }
        children
    }

    pub(crate) fn has_nested_value(raw: &str) -> bool {
        let raw = raw.trim();
        raw.starts_with('{') || raw.starts_with('[')
    }

    /// D-DBG3 step 2 (I2): translate a raw Rust local/param name back to its
    /// Jet spelling. Codegen's `mangle()` (`crates/jet-codegen/src/Codegen/mod.rs`)
    /// prefixes every non-`main` binding with `__jet_`; strip it. Names without
    /// that prefix are foreign/runtime frames and remain filtered.
    pub(crate) fn rust_local_to_jet(name: &str) -> Option<String> {
        name.strip_prefix(Syntax::GENERATED_NAME_PREFIX)
            .map(|rest| {
                // The exact inverse of `Names::mangle`: a comptime-marked Jet name
                // rides into Rust as `__jet_ct_<name>`, so give the mark back.
                match rest.strip_prefix("ct_") {
                    Some(marked) => format!("{}{marked}", Syntax::COMPTIME_MARK),
                    None => rest.to_string(),
                }
            })
    }

    /// Translate a generated field or array child into Jet display syntax.
    pub(crate) fn rust_member_to_jet(name: &str) -> Option<String> {
        let name = name.trim();
        if name.starts_with('[')
            && name.ends_with(']')
            && name[1..name.len() - 1].chars().all(|c| c.is_ascii_digit())
        {
            return Some(name.to_string());
        }
        Self::rust_local_is_jet_visible(name)
            .then(|| Self::rust_local_to_jet(name))
            .flatten()
    }

    /// Generated locals use the reserved dunder suffix. They remain available
    /// to an explicit raw scope, but never cross the default Jet projection.
    pub(crate) fn rust_local_is_jet_visible(name: &str) -> bool {
        name.strip_prefix(Syntax::GENERATED_NAME_PREFIX)
            .is_some_and(|rest| !rest.is_empty() && !rest.starts_with("__"))
    }

    /// Keep native summaries from exposing Rust layout, addresses, paths, or
    /// optimized-away storage through the default debugger view.
    pub(crate) fn safe_value(type_name: &str, raw: &str) -> String {
        let raw = raw.trim();
        if matches!(raw, "<optimized out>" | "<unavailable>") {
            return raw.to_string();
        }
        let safe = match type_name.trim() {
            "bool" | "Bool" => matches!(raw, "true" | "false"),
            "f32" | "f64" | "Float" => raw.parse::<f64>().is_ok(),
            "int" | "i8" | "i16" | "i32" | "i64" | "isize" | "u8" | "u16" | "u32" | "u64"
            | "usize" | "Int" => raw.parse::<i128>().is_ok(),
            "alloc::string::String" | "std::string::String" | "String" | "&str" => {
                complete_quoted_literal(raw).is_some_and(|value| value.len() == raw.len())
            }
            "()" | "Unit" => raw == "()",
            _ => false,
        };
        if safe {
            raw.to_string()
        } else {
            "<unavailable>".to_string()
        }
    }

    pub(crate) fn jet_type_name(raw: &str) -> Option<&'static str> {
        match raw.trim() {
            "bool" => Some("Bool"),
            "f32" | "f64" => Some("Float"),
            "int" | "i8" | "i16" | "i32" | "i64" | "isize" | "u8" | "u16" | "u32" | "u64"
            | "usize" => Some("Int"),
            "alloc::string::String" | "std::string::String" | "String" | "&str" => Some("String"),
            "()" => Some("Unit"),
            _ => None,
        }
    }

    /// The reverse of [`Self::rust_local_to_jet`] — the mangled Rust name
    /// lldb needs for `frame variable <name>`.
    pub(crate) fn jet_local_to_rust(name: &str) -> String {
        mangle(name)
    }

    /// Translate a validated Jet local path to the generated Rust path LLDB
    /// needs. Field names use the canonical codegen mangle; indices stay
    /// literal. This never asks LLDB to evaluate a Rust expression.
    pub(crate) fn jet_path_to_rust(root: &str, suffix: &str) -> Option<String> {
        let mut rust = mangle(root);
        let mut rest = suffix;
        while !rest.is_empty() {
            if let Some(field) = rest.strip_prefix('.') {
                let end = field
                    .char_indices()
                    .find(|(_, c)| !c.is_ascii_alphanumeric() && *c != '_')
                    .map(|(index, _)| index)
                    .unwrap_or(field.len());
                if end == 0 {
                    return None;
                }
                rust.push('.');
                rust.push_str(&mangle(&field[..end]));
                rest = &field[end..];
            } else if let Some(index) = rest.strip_prefix('[') {
                let end = index.find(']')?;
                let digits = &index[..end];
                if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
                    return None;
                }
                rust.push('[');
                rust.push_str(digits);
                rust.push(']');
                rest = &index[end + 1..];
            } else {
                return None;
            }
        }
        Some(rust)
    }

    /// Clean a raw Rust function symbol (`prog::main::h4002…` or
    /// `prog::__jet_helper::h991…`) down to its Jet name (I2): drop the
    /// trailing `::h<hash>` rustc appends, take the last path segment, then
    /// strip the `__jet_` mangling prefix (bare `main` passes through
    /// unchanged since codegen never mangles it — see `mangle()`).
    pub(crate) fn rust_func_to_jet(func: &str) -> String {
        let mut segs: Vec<&str> = func.split("::").collect();
        let looks_like_hash = segs.last().is_some_and(|s| {
            s.len() >= 2 && s.starts_with('h') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
        });
        if looks_like_hash {
            segs.pop();
        }
        let name = segs.last().copied().unwrap_or(func);
        name.strip_prefix(Syntax::GENERATED_NAME_PREFIX)
            .unwrap_or(name)
            .to_string()
    }

    pub(crate) fn safe_jet_func(func: &str) -> String {
        let name = Self::rust_func_to_jet(func);
        let generated = func
            .split("::")
            .any(|segment| segment.starts_with(Syntax::GENERATED_NAME_PREFIX));
        if name.starts_with("__")
            || name == "?"
            || name.contains("::")
            || (!generated && name != "main")
        {
            "<native frame>".to_string()
        } else {
            name
        }
    }
}

/// LLDB echoes commands as a prompt-prefixed line. Match that complete echo,
/// rather than a bare token: a user value or backend diagnostic can contain
/// the unique token without completing the command reply.
fn sentinel_echo_at_end(buf: &[u8]) -> bool {
    let marker = format!("(lldb) {}", SENTINEL);
    let marker = marker.as_bytes();
    if buf.len() < marker.len() || &buf[buf.len() - marker.len()..] != marker {
        return false;
    }
    let start = buf.len() - marker.len();
    start == 0 || buf[start - 1] == b'\n'
}

impl Drop for Inferior {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// `<rustc sysroot>/lib/rustlib/etc/lldb_lookup.py` — the Rust value
/// pretty-printer `rust-lldb` loads (`command script import`). `None` if
/// `rustc`/the file can't be found; the caller just runs without it (a
/// struct/`String` local then shows its raw field layout instead of a
/// friendly one-line value — a real Rust binary the compiler already needs
/// to produce this build, not an optional tool like lldb itself).
fn rust_pretty_printer_files() -> Option<(PathBuf, PathBuf)> {
    let out = Command::new("rustc")
        .arg("--print")
        .arg("sysroot")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let sysroot = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let etc = PathBuf::from(sysroot).join("lib/rustlib/etc");
    let script = etc.join("lldb_lookup.py");
    let commands = etc.join("lldb_commands");
    (script.exists() && commands.exists()).then_some((script, commands))
}

/// If `s` starts with a `"` and has a matching (unescaped) closing `"`,
/// return the quoted literal INCLUDING its quotes — everything after it (the
/// synthetic children dump the Rust `String`/`&str` summary provider still
/// appends) is deliberately dropped. `None` for anything that isn't a
/// complete quoted string (a plain scalar, or a composite value with no
/// summary provider at all).
fn complete_quoted_literal(s: &str) -> Option<&str> {
    if !s.starts_with('"') {
        return None;
    }
    let mut escaped = false;
    for (i, c) in s.char_indices().skip(1) {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' => escaped = true,
            '"' => return Some(&s[..=i]),
            _ => {}
        }
    }
    None
}

fn parse_typed_line(line: &str) -> Option<(&str, &str, &str)> {
    let line = line.trim_start();
    let (type_name, after_type) = if let Some(rest) = line.strip_prefix('(') {
        let close = rest.find(')')?;
        (&rest[..close], rest[close + 1..].trim_start())
    } else {
        ("", line)
    };
    let (name, value) = after_type.split_once(" = ")?;
    let name = name.trim();
    (!name.is_empty()).then_some((type_name, name, value.trim()))
}

fn parse_breakpoint(output: &str) -> Option<Breakpoint> {
    output.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("Breakpoint ")?;
        let id = rest.split(':').next()?.parse().ok()?;
        Some(Breakpoint {
            id,
            resolved: !line.contains("no locations"),
        })
    })
}

fn parse_exit_status(output: &str) -> Option<i32> {
    output.lines().find_map(|line| {
        let rest = line.split_once("exited with status =")?.1.trim_start();
        rest.split_whitespace().next()?.parse().ok()
    })
}

pub(crate) fn parse_exit_signal(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let marker = if line.contains("stop reason = signal ") {
            "stop reason = signal "
        } else if line.contains("stopped with signal ") {
            "stopped with signal "
        } else {
            return None;
        };
        line.split_once(marker).map(|(_, rest)| {
            rest.split_whitespace()
                .next()
                .unwrap_or("unknown")
                .to_string()
        })
    })
}

fn lldb_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn checked_lldb_quote(value: &str) -> io::Result<String> {
    if value.chars().any(|c| c.is_control()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "debugger path contains a control character",
        ));
    }
    Ok(lldb_quote(value))
}

fn valid_frame_expression(expression: &str) -> bool {
    !expression.is_empty()
        && expression
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '[' | ']' | ' '))
        && !expression.contains("  ")
}

fn parse_threads(output: &str) -> Vec<ThreadInfo> {
    output
        .lines()
        .filter_map(|line| {
            let marker = line.find("thread #")?;
            let rest = &line[marker + "thread #".len()..];
            let id = rest
                .split(|c: char| !c.is_ascii_digit())
                .next()?
                .parse()
                .ok()?;
            let name = rest
                .split_once("name = ")
                .map(|(_, rest)| rest.split(',').next().unwrap_or(rest).trim())
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format!("thread {id}"));
            Some(ThreadInfo { id, name })
        })
        .collect()
}

pub(crate) fn parse_current_thread_id(output: &str) -> Option<u32> {
    output.lines().find_map(|line| {
        let start = line.find("thread #")? + "thread #".len();
        let digits = line[start..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>();
        (!digits.is_empty()).then(|| digits.parse().ok()).flatten()
    })
}

fn clean_lldb_error(output: &str, fallback: &str) -> String {
    output
        .lines()
        .find(|line| line.contains("error:") || line.contains("no locations"))
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

#[cfg(target_os = "linux")]
struct ProcSnapshot {
    executable: PathBuf,
    executable_hash: String,
    uid: u32,
    session: u64,
    start_time: u64,
    state: char,
    tracer_pid: u32,
}

#[cfg(target_os = "linux")]
impl ProcSnapshot {
    fn read(base: &Path) -> io::Result<Self> {
        let status = std::fs::read_to_string(base.join("status"))?;
        let stat = std::fs::read_to_string(base.join("stat"))?;
        let executable_path = base.join("exe");
        let executable = std::fs::canonicalize(&executable_path)?;
        let executable_hash = SHA256::sha256_file_hex(&executable_path)?;
        let uid = proc_status_number(&status, "Uid:", 1)?;
        let tracer_pid = proc_status_number(&status, "TracerPid:", 0)?;
        let fields = proc_stat_fields(&stat)?;
        let state = fields
            .first()
            .and_then(|field| field.chars().next())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "process has no state"))?;
        let session = fields
            .get(3)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "process has no session"))?
            .parse()
            .map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "process session is invalid")
            })?;
        let start_time = fields
            .get(19)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "process has no start time"))?
            .parse()
            .map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "process start time is invalid")
            })?;
        Ok(Self {
            executable,
            executable_hash,
            uid,
            session,
            start_time,
            state,
            tracer_pid,
        })
    }
}

#[cfg(target_os = "linux")]
impl TargetIdentity {
    fn capture(binary: &Path, pid: u32) -> io::Result<Self> {
        if pid == 0 || pid == std::process::id() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "attach processId must identify another positive process",
            ));
        }
        if !binary.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "attach program does not exist",
            ));
        }
        let executable = std::fs::canonicalize(binary)?;
        let executable_hash = SHA256::sha256_file_hex(&executable)?;
        let proc_dir = std::fs::File::open(format!("/proc/{pid}"))?;
        let target = ProcSnapshot::read(&proc_base(&proc_dir))?;
        let current_status = std::fs::read_to_string("/proc/self/status")?;
        let current_uid = proc_status_number(&current_status, "Uid:", 1)?;
        validate_target(&target, &executable, &executable_hash, current_uid)?;
        Ok(Self {
            pid,
            proc_dir,
            executable,
            executable_hash,
            uid: target.uid,
            session: target.session,
            start_time: target.start_time,
        })
    }

    fn verify(&self) -> io::Result<()> {
        let current = ProcSnapshot::read(&proc_base(&self.proc_dir))?;
        if current.executable != self.executable
            || current.executable_hash != self.executable_hash
            || current.uid != self.uid
            || current.session != self.session
            || current.start_time != self.start_time
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "attach target identity changed",
            ));
        }
        if matches!(current.state, 'Z' | 'X' | 'x') {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "attach target has exited",
            ));
        }
        Ok(())
    }

    fn verify_running(&self) -> io::Result<()> {
        for _ in 0..20 {
            let current = ProcSnapshot::read(&proc_base(&self.proc_dir))?;
            self.verify()?;
            if !matches!(current.state, 'T' | 't') {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            format!("attach target {} did not resume after detach", self.pid),
        ))
    }
}

#[cfg(not(target_os = "linux"))]
impl TargetIdentity {
    fn capture(_binary: &Path, _pid: u32) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "verified local attach is unavailable on this host",
        ))
    }

    fn verify(&self) -> io::Result<()> {
        let _ = self;
        Ok(())
    }

    fn verify_running(&self) -> io::Result<()> {
        let _ = self;
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn proc_base(proc_dir: &std::fs::File) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", proc_dir.as_raw_fd()))
}

#[cfg(target_os = "linux")]
fn proc_status_number(status: &str, key: &str, index: usize) -> io::Result<u32> {
    let values = status
        .lines()
        .find_map(|line| line.strip_prefix(key))
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "process status is incomplete")
        })?;
    values
        .split_whitespace()
        .nth(index)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "process status is incomplete"))?
        .parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "process status is invalid"))
}

#[cfg(target_os = "linux")]
fn proc_stat_fields(stat: &str) -> io::Result<Vec<&str>> {
    let close = stat
        .rfind(')')
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "process stat is invalid"))?;
    Ok(stat[close + 1..].split_whitespace().collect())
}

#[cfg(target_os = "linux")]
fn validate_target(
    target: &ProcSnapshot,
    executable: &Path,
    executable_hash: &str,
    current_uid: u32,
) -> io::Result<()> {
    if target.uid != current_uid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "attach target belongs to another user",
        ));
    }
    if target.tracer_pid != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "attach target is already being traced",
        ));
    }
    if matches!(target.state, 'Z' | 'X' | 'x') {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "attach target has exited",
        ));
    }
    if target.executable != executable || target.executable_hash != executable_hash {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "attach target executable does not match the debug binary",
        ));
    }
    Ok(())
}

fn lldb_reported_error(output: &str) -> bool {
    output
        .lines()
        .any(|line| line.trim_start().starts_with("error:"))
}

fn verify_attach_target(binary: &Path, pid: u32) -> io::Result<TargetIdentity> {
    TargetIdentity::capture(binary, pid)
}

fn cleanup_failed_start(
    mut child: Child,
    stdin: ChildStdin,
    rx: Receiver<u8>,
    reader: std::thread::JoinHandle<()>,
    paths: &[&Path],
) {
    drop(stdin);
    drop(rx);
    let _ = child.kill();
    let _ = child.wait();
    let _ = reader.join();
    for path in paths {
        let _ = std::fs::remove_file(path);
    }
}

fn capture_path(tmp: &Path, counter: u64, stream: &str) -> PathBuf {
    tmp.join(format!(
        "jet_debug_native_{}_{}.{}",
        std::process::id(),
        counter,
        stream
    ))
}

fn create_capture_file(path: &Path) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    options.open(path).map(|_| ())
}

fn parse_frame_line(line: &str) -> Option<RawFrame> {
    // lldb marks the selected frame with a leading `* ` (e.g. `  * frame #0: …`);
    // every other frame just has leading whitespace before `frame #N:`.
    let line = line.strip_prefix("* ").unwrap_or(line);
    if !line.starts_with("frame #") {
        return None;
    }
    let (head, loc) = line.rsplit_once(" at ")?;
    let mut loc_parts = loc.rsplit(':');
    let _col = loc_parts.next()?;
    let line_str = loc_parts.next()?;
    let rust_line: usize = line_str.parse().ok()?;
    let rust_file = loc_parts.rev().collect::<Vec<_>>().join(":");
    // `head` looks like `frame #0: 0x... binary`func_name + 20` — the function
    // name sits right after the backtick, up to ` + ` (an offset) or the end.
    let func = head
        .rsplit_once('`')
        .map(|(_, rest)| rest.split(" + ").next().unwrap_or(rest).trim().to_string())
        .unwrap_or_else(|| "?".to_string());
    Some(RawFrame {
        func,
        rust_file,
        rust_line,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exited_lldb_is_not_reported_as_a_proven_disconnect() {
        let mut child = Command::new("true")
            .stdin(Stdio::piped())
            .spawn()
            .expect("spawn exited debugger stand-in");
        let stdin = child.stdin.take().expect("piped stdin");
        child.wait().expect("reap exited debugger stand-in");
        let (_tx, rx) = std::sync::mpsc::channel();
        let mut inferior = Inferior {
            child,
            stdin,
            rx,
            _reader: None,
            stdout_path: PathBuf::new(),
            stderr_path: PathBuf::new(),
            stdout_pos: 0,
            stderr_pos: 0,
            attached: None,
            detached: false,
            debuggee_exited: false,
            closed: false,
        };
        let error = inferior
            .detach()
            .expect_err("dead lldb cannot prove detach");
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_identity_verifies_a_real_process_and_rejects_another_binary() {
        let mut target = Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn real attach target");
        let pid = target.id();
        let result = (|| -> io::Result<()> {
            let target_binary = std::fs::canonicalize(format!("/proc/{pid}/exe"))?;
            let identity = TargetIdentity::capture(&target_binary, pid)?;
            identity.verify()?;

            let other_binary = std::env::current_exe()?;
            match TargetIdentity::capture(&other_binary, pid) {
                Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {}
                Err(error) => {
                    return Err(io::Error::new(
                        error.kind(),
                        "wrong-binary attach failed for the wrong reason",
                    ))
                }
                Ok(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        "wrong-binary attach was accepted",
                    ))
                }
            }
            Ok(())
        })();
        let _ = target.kill();
        let _ = target.wait();
        result.expect("real-process attach identity verification");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attaches_to_and_detaches_from_a_real_local_process() {
        if !Inferior::available() {
            eprintln!("skipping live native attach test: lldb is unavailable");
            return;
        }
        let mut target = Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn real attach target");
        let pid = target.id();
        let target_binary = std::fs::canonicalize(format!("/proc/{pid}/exe"))
            .expect("resolve real attach target binary");
        let mut inferior = match Inferior::attach(&target_binary, pid) {
            Ok(inferior) => inferior,
            Err(error) => {
                let _ = target.kill();
                let _ = target.wait();
                panic!("real local attach failed: {error}");
            }
        };
        let detach = inferior.detach();
        inferior.quit();
        let _ = target.kill();
        let _ = target.wait();
        detach.expect("real local detach was not proven");
    }

    #[test]
    fn parses_a_frame_line() {
        let out = "Process 1 stopped\n* thread #1, stop reason = breakpoint 1.1\n    frame #0: 0x0000000100000f50 jet_golden_05_loops`__jet_main + 20 at 05_loops.rs:7:5\n";
        let f = Inferior::parse_top_frame(out).expect("frame parsed");
        assert_eq!(f.func, "__jet_main");
        assert_eq!(f.rust_file, "05_loops.rs");
        assert_eq!(f.rust_line, 7);
    }

    #[test]
    fn parses_breakpoint_identity_and_pending_state() {
        let resolved =
            parse_breakpoint("Breakpoint 4: where = prog`run + 8 at app.rs:7:1").unwrap();
        assert_eq!(resolved.id, 4);
        assert!(resolved.resolved);

        let pending = parse_breakpoint("Breakpoint 5: no locations (pending).").unwrap();
        assert_eq!(pending.id, 5);
        assert!(!pending.resolved);
    }

    #[test]
    fn parses_exit_status_and_quotes_paths_for_lldb() {
        assert_eq!(
            parse_exit_status("Process 1 exited with status = 17 (0x11)"),
            Some(17)
        );
        assert_eq!(lldb_quote("a file\\name.rs"), "\"a file\\\\name.rs\"");
    }

    #[test]
    fn sentinel_requires_the_lldb_prompt_echo() {
        let token = SENTINEL.as_bytes();
        assert!(!sentinel_echo_at_end(token));
        assert!(!sentinel_echo_at_end(
            b"(lldb) frame variable text = __jet_dbg_sentinel_9f3c__"
        ));
        assert!(sentinel_echo_at_end(b"(lldb) __jet_dbg_sentinel_9f3c__"));
        assert!(sentinel_echo_at_end(
            b"output\n(lldb) __jet_dbg_sentinel_9f3c__"
        ));
    }

    #[test]
    fn parses_multiple_frames_for_backtrace() {
        let out = "  * frame #0: 0x1 bin`__jet_helper at a.rs:3:1\n    frame #1: 0x2 bin`__jet_main + 10 at a.rs:9:1\n";
        let frames = Inferior::parse_frames(out);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].func, "__jet_helper");
        assert_eq!(frames[1].rust_line, 9);
    }

    #[test]
    fn parses_locals_with_type_prefix() {
        let out = "(int) n = 5\n(int) total = 0\n";
        let locals = Inferior::parse_typed_locals(out);
        assert_eq!(
            locals,
            vec![
                ("int".to_string(), "n".to_string(), "5".to_string()),
                ("int".to_string(), "total".to_string(), "0".to_string())
            ]
        );
    }

    /// The rust pretty-printer's `String`/`&str` summary prints a readable
    /// quoted literal FOLLOWED by the raw per-byte synthetic children on the
    /// same line — confirmed live. Only the quoted literal should surface.
    #[test]
    fn parses_a_string_summary_and_drops_its_synthetic_children() {
        let out = "(alloc::string::String) text = \"hi\" { [0] = 'h' [1] = 'i' }\n";
        let locals = Inferior::parse_typed_locals(out);
        assert_eq!(
            locals,
            vec![(
                "alloc::string::String".to_string(),
                "text".to_string(),
                "\"hi\"".to_string()
            )]
        );
    }

    /// A composite value with NO summary provider genuinely spans multiple
    /// lines (each nested field on its own indented line) — the whole thing
    /// should be captured, not truncated at the first `{`.
    #[test]
    fn parses_a_multiline_struct_without_a_summary_provider() {
        let out = "(__jet_Point) p = {\n  x = 1\n  y = 2\n}\n";
        let locals = Inferior::parse_typed_locals(out);
        assert_eq!(locals.len(), 1);
        assert_eq!(locals[0].0, "__jet_Point");
        assert_eq!(locals[0].1, "p");
        assert!(locals[0].2.contains("x = 1") && locals[0].2.contains("y = 2"));
    }

    #[test]
    fn parses_immediate_children_without_exposing_deeper_rows() {
        let out = "(__jet_Point) __jet_point = {\n  (__int) __jet_x = 1\n  (__jet_Point) __jet_nested = {\n    (__int) __jet_y = 2\n  }\n}\n";
        assert_eq!(
            Inferior::parse_variable_children(out),
            vec![
                ("__int".to_string(), "__jet_x".to_string(), "1".to_string()),
                (
                    "__jet_Point".to_string(),
                    "__jet_nested".to_string(),
                    "{".to_string()
                ),
            ]
        );
    }

    #[test]
    fn no_frame_line_is_none() {
        assert!(Inferior::parse_top_frame("Process 1 exited with status = 0\n").is_none());
    }

    #[test]
    fn local_name_translation_round_trips() {
        assert_eq!(
            Inferior::rust_local_to_jet("__jet_total"),
            Some("total".to_string())
        );
        assert_eq!(Inferior::jet_local_to_rust("total"), "__jet_total");
        assert_eq!(
            Inferior::rust_local_to_jet("__jet_switch_subject"),
            Some("switch_subject".to_string())
        );
        assert_eq!(
            Inferior::rust_local_to_jet("__jet___switch_subject"),
            Some("__switch_subject".to_string())
        );
        assert_eq!(
            Inferior::jet_local_to_rust("__switch_subject"),
            "__jet___switch_subject"
        );
    }

    #[test]
    fn default_local_projection_hides_internal_names_and_unknown_values() {
        assert!(Inferior::rust_local_is_jet_visible("__jet_total"));
        assert!(!Inferior::rust_local_is_jet_visible("__jet_"));
        assert!(!Inferior::rust_local_is_jet_visible("__jet___temporary"));
        assert!(!Inferior::rust_local_is_jet_visible("allocator_temp"));
        assert_eq!(Inferior::safe_value("int", "7"), "7");
        assert_eq!(
            Inferior::safe_value("SomeRustStruct", "{ address = 0x1 }"),
            "<unavailable>"
        );
        assert_eq!(
            Inferior::safe_value("int", "<optimized out>"),
            "<optimized out>"
        );
        assert_eq!(Inferior::jet_type_name("SomeRustStruct"), None);
    }

    #[test]
    fn lldb_paths_reject_control_characters() {
        assert!(checked_lldb_quote("safe/name.rs").is_ok());
        assert!(checked_lldb_quote("bad\nname.rs").is_err());
        assert!(valid_frame_expression("__jet_value.items[0]"));
        assert!(!valid_frame_expression("__jet_value; process kill"));
    }

    #[test]
    fn translates_jet_paths_and_keeps_array_indices_literal() {
        assert_eq!(
            Inferior::jet_path_to_rust("point", ".x[2].label"),
            Some("__jet_point.__jet_x[2].__jet_label".to_string())
        );
        assert_eq!(Inferior::jet_path_to_rust("point", "."), None);
        assert_eq!(
            Inferior::rust_member_to_jet("__jet_x"),
            Some("x".to_string())
        );
        assert_eq!(Inferior::rust_member_to_jet("allocator_temp"), None);
        assert_eq!(Inferior::rust_member_to_jet("__jet___temporary"), None);
        assert_eq!(Inferior::rust_member_to_jet("[2]"), Some("[2]".to_string()));
    }

    #[test]
    fn parses_the_selected_thread_id_from_a_stop_banner() {
        assert_eq!(
            parse_current_thread_id("* thread #7, stop reason = breakpoint 1.1\n"),
            Some(7)
        );
        assert_eq!(
            parse_current_thread_id("Process 1 exited with status = 0"),
            None
        );
    }

    #[test]
    fn func_name_strips_crate_path_and_hash() {
        assert_eq!(
            Inferior::rust_func_to_jet("prog::main::h40021039c79c235b"),
            "main"
        );
        assert_eq!(
            Inferior::rust_func_to_jet("prog::__jet_helper::h991a2b3c4d5e6f70"),
            "helper"
        );
        // No hash suffix and no `::` path (e.g. a libc frame, already past
        // `parse_frame_line`'s own backtick split) — passes through as-is.
        assert_eq!(
            Inferior::rust_func_to_jet("__libc_start_main"),
            "__libc_start_main"
        );
        assert_eq!(
            Inferior::safe_jet_func("__libc_start_main"),
            "<native frame>"
        );
        assert_eq!(
            Inferior::safe_jet_func("prog::helper::h4002"),
            "<native frame>"
        );
        assert_eq!(Inferior::safe_jet_func("prog::__jet_run::h4002"), "run");
    }
}
