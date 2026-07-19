//! D-DBG1 / D-DBG3 — `jet debug <file>`: the source-level step debugger.
//!
//! The debugger steps your program at the **Jet source level** (I2): every
//! breakpoint, step, frame, and value is in Jet terms. It never surfaces the
//! generated Rust — values are shown through the interpreter's own
//! `CtValue::jet_show()` path, the same bytes a `print` would produce.
//!
//! Backend (D-DBG3 step 1, the shipped vertical slice): the debugger drives the
//! existing tree-walking interpreter (`crate::Comptime`) — the same engine
//! behind `jet dev` and `jet repl`. It registers a [`DebugHook`] that the
//! interpreter calls before every statement; the hook decides whether to pause
//! and run the interactive `(jet)` prompt. Because it reuses the dev
//! interpreter, it covers the same deterministic subset and declines the same
//! boundary (FFI / tasks / `@Unsafe` / native std) with **E2203**, pointing at
//! the native lldb-backed adapter shipped by D-DBG3 step 2.
//!
//! Command surface (D-DBG3, ratified A): the prompt is `(jet)`; the step verbs
//! are lldb-familiar `step` / `next` / `continue` / `finish` with single-letter
//! aliases `s` / `n` / `c` / `f`. A paused line shows a `<- here` caret and a
//! one-line `locals:` dump; `help` lists every verb. Only Jet frames/lines and
//! safe locals are shown (D-DBG2 — `--raw-frames` is the expert opt-in, a
//! follow-on once the native backend lands).
//!
//! I6: std-only — no DAP/JSON crate, no debugger library. The interactive loop
//! reads stdin with `std::io`; tests drive it with a scripted-input transcript
//! (`run_session`), the same shape as the REPL transcript tests.
//!
//! D-DBG3 step 2 (dap-debugger): the native backend lives in the sibling
//! submodules below. [`LineMap`] and [`Inferior`] are the shared building
//! blocks; [`Native`] is the `(jet)`-prompt terminal session; [`Dap`] is the
//! Debug Adapter Protocol server editors (VS Code/Zed) launch instead.

#![allow(non_snake_case)]
#![deny(warnings)]

// D-ARCH-SOURCE1=A: full debugger ownership lives here. Compiler semantics
// enter through inward path-only seams; no root host dependency exists.
pub use jet_driver::{AST, Comptime, Diagnostics, Loader, Sema, Syntax};
pub use jet_foundation::ExitCodes;

mod Dap;
mod EventObservation;
mod Inferior;
mod LineMap;
mod Native;

pub use EventObservation::render as render_event_observations;

use std::collections::{HashMap, HashSet};

use crate::Comptime::{CtValue, DebugHook, DevSink};
use crate::Diagnostics::{span_line_col, Diagnostic, Span};

/// One paused frame for the `backtrace` view: the executing function and the
/// Jet line it is stopped on. Newest (innermost) frame last.
#[derive(Clone)]
struct Frame {
    func: String,
    line: usize,
}

/// What the user asked the debugger to do next, set by a `(jet)` command and
/// read by the hook to decide where to stop.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Resume {
    /// `step`/`s`: stop at the very next statement, at any call depth.
    Step,
    /// `next`/`n`: stop at the next statement at this depth or shallower (step
    /// over a call without descending into it).
    Next { depth: usize },
    /// `finish`/`f`: run until control returns to a shallower frame.
    Finish { depth: usize },
    /// `continue`/`c`: run until the next breakpoint (or the program ends).
    Continue,
}

/// How the debugger reads its `(jet)` commands and where its output goes.
/// Interactive mode reads stdin and writes the real terminal; scripted mode
/// pops from a fixed input queue and appends output to an owned buffer so tests
/// can assert on the exact session (the same model as `REPL::run_transcript`).
enum Io {
    /// Live session: prompt + read a line from stdin, print to stdout.
    Interactive,
    /// Scripted session: a queue of typed inputs and an owned output buffer.
    Scripted {
        inputs: std::collections::VecDeque<String>,
        out: String,
    },
}

impl Io {
    /// True for the scripted (test/transcript) path.
    fn is_scripted(&self) -> bool {
        matches!(self, Io::Scripted { .. })
    }
    /// Append a line of debugger/program output (scripted buffer only; the
    /// interactive path writes the terminal directly at the call site).
    fn push_line(&mut self, s: &str) {
        if let Io::Scripted { out, .. } = self {
            out.push_str(s);
            out.push('\n');
        }
    }
    /// Take the captured transcript (empty for the interactive path).
    fn into_output(self) -> String {
        match self {
            Io::Scripted { out, .. } => out,
            Io::Interactive => String::new(),
        }
    }
}

