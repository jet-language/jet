//! D-DBG3 step 2 (dap-debugger): the native lldb-backed `jet debug` session.
//! Reuses the EXACT `(jet)` command vocabulary the step-1 interpreter debugger
//! ships (`jet-debug`, D-DBG3 — I8: one vocabulary regardless of
//! backend) but steps the REAL compiled binary through [`super::Inferior`], so
//! it covers the full feature set the interpreter declines (FFI, tasks,
//! `#Unsafe`, native std — the E2203 boundary).
//!
//! I2: every frame/line/value shown by default is translated back to Jet terms
//! through [`super::LineMap`]; a frame with no Jet line (prelude/generated glue)
//! is stepped over transparently, never shown raw. `--raw-frames` (D-DBG2) is the
//! expert opt-in that shows the raw Rust file:line instead.
//!
//! Caveat (honest, not a stub): this module's lldb-output parsing
//! (`Inferior::parse_top_frame`/`parse_typed_locals`) is written against lldb's
//! documented, stable batch-mode text shapes, but this sandbox has no `lldb`
//! binary to verify against live — `Inferior::available()` gates every entry
//! point and `tests/debug_native.rs` skips (not fails) when it's absent, the
//! same posture `tests/observe.rs` takes for `rustc`. Verify on a machine with
//! lldb before calling this airtight.

use std::path::Path;

use super::Inferior::{Inferior, ResumeResult};
use super::LineMap::LineMap;
use super::{DebugSnapshot, FrameSnapshot, ValueSnapshot};
use crate::ExitCodes;
use crate::Syntax;

/// A bound on auto step-over retries when a stop lands on a frame with no Jet
/// line (prelude/generated glue) — avoids hanging forever if lldb never
/// reaches mapped code (e.g. it ran off into library code with no way back).
const MAX_STEP_OVER_UNMAPPED: usize = 200;

/// How the session reads its `(jet)` commands and where its output goes — the
/// same split this crate's interpreter backend uses, so a test can
/// script a native session exactly like `run_session` scripts the interpreter.
enum IO {
    Interactive,
    Scripted {
        inputs: std::collections::VecDeque<String>,
        out: String,
        /// Keep the inferior at the current stop when scripted input ends.
        /// Canvas uses this to expose one live command boundary; the existing
        /// transcript API retains its run-to-completion behavior.
        pause_on_input_end: bool,
    },
}

impl IO {
    fn is_scripted(&self) -> bool {
        matches!(self, IO::Scripted { .. })
    }

    fn pause_on_input_end(&self) -> bool {
        matches!(
            self,
            IO::Scripted {
                pause_on_input_end: true,
                ..
            }
        )
    }
}

pub fn run(
    binary: &Path,
    rust_file: &str,
    rust_src: &str,
    jet_file: &str,
    jet_src: &str,
    raw_frames: bool,
) -> i32 {
    let (code, _captured, _paused) = run_with_io(
        binary,
        rust_file,
        rust_src,
        jet_file,
        jet_src,
        raw_frames,
        IO::Interactive,
    );
    code
}

/// Scripted native session for tests: feeds `inputs` to the `(jet)` prompt in
/// order and returns the captured transcript, the same shape
/// `Debug::run_session` gives the interpreter backend. Gated on `lldb`
/// presence by the caller (`Inferior::available()`); this function itself
/// just reports `E2203`-style unavailability into the transcript if called
/// without it, rather than assuming the caller checked.
pub fn run_scripted(
    binary: &Path,
    rust_file: &str,
    rust_src: &str,
    jet_file: &str,
    jet_src: &str,
    raw_frames: bool,
    inputs: &[&str],
) -> String {
    let queue: std::collections::VecDeque<String> = inputs.iter().map(|s| s.to_string()).collect();
    let io = IO::Scripted {
        inputs: queue,
        out: String::new(),
        pause_on_input_end: false,
    };
    let (_code, captured, _paused) = run_with_io(
        binary, rust_file, rust_src, jet_file, jet_src, raw_frames, io,
    );
    captured
}

/// Scripted native session with one live command boundary. The inferior is
/// restarted from the same compiled artifact on each replay, so the caller
/// can keep a bounded command history while still using the native debugger
/// for the current execution tier.
pub fn run_scripted_paused(
    binary: &Path,
    rust_file: &str,
    rust_src: &str,
    jet_file: &str,
    jet_src: &str,
    raw_frames: bool,
    inputs: &[&str],
) -> (i32, String, bool) {
    run_scripted_mode(
        binary, rust_file, rust_src, jet_file, jet_src, raw_frames, inputs, true,
    )
}

