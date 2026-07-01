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

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::Receiver;
use std::time::Duration;

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

/// What a resuming command (`run`/`continue`/a step) settled into — see the
/// module doc point 2 for why this is derived from a follow-up `bt`, never
/// from the resume command's own text.
pub(crate) enum ResumeResult {
    /// Stopped at a frame; the raw `bt` reply (parse with
    /// [`Inferior::parse_top_frame`]/[`Inferior::parse_frames`]).
    Stopped(String),
    /// The debuggee ran to completion.
    Exited,
}

pub(crate) struct Inferior {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<u8>,
    _reader: std::thread::JoinHandle<()>,
    stdout_path: PathBuf,
    stderr_path: PathBuf,
    /// Bytes already drained from each redirected file (see module doc point 3).
    stdout_pos: u64,
    stderr_pos: u64,
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
        let mut cmd = Command::new("lldb");
        cmd.arg("--no-lldbinit");
        if let Some((script, commands)) = rust_pretty_printer_files() {
            cmd.arg("--one-line-before-file")
                .arg(format!("command script import \"{}\"", script.display()))
                .arg("--source-before-file")
                .arg(commands);
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
        let stdout_path = tmp.join(format!("jet_debug_native_{}_{}.stdout", std::process::id(), n));
        let stderr_path = tmp.join(format!("jet_debug_native_{}_{}.stderr", std::process::id(), n));
        std::fs::write(&stdout_path, b"")?;
        std::fs::write(&stderr_path, b"")?;

        let mut inf = Inferior {
            child,
            stdin,
            rx,
            _reader: reader,
            stdout_path,
            stderr_path,
            stdout_pos: 0,
            stderr_pos: 0,
        };
        inf.write_lines(&[])?;
        let out_setting = format!("settings set target.output-path {}", inf.stdout_path.display());
        let err_setting = format!("settings set target.error-path {}", inf.stderr_path.display());
        inf.write_lines(&[&out_setting, &err_setting])?;
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
        let marker = SENTINEL.as_bytes();
        let mut buf: Vec<u8> = Vec::new();
        loop {
            let byte = match self.rx.recv_timeout(READ_TIMEOUT) {
                Ok(b) => b,
                Err(_) => break, // EOF or a genuine stall — return what we have.
            };
            buf.push(byte);
            if buf.len() >= marker.len() && &buf[buf.len() - marker.len()..] == marker {
                break; // The sentinel's echo is on stdout; its rejection went
                       // to stderr, which is null — nothing more to consume.
            }
        }
        // Drop the sentinel's own echo from the returned text (the trailing
        // `\n(lldb) __jet_dbg_sentinel_9f3c__` line) so callers see only the
        // real command(s)' output.
        let text = String::from_utf8_lossy(&buf).into_owned();
        let cut = text.rfind(&format!("(lldb) {}", SENTINEL)).unwrap_or(text.len());
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
        if full.contains("exited with status") {
            return Ok(ResumeResult::Exited);
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
    pub(crate) fn set_breakpoint(&mut self, rust_file: &str, rust_line: usize) -> std::io::Result<()> {
        self.cmd(&format!("breakpoint set -f {} -l {}", rust_file, rust_line))?;
        Ok(())
    }

    /// `frame variable` — every local in the current frame, `(type) name = value`
    /// per line (parsed by [`Self::parse_locals`]).
    pub(crate) fn locals(&mut self) -> std::io::Result<String> {
        self.cmd("frame variable")
    }

    /// `frame variable <name>` — a single local by its JET name (translated to
    /// the mangled Rust name lldb needs — see [`jet_local_to_rust`]).
    pub(crate) fn print_var(&mut self, jet_name: &str) -> std::io::Result<String> {
        self.cmd(&format!("frame variable {}", Self::jet_local_to_rust(jet_name)))
    }

    /// `bt` — the full native call stack (parsed by [`Self::parse_frames`]).
    /// Safe to call standalone (not after a resume) since the debuggee is
    /// already stopped by the time the caller asks.
    pub(crate) fn backtrace(&mut self) -> std::io::Result<String> {
        self.cmd("bt")
    }

    /// End the session: `quit`, then reap the child so it never lingers.
    pub(crate) fn quit(mut self) {
        let _ = self.cmd("quit");
        let _ = self.child.wait();
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

    /// Parse `frame variable`'s `(type) name = value` lines into `(name,
    /// value)` pairs, in the order lldb printed them. A composite value
    /// (`String`/struct/enum without a one-line summary provider) spans
    /// MULTIPLE lines, opening a `{` that doesn't close until a later line
    /// (confirmed live) — this tracks brace depth and joins the continuation
    /// lines into one compact value rather than truncating at the first `{`.
    pub(crate) fn parse_locals(lldb_output: &str) -> Vec<(String, String)> {
        let mut pairs = Vec::new();
        let mut lines = lldb_output.lines();
        while let Some(line) = lines.next() {
            let after_ty = if line.starts_with('(') {
                match line.find(')') {
                    Some(close) => line[close + 1..].trim_start(),
                    None => continue,
                }
            } else {
                line.trim_start()
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
                pairs.push((name.trim().to_string(), quoted.to_string()));
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
            pairs.push((name.trim().to_string(), value));
        }
        pairs
    }

    /// D-DBG3 step 2 (I2): translate a raw Rust local/param name back to its
    /// Jet spelling. Codegen's `mangle()` (`crates/jet-codegen/src/Codegen/mod.rs`)
    /// prefixes every non-`main` binding with `user_`; strip it. A name
    /// WITHOUT that prefix is a compiler-internal temporary
    /// (`_jet_switch_subject`, destructure temps, …) that should never
    /// surface — `None` means "filter this one out", not "show it raw".
    pub(crate) fn rust_local_to_jet(name: &str) -> Option<String> {
        name.strip_prefix("user_").map(|s| s.to_string())
    }

    /// The reverse of [`Self::rust_local_to_jet`] — the mangled Rust name
    /// lldb needs for `frame variable <name>`.
    pub(crate) fn jet_local_to_rust(name: &str) -> String {
        format!("user_{}", name)
    }

    /// Clean a raw Rust function symbol (`prog::main::h4002…` or
    /// `prog::user_helper::h991…`) down to its Jet name (I2): drop the
    /// trailing `::h<hash>` rustc appends, take the last path segment, then
    /// strip the `user_` mangling prefix (bare `main` passes through
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
        name.strip_prefix("user_").unwrap_or(name).to_string()
    }
}

/// `<rustc sysroot>/lib/rustlib/etc/lldb_lookup.py` — the Rust value
/// pretty-printer `rust-lldb` loads (`command script import`). `None` if
/// `rustc`/the file can't be found; the caller just runs without it (a
/// struct/`String` local then shows its raw field layout instead of a
/// friendly one-line value — a real Rust binary the compiler already needs
/// to produce this build, not an optional tool like lldb itself).
fn rust_pretty_printer_files() -> Option<(PathBuf, PathBuf)> {
    let out = Command::new("rustc").arg("--print").arg("sysroot").output().ok()?;
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
    fn parses_a_frame_line() {
        let out = "Process 1 stopped\n* thread #1, stop reason = breakpoint 1.1\n    frame #0: 0x0000000100000f50 jet_golden_05_loops`user_main + 20 at 05_loops.rs:7:5\n";
        let f = Inferior::parse_top_frame(out).expect("frame parsed");
        assert_eq!(f.func, "user_main");
        assert_eq!(f.rust_file, "05_loops.rs");
        assert_eq!(f.rust_line, 7);
    }

    #[test]
    fn parses_multiple_frames_for_backtrace() {
        let out = "  * frame #0: 0x1 bin`user_helper at a.rs:3:1\n    frame #1: 0x2 bin`user_main + 10 at a.rs:9:1\n";
        let frames = Inferior::parse_frames(out);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].func, "user_helper");
        assert_eq!(frames[1].rust_line, 9);
    }

    #[test]
    fn parses_locals_with_type_prefix() {
        let out = "(int) n = 5\n(int) total = 0\n";
        let locals = Inferior::parse_locals(out);
        assert_eq!(
            locals,
            vec![
                ("n".to_string(), "5".to_string()),
                ("total".to_string(), "0".to_string())
            ]
        );
    }

    /// The rust pretty-printer's `String`/`&str` summary prints a readable
    /// quoted literal FOLLOWED by the raw per-byte synthetic children on the
    /// same line — confirmed live. Only the quoted literal should surface.
    #[test]
    fn parses_a_string_summary_and_drops_its_synthetic_children() {
        let out = "(alloc::string::String) text = \"hi\" { [0] = 'h' [1] = 'i' }\n";
        let locals = Inferior::parse_locals(out);
        assert_eq!(locals, vec![("text".to_string(), "\"hi\"".to_string())]);
    }

    /// A composite value with NO summary provider genuinely spans multiple
    /// lines (each nested field on its own indented line) — the whole thing
    /// should be captured, not truncated at the first `{`.
    #[test]
    fn parses_a_multiline_struct_without_a_summary_provider() {
        let out = "(user_Point) p = {\n  x = 1\n  y = 2\n}\n";
        let locals = Inferior::parse_locals(out);
        assert_eq!(locals.len(), 1);
        assert_eq!(locals[0].0, "p");
        assert!(locals[0].1.contains("x = 1") && locals[0].1.contains("y = 2"));
    }

    #[test]
    fn no_frame_line_is_none() {
        assert!(Inferior::parse_top_frame("Process 1 exited with status = 0\n").is_none());
    }

    #[test]
    fn local_name_translation_round_trips() {
        assert_eq!(Inferior::rust_local_to_jet("user_total"), Some("total".to_string()));
        assert_eq!(Inferior::jet_local_to_rust("total"), "user_total");
        // A compiler-internal temp (no `user_` prefix) is filtered, not shown raw.
        assert_eq!(Inferior::rust_local_to_jet("_jet_switch_subject"), None);
    }

    #[test]
    fn func_name_strips_crate_path_and_hash() {
        assert_eq!(Inferior::rust_func_to_jet("prog::main::h40021039c79c235b"), "main");
        assert_eq!(Inferior::rust_func_to_jet("prog::user_helper::h991a2b3c4d5e6f70"), "helper");
        // No hash suffix and no `::` path (e.g. a libc frame, already past
        // `parse_frame_line`'s own backtick split) — passes through as-is.
        assert_eq!(Inferior::rust_func_to_jet("__libc_start_main"), "__libc_start_main");
    }
}