/// The interactive driver. Holds the breakpoint set, the pending resume mode,
/// the source text (for the `<- here` caret and `list`), and the live call
/// stack. Implements [`DebugHook`] so the interpreter can call it per statement.
struct Debugger {
    /// Jet line numbers the user set a breakpoint on (`break <line>`).
    breakpoints: HashSet<usize>,
    /// What to do next; consumed/recomputed each time we pause.
    resume: Resume,
    /// The entry file's source, for caret/`list` and the breakpoint banner.
    src: String,
    /// The file name shown in banners (e.g. `loops.jet`).
    file: String,
    /// The live call stack, innermost last. Updated each time the hook fires.
    stack: Vec<Frame>,
    io: Io,
    /// True once the program has stopped at least once, so the first pause
    /// prints the full banner and later ones print just the moved line.
    started: bool,
    /// Set when the user typed `quit`: the hook returns E2204 to abort the run.
    quit: bool,
}

impl Debugger {
    fn new(src: String, file: String, io: Io) -> Self {
        Debugger {
            breakpoints: HashSet::new(),
            // Stop on the very first statement so the user lands inside `main`
            // (lldb/gdb behavior: `run` halts at the entry breakpoint).
            resume: Resume::Step,
            src,
            file,
            stack: Vec::new(),
            io,
            started: false,
            quit: false,
        }
    }

    /// 1-based Jet line for a span's start.
    fn line_of(&self, span: Span) -> usize {
        span_line_col(&self.src, span.start).0
    }

    /// The text of a 1-based source line, without the trailing newline.
    fn source_line(&self, line: usize) -> &str {
        self.src.lines().nth(line.saturating_sub(1)).unwrap_or("")
    }

    /// Emit a line of debugger output (to stdout or the transcript buffer).
    fn emit(&mut self, s: &str) {
        match &mut self.io {
            Io::Interactive => println!("{}", s),
            Io::Scripted { out, .. } => {
                out.push_str(s);
                out.push('\n');
            }
        }
    }

    /// Read the next `(jet)` command line. `None` means end-of-input: in a live
    /// session that is EOF (treat as `continue`); in a script it means the
    /// transcript ran out (also `continue` to let the program finish).
    fn read_command(&mut self) -> Option<String> {
        match &mut self.io {
            Io::Interactive => {
                use std::io::Write;
                print!("{} ", Syntax::DBG_PROMPT);
                let _ = std::io::stdout().flush();
                let mut line = String::new();
                match std::io::stdin().read_line(&mut line) {
                    Ok(0) => None, // EOF (Ctrl-D)
                    Ok(_) => Some(line.trim().to_string()),
                    Err(_) => None,
                }
            }
            Io::Scripted { inputs, out } => {
                let next = inputs.pop_front()?;
                // Echo the typed command after the prompt so the transcript
                // reads like a real session.
                out.push_str(&format!("{} {}\n", Syntax::DBG_PROMPT, next.trim()));
                Some(next.trim().to_string())
            }
        }
    }

    /// Print the stop banner: the file:line, the function, a small source
    /// window with a `<- here` caret on the current line, and the one-line
    /// `locals:` dump (D-DBG3 layout).
    fn show_stop(&mut self, line: usize, scope: &HashMap<String, CtValue>) {
        if !self.started {
            self.started = true;
            let func = self
                .stack
                .last()
                .map(|f| f.func.clone())
                .unwrap_or_else(|| "run".to_string());
            self.emit(&format!(
                "breakpoint hit  {}:{}  in {}()",
                self.file, line, func
            ));
        }
        // A two-line window (prev + current) mirrors the plan's worked example.
        for l in line.saturating_sub(1)..=line {
            if l == 0 {
                continue;
            }
            let text = self.source_line(l);
            if text.is_empty() && l != line {
                continue;
            }
            let caret = if l == line { "        <- here" } else { "" };
            self.emit(&format!("   {} | {}{}", l, text.trim_end(), caret));
        }
        self.emit(&render_locals(scope));
    }