pub(crate) fn run_scripted_mode(
    binary: &Path,
    rust_file: &str,
    rust_src: &str,
    jet_file: &str,
    jet_src: &str,
    raw_frames: bool,
    inputs: &[&str],
    pause_on_input_end: bool,
) -> (i32, String, bool) {
    let queue: std::collections::VecDeque<String> = inputs.iter().map(|s| s.to_string()).collect();
    let io = IO::Scripted {
        inputs: queue,
        out: String::new(),
        pause_on_input_end,
    };
    run_with_io(
        binary, rust_file, rust_src, jet_file, jet_src, raw_frames, io,
    )
}

fn run_with_io(
    binary: &Path,
    rust_file: &str,
    rust_src: &str,
    jet_file: &str,
    jet_src: &str,
    raw_frames: bool,
    mut io: IO,
) -> (i32, String, bool) {
    if !Inferior::available() {
        let msg = format!(
            "error: native `jet debug` needs `lldb` on PATH, which isn't installed\n fix: install lldb for the native backend (FFI/tasks/#Unsafe/native-std), or use `jet debug {}` on a program the step-1 interpreter covers",
            jet_file
        );
        if io.is_scripted() {
            if let IO::Scripted { out, .. } = &mut io {
                out.push_str(&msg);
                out.push('\n');
            }
        } else {
            eprintln!("{}", msg);
        }
        return (ExitCodes::USER_ERROR, io_into_output(io), false);
    }
    let map_path = binary.with_extension("jetmap");
    if let Err(_error) =
        LineMap::write_artifact(&map_path, jet_file, jet_src, rust_file, rust_src, binary)
    {
        report_error(
            &mut io,
            "error: native debugger could not write the source map",
        );
        return (ExitCodes::USER_ERROR, io_into_output(io), false);
    }
    let map =
        match LineMap::load_verified(&map_path, jet_file, jet_src, rust_file, rust_src, binary) {
            Ok(map) => map,
            Err(_error) => {
                report_error(&mut io, "error: native debugger map is not usable");
                return (ExitCodes::USER_ERROR, io_into_output(io), false);
            }
        };
    let mut inf = match Inferior::spawn(binary) {
        Ok(i) => i,
        Err(_e) => {
            report_error(
                &mut io,
                "error: native debugger could not launch the compiled session",
            );
            return (ExitCodes::ICE, io_into_output(io), false);
        }
    };
    // Stop at `fn run`'s first REAL statement, by file:line — the same place
    // the step-1 interpreter debugger stops (Resume::Step on the very first
    // statement). A name-based `-n main` breakpoint can resolve to more than
    // one symbol and land with no source line info at all (verified against a
    // live lldb — see the module doc); a marker-based file:line breakpoint has
    // no such ambiguity.
    let Some(entry_line) = map.main_entry_line(rust_src) else {
        report_error(
            &mut io,
            "error: native debugger could not find a source-mapped entry statement",
        );
        return (ExitCodes::ICE, io_into_output(io), false);
    };
    if let Err(_e) = inf.set_breakpoint(rust_file, entry_line) {
        report_error(
            &mut io,
            "error: native debugger could not set the source breakpoint",
        );
        return (ExitCodes::ICE, io_into_output(io), false);
    }
    let result = match inf.resume_and_locate("run") {
        Ok(r) => r,
        Err(_e) => {
            report_error(
                &mut io,
                "error: native debugger could not start the compiled session",
            );
            return (ExitCodes::ICE, io_into_output(io), false);
        }
    };
    let mut session = Session {
        inf,
        map,
        rust_file: rust_file.to_string(),
        jet_file: jet_file.to_string(),
        jet_src: jet_src.to_string(),
        raw_frames,
        started: false,
        exited: false,
        exit_code: ExitCodes::OK,
        io,
    };
    session.handle_resume(result);
    let (code, paused) = session.prompt_loop();
    let io = session.io;
    session.inf.quit();
    (code, io_into_output(io), paused)
}

fn io_into_output(io: IO) -> String {
    match io {
        IO::Scripted { out, .. } => out,
        IO::Interactive => String::new(),
    }
}

fn report_error(io: &mut IO, message: &str) {
    if let IO::Scripted { out, .. } = io {
        out.push_str(message);
        out.push('\n');
    } else {
        eprintln!("{}", message);
    }
}

struct Session {
    inf: Inferior,
    map: LineMap,
    /// The generated Rust file's own name (what lldb's debug info points at —
    /// NOT the Jet file), for `breakpoint set -f <rust_file> -l <rust_line>`.
    rust_file: String,
    jet_file: String,
    jet_src: String,
    raw_frames: bool,
    started: bool,
    exited: bool,
    exit_code: i32,
    io: IO,
}

impl Session {
    fn emit(&mut self, s: &str) {
        match &mut self.io {
            IO::Interactive => println!("{}", s),
            IO::Scripted { out, .. } => {
                out.push_str(s);
                out.push('\n');
            }
        }
    }

    /// Read the next `(jet)` command. `None` means end-of-input: live EOF, or
    /// a scripted transcript that ran out (both treated as "run to completion").
    fn read_command(&mut self) -> Option<String> {
        match &mut self.io {
            IO::Interactive => {
                use std::io::Write;
                print!("{} ", Syntax::DBG_PROMPT);
                let _ = std::io::stdout().flush();
                let mut line = String::new();
                match std::io::stdin().read_line(&mut line) {
                    Ok(0) => None,
                    Ok(_) => Some(line.trim().to_string()),
                    Err(_) => None,
                }
            }
            IO::Scripted { inputs, out, .. } => {
                let next = inputs.pop_front()?;
                out.push_str(&format!("{} {}\n", Syntax::DBG_PROMPT, next.trim()));
                Some(next.trim().to_string())
            }
        }
    }

    fn source_line(&self, line: usize) -> &str {
        self.jet_src
            .lines()
            .nth(line.saturating_sub(1))
            .unwrap_or("")
    }

    /// A stop banner + source window, in the SAME shape as the step-1
    /// interpreter debugger (`Debugger::show_stop`, this crate):
    /// `breakpoint hit  file:line  in fn()`, a two-line window with `<- here`.
    fn print_stop_banner(&mut self, func: &str, file: &str, line: usize) {
        if self.raw_frames {
            self.started = true;
            self.emit(&format!(
                "[raw] breakpoint hit  {}:{}  in {}()",
                file, line, func
            ));
            return;
        }
        if !self.started {
            self.started = true;
            let banner = format!("breakpoint hit  {}:{}  in {}()", file, line, func);
            self.emit(&banner);
        }
        for l in line.saturating_sub(1)..=line {
            if l == 0 {
                continue;
            }
            let text = self.source_line(l).to_string();
            if text.is_empty() && l != line {
                continue;
            }
            let caret = if l == line { "        <- here" } else { "" };
            let rendered = format!("   {} | {}{}", l, text.trim_end(), caret);
            self.emit(&rendered);
        }
    }

    /// Flush any Jet `print()`/`eprint()` output the debuggee wrote since the
    /// last check (module doc point 3 in `Inferior.rs`) into the transcript
    /// BEFORE the stop banner, matching program-output-then-pause ordering.
    fn drain_program_output(&mut self) {
        let (out, err) = self.inf.drain_program_output();
        if !out.trim_end().is_empty() {
            let text = out.trim_end().to_string();
            self.emit(&text);
        }
        if !err.trim_end().is_empty() {
            let text = err.trim_end().to_string();
            self.emit(&text);
        }
    }

    /// Handle the outcome of a resuming command (`run`/a step/`continue`),
    /// already resolved via `Inferior::resume_and_locate`'s follow-up `bt` (see
    /// `Inferior.rs`'s module doc on why the resume command's OWN text is
    /// never trusted for frame info).
    fn handle_resume(&mut self, result: ResumeResult) {
        self.drain_program_output();
        match result {
            ResumeResult::Exited { status, signal } => {
                self.exited = true;
                self.exit_code = status.unwrap_or(if signal.is_some() { 128 } else { 0 });
                if let Some(signal) = signal {
                    self.emit(&format!("program terminated by signal {}", signal));
                } else if self.exit_code == ExitCodes::OK {
                    self.emit("program finished");
                } else {
                    self.emit(&format!("program exited with status {}", self.exit_code));
                }
            }
            ResumeResult::Stopped(bt_text) => match Inferior::parse_top_frame(&bt_text) {
                Some(frame) => {
                    let func = if self.raw_frames {
                        frame.func.clone()
                    } else {
                        Inferior::safe_jet_func(&frame.func)
                    };
                    self.show_frame(&func, &frame.rust_file, frame.rust_line, true)
                }
                None => {
                    // A raw debugger transcript is an expert view only. The
                    // default projection reports an honest, Jet-level state.
                    let trimmed = bt_text.trim_end();
                    if self.raw_frames && !trimmed.is_empty() {
                        self.emit(&format!("[raw] {}", trimmed));
                    } else {
                        self.no_jet_frame();
                    }
                }
            },
        }
    }