    /// Run the `(jet)` prompt until the user issues a resume verb (step/next/
    /// continue/finish) or quits. `line`/`scope`/`depth` describe the current
    /// stop. Returns once `self.resume` is set for the run to proceed.
    fn prompt_loop(&mut self, line: usize, depth: usize, scope: &HashMap<String, CtValue>) {
        loop {
            let cmd = match self.read_command() {
                Some(c) => c,
                None => {
                    // End of input: let the program run to completion.
                    self.resume = Resume::Continue;
                    return;
                }
            };
            if cmd.is_empty() {
                // Bare Enter repeats the last step kind (lldb behavior): re-issue
                // a single step.
                self.resume = Resume::Step;
                return;
            }
            let mut parts = cmd.split_whitespace();
            let verb = parts.next().unwrap_or("");
            let arg = parts.next();
            match verb {
                v if v == Syntax::DBG_STEP || v == "s" => {
                    self.resume = Resume::Step;
                    return;
                }
                v if v == Syntax::DBG_NEXT || v == "n" => {
                    self.resume = Resume::Next { depth };
                    return;
                }
                v if v == Syntax::DBG_FINISH || v == "f" => {
                    self.resume = Resume::Finish { depth };
                    return;
                }
                v if v == Syntax::DBG_CONTINUE || v == "c" => {
                    self.resume = Resume::Continue;
                    return;
                }
                v if v == Syntax::DBG_BREAK || v == "b" => {
                    self.cmd_break(arg);
                }
                v if v == Syntax::DBG_LIST || v == "l" => {
                    self.cmd_list(line);
                }
                v if v == Syntax::DBG_PRINT || v == "p" => {
                    self.cmd_print(arg, scope);
                }
                v if v == Syntax::DBG_LOCALS => {
                    let rendered = render_locals(scope);
                    self.emit(&rendered);
                }
                v if v == Syntax::DBG_BACKTRACE || v == "bt" => {
                    self.cmd_backtrace();
                }
                v if v == Syntax::DBG_HELP || v == "h" => {
                    self.cmd_help();
                }
                v if v == Syntax::DBG_QUIT || v == "q" => {
                    self.quit = true;
                    return;
                }
                other => {
                    self.emit(&format!(
                        "unknown command `{}` — type `help` for the verbs",
                        other
                    ));
                }
            }
        }
    }

    fn cmd_break(&mut self, arg: Option<&str>) {
        match arg.and_then(|a| a.parse::<usize>().ok()) {
            Some(n) if n >= 1 => {
                self.breakpoints.insert(n);
                self.emit(&format!("breakpoint set  {}:{}", self.file, n));
            }
            _ => self.emit("break needs a line number, e.g. `break 7`"),
        }
    }

    fn cmd_list(&mut self, line: usize) {
        let lo = line.saturating_sub(2).max(1);
        let hi = line + 2;
        for l in lo..=hi {
            let text = self.source_line(l);
            if text.is_empty() && l > line {
                break;
            }
            let marker = if l == line { "->" } else { "  " };
            self.emit(&format!("{} {} | {}", marker, l, text.trim_end()));
        }
    }

    fn cmd_print(&mut self, arg: Option<&str>, scope: &HashMap<String, CtValue>) {
        match arg {
            Some(name) => match scope.get(name) {
                Some(v) => {
                    let line = format!("{} = {}", name, v.jet_show());
                    self.emit(&line);
                }
                None => self.emit(&format!("no local named `{}` in this frame", name)),
            },
            None => self.emit("print needs a name, e.g. `print total`"),
        }
    }

    fn cmd_backtrace(&mut self) {
        // Innermost frame first (lldb `bt` order): index 0 = where we stopped.
        let frames: Vec<String> = self
            .stack
            .iter()
            .rev()
            .enumerate()
            .map(|(i, f)| format!("#{}  {}()  at {}:{}", i, f.func, self.file, f.line))
            .collect();
        let joined = frames.join("\n");
        self.emit(&joined);
    }

    fn cmd_help(&mut self) {
        let text = "\
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
  help, h        show this list
  quit, q        end the debug session";
        self.emit(text);
    }
}

impl DebugHook for Debugger {
    fn at_stmt(
        &mut self,
        func: &str,
        depth: usize,
        span: Span,
        scope: &HashMap<String, CtValue>,
    ) -> Result<(), Diagnostic> {
        let line = self.line_of(span);
        // Keep the live call stack in sync: frame `depth` is this function at
        // this line. Drop any deeper frames left by a returned callee, then
        // update the current frame (or push it if we just descended).
        self.stack.truncate(depth + 1);
        if self.stack.len() <= depth {
            self.stack.push(Frame {
                func: func.to_string(),
                line,
            });
        } else if let Some(top) = self.stack.last_mut() {
            top.func = func.to_string();
            top.line = line;
        }

        let hit_breakpoint = self.breakpoints.contains(&line);
        let should_stop = match self.resume {
            Resume::Step => true,
            Resume::Next { depth: d } => depth <= d,
            Resume::Finish { depth: d } => depth < d,
            Resume::Continue => false,
        } || hit_breakpoint;

        if !should_stop {
            return Ok(());
        }

        self.show_stop(line, scope);
        self.prompt_loop(line, depth, scope);
        if self.quit {
            return Err(Diagnostic::error(
                "E2204",
                "debug session ended before the program finished".to_string(),
                "you typed `quit` at the `(jet)` prompt, which stops the interpreted run".to_string(),
                "run `jet debug <file>` again and use `continue` to run to the end, or `jet run <file>` to run without the debugger".to_string(),
                Some(span),
            ));
        }
        Ok(())
    }
}