    /// Show a stopped frame: `--raw-frames` (D-DBG2) shows the raw Rust
    /// file:line; the default view translates through `LineMap` and steps over
    /// transparently (bounded) when a frame has no Jet line (I2).
    fn show_frame(&mut self, func: &str, rust_file: &str, rust_line: usize, allow_step_over: bool) {
        if self.raw_frames {
            self.print_stop_banner(func, rust_file, rust_line);
            return;
        }
        match self.map.jet_line_for(rust_line) {
            Some(jline) => {
                let file = self.jet_file.clone();
                self.print_stop_banner(func, &file, jline)
            }
            None if allow_step_over => self.step_over_unmapped(),
            None => {
                // Exhausted the retry budget — never fall back to a generated
                // Rust location in the default Jet projection.
                self.no_jet_frame();
            }
        }
    }

    fn no_jet_frame(&mut self) {
        self.emit("debugger stopped outside Jet source; no Jet frame is available");
    }

    /// I2: a frame with no Jet line (prelude/generated glue) is never shown by
    /// default — step over it and re-check, bounded so a run into
    /// library code with no way back can't hang the session forever.
    fn step_over_unmapped(&mut self) {
        for _ in 0..MAX_STEP_OVER_UNMAPPED {
            let result = match self.inf.resume_and_locate("thread step-over") {
                Ok(r) => r,
                Err(_) => {
                    self.no_jet_frame();
                    return;
                }
            };
            self.drain_program_output();
            match result {
                ResumeResult::Exited { status, signal } => {
                    self.exited = true;
                    self.exit_code = status.unwrap_or(if signal.is_some() { 128 } else { 0 });
                    if let Some(signal) = signal {
                        self.emit(&format!("program terminated by signal {}", signal));
                    } else if self.exit_code == ExitCodes::OK {
                        self.emit("program finished");
                    } else {
                        self.emit(&format!("program exited with status {}", self.exit_code));
                    }
                    return;
                }
                ResumeResult::Stopped(bt_text) => match Inferior::parse_top_frame(&bt_text) {
                    Some(frame) => {
                        if let Some(jline) = self.map.jet_line_for(frame.rust_line) {
                            let file = self.jet_file.clone();
                            let func = Inferior::safe_jet_func(&frame.func);
                            self.print_stop_banner(&func, &file, jline);
                            return;
                        }
                    }
                    None => {
                        self.no_jet_frame();
                        return;
                    }
                },
            }
        }
        self.no_jet_frame();
    }

    fn render_locals(&mut self) -> String {
        match self.inf.locals() {
            Ok(out) => {
                let body: Vec<String> = Inferior::parse_typed_locals(&out)
                    .iter()
                    .filter_map(|(ty, n, v)| {
                        if self.raw_frames {
                            return Some(format!("{} : {} = {}", n, ty, v));
                        }
                        if !Inferior::rust_local_is_jet_visible(n) {
                            return None;
                        }
                        let jn = Inferior::rust_local_to_jet(n)?;
                        let value = Inferior::safe_value(ty, v);
                        Some(match Inferior::jet_type_name(ty) {
                            Some(ty) => format!("{} : {} = {}", jn, ty, value),
                            None => format!("{} = {}", jn, value),
                        })
                    })
                    .collect();
                if body.is_empty() {
                    return if self.raw_frames {
                        "[raw] locals:  (none)".to_string()
                    } else {
                        "locals:  (none)".to_string()
                    };
                }
                let prefix = if self.raw_frames {
                    "[raw] locals:  "
                } else {
                    "locals:  "
                };
                format!("{}{}", prefix, body.join("   "))
            }
            Err(_) => "locals:  (none)".to_string(),
        }
    }

    fn cmd_break(&mut self, arg: Option<&str>) {
        let Some(n) = arg
            .and_then(|a| a.parse::<usize>().ok())
            .filter(|n| *n >= 1)
        else {
            self.emit("break needs a line number, e.g. `break 7`");
            return;
        };
        match self.map.rust_line_for(n) {
            Some(rust_line) => {
                let rust_file = self.rust_file.clone();
                if self.inf.set_breakpoint(&rust_file, rust_line).is_err() {
                    self.emit("couldn't set the breakpoint");
                    return;
                }
                let file = self.jet_file.clone();
                self.emit(&format!("breakpoint set  {}:{}", file, n));
            }
            None => self.emit(&format!(
                "line {} has no statement to break on (blank line, comment, or a declaration)",
                n
            )),
        }
    }

    fn cmd_list(&mut self, around: Option<usize>) {
        let line = around.unwrap_or(1);
        let lo = line.saturating_sub(2).max(1);
        let hi = line + 2;
        for l in lo..=hi {
            let text = self.source_line(l).to_string();
            if text.is_empty() && l > line {
                break;
            }
            let marker = if l == line { "->" } else { "  " };
            self.emit(&format!("{} {} | {}", marker, l, text.trim_end()));
        }
    }

    fn cmd_backtrace(&mut self) {
        let out = match self.inf.backtrace() {
            Ok(o) => o,
            Err(_) => {
                self.emit("couldn't get the backtrace");
                return;
            }
        };
        let frames = Inferior::parse_frames(&out);
        let jet_file = self.jet_file.clone();
        for (i, f) in frames.iter().enumerate() {
            if self.raw_frames {
                self.emit(&format!(
                    "[raw] #{}  {}()  at {}:{}",
                    i, f.func, f.rust_file, f.rust_line
                ));
            } else if let Some(jline) = self.map.jet_line_for(f.rust_line) {
                let func = Inferior::safe_jet_func(&f.func);
                self.emit(&format!("#{}  {}()  at {}:{}", i, func, jet_file, jline));
            }
            // A no-Jet-line frame is skipped (I2) — it never had a source line
            // to show, and `--raw-frames` is the expert opt-in that would show it.
        }
    }

    fn cmd_print(&mut self, arg: Option<&str>) {
        let Some(name) = arg else {
            self.emit("print needs a name, e.g. `print total`");
            return;
        };
        let name = name.to_string();
        // `Inferior::print_var` already translates `name` to its mangled Rust
        // form to query lldb; a single-name query returns at most one pair.
        match self.inf.print_var(&name) {
            Ok(out) => match Inferior::parse_typed_locals(&out).into_iter().next() {
                Some((ty, raw_name, value)) if self.raw_frames => {
                    self.emit(&format!("[raw] {} : {} = {}", raw_name, ty, value))
                }
                Some((ty, _, value)) => {
                    let value = Inferior::safe_value(&ty, &value);
                    self.emit(&format!("{} = {}", name, value));
                }
                None => self.emit(&format!("no local named `{}` in this frame", name)),
            },
            Err(_) => self.emit(&format!("couldn't read `{}`", name)),
        }
    }