/// Render the one-line `locals:` dump (D-DBG3): `locals:  a = 1   b = "hi"`.
/// Names are shown in a stable (sorted) order so the transcript is
/// deterministic. Values come through `jet_show` (I2 — Jet display, not Rust).
fn render_locals(scope: &HashMap<String, CtValue>) -> String {
    if scope.is_empty() {
        return "locals:  (none)".to_string();
    }
    let mut names: Vec<&String> = scope.keys().collect();
    names.sort();
    let body: Vec<String> = names
        .iter()
        .map(|n| format!("{} = {}", n, scope[*n].jet_show()))
        .collect();
    format!("locals:  {}", body.join("   "))
}

/// Load + check `file`, then run it under the debugger driving `io`. Returns
/// the process exit code and the captured transcript (empty in interactive
/// mode, where output already went to the terminal).
fn run_with_io(file: &str, mut io: Io) -> (i32, String) {
    let scripted = io.is_scripted();
    let bundle = match crate::Loader::load_entry(file) {
        Ok(b) => b,
        Err(diags) => {
            for d in &diags {
                let line = format!("error [{}]: {}", d.code, d.what);
                if scripted {
                    io.push_line(&line);
                } else {
                    eprintln!("{}", line);
                }
            }
            return (ExitCodes::USER_ERROR, io.into_output());
        }
    };
    let mut bundle = bundle;
    let diags = crate::Sema::check_bundle(&mut bundle, crate::Sema::CompileMode::Run);
    let errors: Vec<Diagnostic> = diags
        .into_iter()
        .filter(|d| matches!(d.severity, crate::Diagnostics::Severity::Error))
        .collect();
    let src = bundle.modules[bundle.entry].source.clone();
    if !errors.is_empty() {
        emit_diags(&mut io, file, &src, &errors);
        return (ExitCodes::USER_ERROR, io.into_output());
    }
    // The debugger steps the dev interpreter, so it declines the same features
    // `jet dev` does — but with E2203 (debug-specific): names `jet debug` and
    // points at the real build (D-DBG3 step 2, the native backend follow-on).
    if let Some(b) = jet_driver::InterpreterBoundary::debug_boundary_scan(&bundle) {
        emit_diags(&mut io, file, &src, &[b]);
        return (ExitCodes::USER_ERROR, io.into_output());
    }
    run_checked(&bundle, file, io)
}

fn emit_diags(io: &mut Io, file: &str, src: &str, diags: &[Diagnostic]) {
    let rendered = crate::Diagnostics::render_all(file, src, diags);
    match io {
        Io::Scripted { out, .. } => out.push_str(&rendered),
        Io::Interactive => eprint!("{}", rendered),
    }
}

/// Drive the interpreter with the debugger hook attached, then flush the
/// program's own stdout/stderr around the debugger's `(jet)` session.
fn run_checked(bundle: &crate::AST::ProgramBundle, file: &str, mut io: Io) -> (i32, String) {
    let funcs = collect_funcs(bundle);
    let main = match funcs.get("run") {
        Some(f) => *f,
        None => {
            let line = "this program has no `run` to debug — `jet debug` runs a program";
            if io.is_scripted() {
                io.push_line(line);
            } else {
                eprintln!("{}", line);
            }
            return (ExitCodes::USER_ERROR, io.into_output());
        }
    };
    let base_dir = &bundle.project_root;
    let src = bundle.modules[bundle.entry].source.clone();
    let short = std::path::Path::new(file)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| file.to_string());

    let scripted = io.is_scripted();
    let mut sink = DevSink::new();
    let mut dbg = Debugger::new(src, short, io);
    let result = crate::Comptime::run_main_debug(main, &funcs, base_dir, &mut sink, &mut dbg);

    // Flush the program's buffered output, then a completion line, then return
    // the captured transcript (scripted) or write the terminal (interactive).
    let code = match &result {
        Ok(()) => ExitCodes::OK,
        Err(_) => ExitCodes::USER_ERROR,
    };
    if scripted {
        if !sink.stdout.is_empty() {
            dbg.io.push_line(sink.stdout.trim_end_matches('\n'));
        }
        if !sink.stderr.is_empty() {
            dbg.io.push_line(sink.stderr.trim_end_matches('\n'));
        }
        match &result {
            Ok(()) => dbg.io.push_line("program finished"),
            Err(d) => dbg.io.push_line(&format!("[{}] {}", d.code, d.what)),
        }
        (code, dbg.io.into_output())
    } else {
        print!("{}", sink.stdout);
        eprint!("{}", sink.stderr);
        match &result {
            Ok(()) => println!("program finished"),
            Err(d) => eprintln!("[{}] {}", d.code, d.what),
        }
        (code, String::new())
    }
}