    fn cmd_help(&mut self) {
        self.emit(
            "\
commands:
  step, s        run the next line (descend into calls)
  next, n        run the next line (step over calls)
  finish, f      run to the end of this function
  continue, c    run to the next breakpoint
  break N, b N   set a breakpoint on line N
  list, l        show the source around the current line
  print X, p X   show the value of local X
  locals         show every local in this frame
  backtrace, bt  show the Jet call stack
  why X == V     query a source-level recorded receipt
  when X         query a source-level recorded receipt
  help, h        show this list
  quit, q        end the debug session
  (native backend — steps the full feature set, incl. FFI/tasks/#Unsafe)",
        );
    }

    /// Run the `(jet)` prompt until the program exits or the user quits.
    /// Returns the process exit code and whether scripted input ended at a
    /// live stop.
    fn prompt_loop(&mut self) -> (i32, bool) {
        loop {
            if self.exited {
                return (self.exit_code, false);
            }
            let cmd = match self.read_command() {
                Some(c) => c,
                None => {
                    if self.io.pause_on_input_end() {
                        return (ExitCodes::OK, true);
                    }
                    self.run_to_completion();
                    return (self.exit_code, false);
                }
            };
            if cmd.is_empty() {
                self.step(false);
                continue;
            }
            let mut parts = cmd.split_whitespace();
            let verb = parts.next().unwrap_or("");
            let arg = parts.next();
            match verb {
                v if v == Syntax::DBG_STEP || v == "s" => self.step(true),
                v if v == Syntax::DBG_NEXT || v == "n" => self.step(false),
                v if v == Syntax::DBG_FINISH || v == "f" => self.finish(),
                v if v == Syntax::DBG_CONTINUE || v == "c" => self.cont(),
                v if v == Syntax::DBG_BREAK || v == "b" => self.cmd_break(arg),
                v if v == Syntax::DBG_LIST || v == "l" => {
                    let cur = self.current_jet_line();
                    self.cmd_list(cur);
                }
                v if v == Syntax::DBG_PRINT || v == "p" => self.cmd_print(arg),
                v if v == Syntax::DBG_LOCALS => {
                    let rendered = self.render_locals();
                    self.emit(&rendered);
                }
                v if v == Syntax::DBG_BACKTRACE || v == "bt" => self.cmd_backtrace(),
                v if v == Syntax::DBG_WHY || v == Syntax::DBG_WHEN => self.emit(
                    "why/when need a source-level recorded receipt; native stepping has no act snapshots",
                ),
                v if v == Syntax::DBG_HELP || v == "h" => self.cmd_help(),
                v if v == Syntax::DBG_QUIT || v == "q" => {
                    self.emit("error [E2204]: debug session ended before the program finished");
                    return (ExitCodes::USER_ERROR, false);
                }
                other => self.emit(&format!(
                    "unknown command `{}` — type `help` for the verbs",
                    other
                )),
            }
        }
    }

    /// Best-effort: the Jet line of the frame we last showed (for a bare `list`).
    /// Re-reads the current frame via `bt`'s topmost entry rather than caching a
    /// stale line across arbitrary lldb state changes.
    fn current_jet_line(&mut self) -> Option<usize> {
        let out = self.inf.backtrace().ok()?;
        let top = Inferior::parse_frames(&out).into_iter().next()?;
        self.map.jet_line_for(top.rust_line)
    }

    fn step(&mut self, into: bool) {
        let cmd = if into {
            "thread step-in"
        } else {
            "thread step-over"
        };
        match self.inf.resume_and_locate(cmd) {
            Ok(r) => self.handle_resume(r),
            Err(_) => self.emit("step failed; debugger session is unavailable"),
        }
    }

    fn finish(&mut self) {
        match self.inf.resume_and_locate("thread step-out") {
            Ok(r) => self.handle_resume(r),
            Err(_) => self.emit("finish failed; debugger session is unavailable"),
        }
    }

    fn cont(&mut self) {
        match self.inf.resume_and_locate("continue") {
            Ok(r) => self.handle_resume(r),
            Err(_) => self.emit("continue failed; debugger session is unavailable"),
        }
    }

    fn run_to_completion(&mut self) {
        while !self.exited {
            match self.inf.resume_and_locate("continue") {
                Ok(result) => self.handle_resume(result),
                Err(_) => {
                    self.exit_code = ExitCodes::ICE;
                    self.exited = true;
                    self.emit("error: native debugger lost the running session");
                }
            }
        }
    }
}

pub(crate) fn snapshot_from_transcript(transcript: &str) -> Option<DebugSnapshot> {
    let line = transcript.lines().rev().find_map(|line| {
        let before_pipe = line.split('|').next()?.trim();
        let number = before_pipe.split_whitespace().last()?.parse().ok()?;
        line.contains("<- here").then_some(number)
    })?;
    let locals = transcript
        .lines()
        .rev()
        .find(|line| line.starts_with("locals:"))
        .map(|line| {
            line.trim_start_matches("locals:")
                .trim()
                .split("   ")
                .filter_map(|part| {
                    if part == "(none)" || part.is_empty() {
                        return None;
                    }
                    let (name_and_type, value) = part.split_once(" = ")?;
                    let (name, type_name) = name_and_type
                        .split_once(" : ")
                        .map(|(name, ty)| (name.trim(), ty.trim()))
                        .unwrap_or((name_and_type.trim(), ""));
                    Some(ValueSnapshot {
                        name: name.to_string(),
                        type_name: type_name.to_string(),
                        value: value.trim().to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let mut frames = transcript
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix('#')?.split_once("  ")?.1;
            let (function, location) = rest.split_once("()  at ")?;
            let line = location.rsplit_once(':')?.1.parse().ok()?;
            Some(FrameSnapshot {
                function: function.to_string(),
                line,
            })
        })
        .collect::<Vec<_>>();
    if frames.is_empty() {
        frames.push(FrameSnapshot {
            function: "run".to_string(),
            line,
        });
    } else {
        frames.reverse();
    }
    let function = frames
        .last()
        .map(|frame| frame.function.clone())
        .unwrap_or_else(|| "run".to_string());
    Some(DebugSnapshot {
        function,
        line,
        locals,
        call_stack: frames,
    })
}