/// Collect top-level functions into the flat name→func map the interpreter
/// expects (mirrors `Interpreter::collect_funcs`).
fn collect_funcs(bundle: &crate::AST::ProgramBundle) -> HashMap<String, &crate::AST::Func> {
    let mut funcs = HashMap::new();
    for module in &bundle.modules {
        for item in &module.items {
            if let crate::AST::Item::Func(f) = item {
                funcs.entry(f.name.clone()).or_insert(f);
            }
        }
    }
    funcs
}

/// `jet debug <file>` — the interactive entry point (D-DBG1). Loads, checks,
/// and steps the program with a live `(jet)` prompt on stdin/stdout. Returns
/// the process exit code.
pub fn run_debug(file: &str) -> i32 {
    let (code, _captured) = run_with_io(file, Io::Interactive);
    code
}

/// Scripted debug session for tests and golden transcripts. Feeds `inputs` to
/// the `(jet)` prompt in order and returns the captured transcript (banners,
/// `locals:` dumps, command echoes, program output, and the final marker).
pub fn run_session(file: &str, inputs: &[&str]) -> String {
    let queue: std::collections::VecDeque<String> = inputs.iter().map(|s| s.to_string()).collect();
    let io = Io::Scripted {
        inputs: queue,
        out: String::new(),
    };
    let (_code, captured) = run_with_io(file, io);
    captured
}

/// D-DBG3 step 2 (dap-debugger): whether this program needs the native backend
/// — the interpreter's boundary scan (E2203) found an FFI/task/`@Unsafe`/
/// native-std construct step-1 can't step through. `None` means the file
/// couldn't even be loaded; the caller should fall through to [`run_debug`],
/// which reports that error the normal way (no duplicated error path here).
pub fn needs_native(file: &str) -> Option<bool> {
    let bundle = crate::Loader::load_entry(file).ok()?;
    Some(jet_driver::InterpreterBoundary::debug_boundary_scan(&bundle).is_some())
}

/// D-DBG3 step 2: the native lldb-backed `(jet)` terminal session — steps the
/// FULL feature set (FFI/tasks/`@Unsafe`/native std) the interpreter declines.
/// `binary` is the already-built debug binary (full debuginfo); `rust_file`/
/// `rust_src` are the generated Rust this build produced (with `// jet:line N`
/// markers from `emit_bundle_dbg`); `jet_file`/`jet_src` are the original
/// source. `raw_frames` is the D-DBG2 expert opt-in (`--raw-frames`).
pub fn run_native(
    binary: &std::path::Path,
    rust_file: &str,
    rust_src: &str,
    jet_file: &str,
    jet_src: &str,
    raw_frames: bool,
) -> i32 {
    Native::run(binary, rust_file, rust_src, jet_file, jet_src, raw_frames)
}

/// Scripted native session for tests (mirrors [`run_session`] for the native
/// backend): feeds `inputs` to the `(jet)` prompt and returns the transcript.
/// Callers should gate on lldb availability themselves before building a debug
/// binary to hand in — this only reports "lldb missing" into the transcript.
#[allow(clippy::too_many_arguments)]
pub fn run_native_scripted(
    binary: &std::path::Path,
    rust_file: &str,
    rust_src: &str,
    jet_file: &str,
    jet_src: &str,
    raw_frames: bool,
    inputs: &[&str],
) -> String {
    Native::run_scripted(
        binary, rust_file, rust_src, jet_file, jet_src, raw_frames, inputs,
    )
}

/// D-DBG3 step 2: the DAP JSON-over-stdio server (editor wiring) — same native
/// backend as [`run_native`], speaking the Debug Adapter Protocol on stdin/
/// stdout instead of the `(jet)` terminal prompt.
pub fn run_dap(
    binary: &std::path::Path,
    rust_file: &str,
    rust_src: &str,
    jet_file: &str,
    jet_src: &str,
) -> i32 {
    Dap::run(binary, rust_file, rust_src, jet_file, jet_src)
}
