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
//! documented, stable batch-mode text shapes. The live conformance test gates
//! only on the required external tools being absent; once lldb is available,
//! a failure is a test failure rather than a skipped claim.

use std::collections::HashSet;
use std::fmt::Write as FmtWrite;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use super::Inferior::{Inferior, ResumeResult};
use super::LineMap::LineMap;
use super::{
    DebugCheckpoint, DebugEvent, DebugHistory, DebugSnapshot, DebugWrite, FrameSnapshot,
    ValueSnapshot,
};
use crate::ExitCodes;
use crate::Syntax;

/// A bound on auto step-over retries when a stop lands on a frame with no Jet
/// line (prelude/generated glue) — avoids hanging forever if lldb never
/// reaches mapped code (e.g. it ran off into library code with no way back).
const MAX_STEP_OVER_UNMAPPED: usize = 200;

static NATIVE_CANDIDATE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Compiler-published, typed call replacement metadata.  The native adapter
/// never guesses a Rust ABI from a source name: both the host and candidate
/// artifacts must publish this complete row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NativeCallableMetadata {
    pub(crate) schema: u32,
    pub(crate) artifact: String,
    pub(crate) name: String,
    pub(crate) jet: String,
    pub(crate) entry: String,
    pub(crate) install: String,
    pub(crate) signature: String,
    pub(crate) body: String,
    pub(crate) state: String,
}

pub(crate) struct NativeCandidateArtifact {
    pub(crate) dir: PathBuf,
    pub(crate) binary: PathBuf,
    pub(crate) build_id: String,
    pub(crate) rust_source: String,
}

impl Drop for NativeCandidateArtifact {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn native_marker_value<'a>(fields: &'a [(&str, &'a str)], key: &str) -> Option<&'a str> {
    fields.iter().find_map(|(field, value)| (*field == key).then_some(*value))
}

fn native_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

pub(crate) fn native_callable_metadata(source: &str) -> Result<Vec<NativeCallableMetadata>, String> {
    let mut rows = Vec::new();
    for line in source.lines() {
        let Some(payload) = line.trim().strip_prefix("// jet-native-callable:") else {
            continue;
        };
        let fields = payload
            .split_whitespace()
            .filter_map(|field| field.split_once('='))
            .collect::<Vec<_>>();
        let schema = native_marker_value(&fields, "schema")
            .and_then(|value| value.parse::<u32>().ok())
            .ok_or_else(|| "native callable metadata has no valid schema".to_string())?;
        let artifact = native_marker_value(&fields, "artifact")
            .ok_or_else(|| "native callable metadata has no artifact identity".to_string())?;
        let name = native_marker_value(&fields, "name")
            .ok_or_else(|| "native callable metadata has no compiler symbol".to_string())?;
        let jet = native_marker_value(&fields, "jet")
            .ok_or_else(|| "native callable metadata has no Jet name".to_string())?;
        let entry = native_marker_value(&fields, "entry")
            .ok_or_else(|| "native callable metadata has no candidate entry".to_string())?;
        let install = native_marker_value(&fields, "install")
            .ok_or_else(|| "native callable metadata has no host installer".to_string())?;
        let signature = native_marker_value(&fields, "signature")
            .ok_or_else(|| "native callable metadata has no signature identity".to_string())?;
        let body = native_marker_value(&fields, "body")
            .ok_or_else(|| "native callable metadata has no body identity".to_string())?;
        let state = native_marker_value(&fields, "state")
            .ok_or_else(|| "native callable metadata has no state identity".to_string())?;
        if !native_identifier(artifact)
            || !native_identifier(name)
            || !native_identifier(entry)
            || !native_identifier(install)
            || !native_identifier(signature)
            || !native_identifier(body)
            || state != "none"
            || jet.is_empty()
            || jet.bytes().any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
        {
            return Err("native callable metadata contains an unsafe identity".to_string());
        }
        rows.push(NativeCallableMetadata {
            schema,
            artifact: artifact.to_string(),
            name: name.to_string(),
            jet: jet.to_string(),
            entry: entry.to_string(),
            install: install.to_string(),
            signature: signature.to_string(),
            body: body.to_string(),
            state: state.to_string(),
        });
    }
    rows.sort_by(|left, right| left.name.cmp(&right.name));
    rows.dedup_by(|left, right| left.name == right.name);
    Ok(rows)
}

/// Build and verify the candidate Rust shared object before any stopped
/// process is changed.  The semantic check is repeated here so callers cannot
/// accidentally turn a source-only overlay into a native artifact.
pub(crate) fn build_native_candidate(
    jet_file: &str,
    rust_file: &str,
    jet_src: &str,
) -> Result<NativeCandidateArtifact, String> {
    let (_bundle, candidate_source, _shape) =
        crate::checked_bundle_for_fix(jet_file)?;
    if candidate_source != jet_src {
        return Err("native candidate source changed while it was being checked".to_string());
    }
    let output = jet_driver::run_compiler_work(|| {
        jet_driver::Driver::compile_bundle_path_opts_dbg(
            jet_file,
            jet_driver::Sema::CompileMode::Run,
            false,
            jet_driver::Policy::GateSet::default(),
            false,
            true,
            None,
        )
    })
    .map_err(|diags| {
        diags
            .first()
            .map(|diag| format!("[{}] {}", diag.code, diag.what))
            .unwrap_or_else(|| "native candidate compiler rejected the source".to_string())
    })?;
    if output.rust != candidate_source {
        return Err("native candidate compiler source does not match checked source".to_string());
    }
    let callables = native_callable_metadata(&output.rust)?;
    if callables.is_empty() {
        return Err(
            "native candidate has no compiler-published typed replacement slots".to_string(),
        );
    }
    let source_digest = jet_foundation::SHA256::sha256_hex(output.rust.as_bytes());
    let serial = NATIVE_CANDIDATE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "jet-native-fix-{}-{serial}-{}",
        std::process::id(),
        &source_digest[..16]
    ));
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("could not create native candidate directory: {error}"))?;
    let rust_path = dir.join(rust_file);
    let binary = dir.join("candidate.so");
    let map_path = dir.join("candidate.jetmap");
    let cleanup = |error: String| {
        let _ = std::fs::remove_dir_all(&dir);
        error
    };
    if let Err(error) = std::fs::write(&rust_path, output.rust.as_bytes()) {
        return Err(cleanup(format!(
            "could not write native candidate source: {error}"
        )));
    }
    let crate_name = format!("jet_native_candidate_{serial}");
    let mut rustc = Command::new("rustc");
    rustc
        .args([
            "--crate-type",
            "cdylib",
            "--edition",
            "2021",
            "-C",
            "debuginfo=2",
            "-C",
            "opt-level=0",
            "--crate-name",
            &crate_name,
        ])
        .arg(&rust_path)
        .arg("-o")
        .arg(&binary);
    if let Some(link) = &output.ffi {
        rustc
            .arg("--extern")
            .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        for deps_dir in link.dependency_dirs().filter(|dir| dir.is_dir()) {
            rustc
                .arg("-L")
                .arg(format!("dependency={}", deps_dir.display()));
        }
    }
    rustc.args(&output.clinks);
    let compiled = rustc
        .output()
        .map_err(|error| cleanup(format!("could not run native candidate compiler: {error}")))?;
    if !compiled.status.success() {
        let stderr = String::from_utf8_lossy(&compiled.stderr);
        let detail = stderr.lines().take(8).collect::<Vec<_>>().join("\n");
        return Err(cleanup(format!(
            "native candidate compiler rejected the generated Rust{}",
            if detail.is_empty() {
                String::new()
            } else {
                format!(":\n{detail}")
            }
        )));
    }
    LineMap::write_artifact(
        &map_path,
        jet_file,
        jet_src,
        rust_file,
        &output.rust,
        &binary,
    )
    .map_err(|error| cleanup(format!("could not write native candidate map: {error}")))?;
    let _map = LineMap::load_verified(
        &map_path,
        jet_file,
        jet_src,
        rust_file,
        &output.rust,
        &binary,
    )
    .map_err(cleanup)?;
    let build_id = jet_foundation::SHA256::sha256_file_hex(&binary)
        .map(|digest| format!("sha256-{digest}"))
        .map_err(|error| cleanup(format!("could not hash native candidate: {error}")))?;
    Ok(NativeCandidateArtifact {
        dir,
        binary,
        build_id,
        rust_source: output.rust,
    })
}

/// Result of one command sent to a persistent native debugger boundary.
#[derive(Debug)]
pub struct NativeLiveResult {
    pub code: i32,
    pub paused: bool,
    pub transcript: String,
    pub snapshot: Option<DebugSnapshot>,
    pub history: DebugHistory,
}

/// A real stopped LLDB inferior retained by Canvas between HTTP requests.
/// Unlike the transcript/replay helpers, this object owns the process and
/// applies `(jet)` commands to that same process.
pub struct NativeLiveSession {
    session: Session,
}

impl NativeLiveSession {
    pub fn start(
        binary: &Path,
        rust_file: &str,
        rust_src: &str,
        jet_file: &str,
        jet_src: &str,
        raw_frames: bool,
    ) -> Result<(Self, NativeLiveResult), String> {
        if !Inferior::available() {
            return Err("native `jet debug` needs `lldb` on PATH".to_string());
        }
        let map_path = binary.with_extension("jetmap");
        LineMap::write_artifact(&map_path, jet_file, jet_src, rust_file, rust_src, binary)
            .map_err(|error| format!("native debugger could not write the source map: {error}"))?;
        let map = LineMap::load_verified(
            &map_path,
            jet_file,
            jet_src,
            rust_file,
            rust_src,
            binary,
        )?;
        let mut inf = Inferior::spawn(binary)
            .map_err(|error| format!("native debugger could not launch the compiled session: {error}"))?;
        let entry_line = map
            .main_entry_line(rust_src)
            .ok_or_else(|| "native debugger could not find a source-mapped entry statement".to_string())?;
        let entry_breakpoint_id = match inf
            .set_breakpoint(rust_file, entry_line)
            .map_err(|error| format!("native debugger could not set the source breakpoint: {error}"))?
        {
            breakpoint if breakpoint.resolved => Some(breakpoint.id),
            _ => {
                inf.quit();
                return Err("native debugger could not resolve the source breakpoint".to_string());
            }
        };
        let result = inf
            .resume_and_locate("run")
            .map_err(|error| format!("native debugger could not start the compiled session: {error}"))?;
        let mut session = Session {
            inf,
            map,
            rust_file: rust_file.to_string(),
            rust_src: rust_src.to_string(),
            jet_file: jet_file.to_string(),
            jet_src: jet_src.to_string(),
            raw_frames,
            started: false,
            entry_breakpoint_id,
            exited: false,
            exit_code: ExitCodes::OK,
            io: IO::Scripted {
                inputs: std::collections::VecDeque::new(),
                out: String::new(),
                pause_on_input_end: true,
            },
            snapshot: None,
            history: DebugHistory {
                events: Vec::new(),
                checkpoint: Some(DebugCheckpoint {
                    source_identity: jet_foundation::SHA256::sha256_hex(
                        format!("{}:{}", jet_file, jet_src).as_bytes(),
                    ),
                    shape: super::native_checkpoint_shape(jet_file, jet_src),
                    sequence: 0,
                }),
                cursor: None,
                capped: false,
                decision_ledger: None,
            },
            history_cursor: None,
            watches: HashSet::new(),
            breakpoints: HashSet::new(),
            candidate_artifacts: Vec::new(),
            replay_only_after_fix: false,
        };
        session.handle_resume(result);
        let initial = NativeLiveResult {
            code: ExitCodes::OK,
            paused: true,
            transcript: session.take_transcript(),
            snapshot: session.snapshot.clone(),
            history: session.history.clone(),
        };
        Ok((Self { session }, initial))
    }

    pub fn command(&mut self, command: &str) -> NativeLiveResult {
        let (code, paused) = self.session.run_single_command(command);
        NativeLiveResult {
            code,
            paused,
            transcript: self.session.take_transcript(),
            snapshot: self.session.snapshot.clone(),
            history: self.session.history.clone(),
        }
    }
}

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
    let (code, _captured, _paused, _snapshot, _history) = run_with_io(
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
    let (_code, captured, _paused, _snapshot, _history) = run_with_io(
        binary, rust_file, rust_src, jet_file, jet_src, raw_frames, io,
    );
    captured
}



pub(crate) fn run_scripted_mode_with_history(
    binary: &Path,
    rust_file: &str,
    rust_src: &str,
    jet_file: &str,
    jet_src: &str,
    raw_frames: bool,
    inputs: &[&str],
    pause_on_input_end: bool,
) -> (i32, String, bool, Option<DebugSnapshot>, DebugHistory) {
    let queue: std::collections::VecDeque<String> =
        inputs.iter().map(|s| s.to_string()).collect();
    let io = IO::Scripted {
        inputs: queue,
        out: String::new(),
        pause_on_input_end,
    };
    run_with_io(
        binary,
        rust_file,
        rust_src,
        jet_file,
        jet_src,
        raw_frames,
        io,
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
) -> (i32, String, bool, Option<DebugSnapshot>, DebugHistory) {
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
        return (
            ExitCodes::USER_ERROR,
            io_into_output(io),
            false,
            None,
            DebugHistory::default(),
        );
    }
    let map_path = binary.with_extension("jetmap");
    if let Err(_error) =
        LineMap::write_artifact(&map_path, jet_file, jet_src, rust_file, rust_src, binary)
    {
        report_error(
            &mut io,
            "error: native debugger could not write the source map",
        );
        return (
            ExitCodes::USER_ERROR,
            io_into_output(io),
            false,
            None,
            DebugHistory::default(),
        );
    }
    let map =
        match LineMap::load_verified(&map_path, jet_file, jet_src, rust_file, rust_src, binary) {
            Ok(map) => map,
            Err(_error) => {
                report_error(&mut io, "error: native debugger map is not usable");
                return (
                    ExitCodes::USER_ERROR,
                    io_into_output(io),
                    false,
                    None,
                    DebugHistory::default(),
                );
            }
        };
    let mut inf = match Inferior::spawn(binary) {
        Ok(i) => i,
        Err(_e) => {
            report_error(
                &mut io,
                "error: native debugger could not launch the compiled session",
            );
            return (
                ExitCodes::ICE,
                io_into_output(io),
                false,
                None,
                DebugHistory::default(),
            );
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
        // `Inferior` owns a live LLDB child.  Do not let an early setup
        // failure strand that child behind the terminal/DAP session.
        inf.quit();
        return (
            ExitCodes::ICE,
            io_into_output(io),
            false,
            None,
            DebugHistory::default(),
        );
    };
    let entry_breakpoint_id = match inf.set_breakpoint(rust_file, entry_line) {
        Ok(breakpoint) if breakpoint.resolved => Some(breakpoint.id),
        Ok(_) => {
            report_error(
                &mut io,
                "error: native debugger could not set the source breakpoint",
            );
            inf.quit();
            return (
                ExitCodes::ICE,
                io_into_output(io),
                false,
                None,
                DebugHistory::default(),
            );
        }
        Err(_) => {
            report_error(
                &mut io,
                "error: native debugger could not set the source breakpoint",
            );
            inf.quit();
            return (
                ExitCodes::ICE,
                io_into_output(io),
                false,
                None,
                DebugHistory::default(),
            );
        }
    };
    let result = match inf.resume_and_locate("run") {
        Ok(r) => r,
        Err(_e) => {
            report_error(
                &mut io,
                "error: native debugger could not start the compiled session",
            );
            inf.quit();
            return (
                ExitCodes::ICE,
                io_into_output(io),
                false,
                None,
                DebugHistory::default(),
            );
        }
    };
    let mut session = Session {
        inf,
        map,
        rust_file: rust_file.to_string(),
        rust_src: rust_src.to_string(),
        jet_file: jet_file.to_string(),
        jet_src: jet_src.to_string(),
        raw_frames,
        started: false,
        entry_breakpoint_id,
        exited: false,
        exit_code: ExitCodes::OK,
        io,
        snapshot: None,
        history: DebugHistory {
            events: Vec::new(),
            checkpoint: Some(DebugCheckpoint {
                source_identity: jet_foundation::SHA256::sha256_hex(
                    format!("{}:{}", jet_file, jet_src).as_bytes(),
                ),
                shape: super::native_checkpoint_shape(jet_file, jet_src),
                sequence: 0,
            }),
            cursor: None,
            capped: false,
            decision_ledger: None,
        },
        history_cursor: None,
        watches: HashSet::new(),
        breakpoints: HashSet::new(),
        candidate_artifacts: Vec::new(),
        replay_only_after_fix: false,
    };
    session.handle_resume(result);
    let (code, paused) = session.prompt_loop();
    let snapshot = session.snapshot.clone();
    let history = session.history.clone();
    let io = session.io;
    session.inf.quit();
    (code, io_into_output(io), paused, snapshot, history)
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
pub(crate) fn replay_history(
    jet_file: &str,
    jet_src: &str,
    source_history: &DebugHistory,
    inputs: &[&str],
    pause_on_input_end: bool,
) -> (i32, String, bool, Option<DebugSnapshot>, DebugHistory) {
    let mut history = source_history.clone();
    if history.events.is_empty() {
        return (
            ExitCodes::USER_ERROR,
            "reverse unavailable: native session contains no source history\n".to_string(),
            false,
            None,
            history,
        );
    }
    let mut output = String::new();
    let mut active_src = jet_src.to_string();
    let mut breakpoints = HashSet::new();
    let mut watches = HashSet::new();
    let mut cursor = history.cursor.unwrap_or(0);
    if cursor >= history.events.len() {
        return (
            ExitCodes::USER_ERROR,
            "reverse unavailable: native session cursor is outside retained history\n".to_string(),
            false,
            None,
            history,
        );
    }
    let mut snapshot = Some(replay_event(&mut output, jet_file, &active_src, &history, cursor));
    let mut finished = false;
    for command in inputs {
        let command = command.trim();
        let _ = writeln!(output, "{} {}", Syntax::DBG_PROMPT, command);
        let mut parts = command.split_whitespace();
        let verb = parts.next().unwrap_or("");
        let arg = parts.next();
        match verb {
            "break" | "b" => {
                if let Some(line) = arg.and_then(|value| value.parse::<usize>().ok()) {
                    breakpoints.insert(line);
                    let _ = writeln!(output, "breakpoint set  {}:{}", jet_file, line);
                }
            }
            "watch" => {
                if let Some(place) = arg {
                    if super::split_debug_expression(place).is_none() {
                        let _ = writeln!(
                            output,
                            "watch needs a bounded Jet local path, e.g. `watch account.balance`"
                        );
                    } else if watches.contains(place) {
                        let _ = writeln!(output, "watchpoint already set  {}", place);
                    } else if watches.len() >= 32 {
                        let _ = writeln!(output, "watchpoint limit reached (32 session watchpoints)");
                    } else {
                        watches.insert(place.to_string());
                        let _ = writeln!(output, "watchpoint set  {} (session scope)", place);
                    }
                } else {
                    let _ = writeln!(
                        output,
                        "watch needs a bounded Jet local path, e.g. `watch account.balance`"
                    );
                }
            }
            "unwatch" => {
                if let Some(place) = arg {
                    if watches.remove(place) {
                        let _ = writeln!(output, "watchpoint cleared  {}", place);
                    }
                }
            }
            "back" | "reverse" | "reverse-step" | "rstep" => {
                let steps = arg
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(1)
                    .max(1);
                if cursor < steps {
                    let _ = writeln!(output, "reverse unavailable: no earlier native stop exists");
                } else {
                    cursor -= steps;
                    snapshot = Some(replay_event(
                        &mut output,
                        jet_file,
                        &active_src,
                        &history,
                        cursor,
                    ));
                }
            }
            "reverse-continue" | "rcontinue" | "back-continue" => {
                let target = (0..cursor).rev().find(|index| {
                    history.events.get(*index).is_some_and(|event| {
                        breakpoints.contains(&event.line)
                            || event.writes.iter().any(|write| {
                                watches.iter().any(|watch| {
                                    watch == &write.place
                                        || write.place.starts_with(&format!("{watch}."))
                                        || watch.starts_with(&format!("{}.", write.place))
                                })
                            })
                    })
                });
                if let Some(target) = target {
                    cursor = target;
                    snapshot = Some(replay_event(
                        &mut output,
                        jet_file,
                        &active_src,
                        &history,
                        cursor,
                    ));
                } else {
                    let _ = writeln!(
                        output,
                        "reverse unavailable: no earlier native breakpoint or watchpoint"
                    );
                }
            }
            "step" | "s" | "next" | "n" | "finish" | "f" | "continue" | "c" => {
                let next = if verb == "continue" || verb == "c" {
                    (cursor + 1..history.events.len()).find(|index| {
                        history.events.get(*index).is_some_and(|event| {
                            breakpoints.contains(&event.line)
                                || event.writes.iter().any(|write| {
                                    watches.iter().any(|watch| {
                                        watch == &write.place
                                            || write.place.starts_with(&format!("{watch}."))
                                            || watch.starts_with(&format!("{}.", write.place))
                                    })
                                })
                        })
                    })
                } else {
                    (cursor + 1 < history.events.len()).then_some(cursor + 1)
                };
                if let Some(next) = next {
                    cursor = next;
                    snapshot = Some(replay_event(
                        &mut output,
                        jet_file,
                        &active_src,
                        &history,
                        cursor,
                    ));
                } else {
                    finished = true;
                    break;
                }
            }
            "list" | "l" => {
                if let Some(event) = history.events.get(cursor) {
                    for line in event.line.saturating_sub(2).max(1)..=event.line + 2 {
                        let text = active_src.lines().nth(line.saturating_sub(1)).unwrap_or("");
                        if text.is_empty() && line > event.line {
                            break;
                        }
                        let marker = if line == event.line { "->" } else { "  " };
                        let _ = writeln!(output, "{} {} | {}", marker, line, text.trim_end());
                    }
                }
            }
            "locals" => {
                if let Some(event) = history.events.get(cursor) {
                    let values = event
                        .locals
                        .iter()
                        .map(|value| format!("{} = {}", value.name, value.value))
                        .collect::<Vec<_>>();
                    let _ = writeln!(
                        output,
                        "locals:  {}",
                        if values.is_empty() {
                            "(none)".to_string()
                        } else {
                            values.join("   ")
                        }
                    );
                }
            }
            "print" | "p" => {
                if let Some(place) = arg {
                    let value = history
                        .events
                        .get(cursor)
                        .and_then(|event| event.locals.iter().find(|value| value.name == place));
                    if let Some(value) = value {
                        let _ = writeln!(output, "{} = {}", value.name, value.value);
                    } else {
                        let _ = writeln!(output, "no local named `{}` in this recorded frame", place);
                    }
                }
            }
            "bt" | "backtrace" => {
                if let Some(event) = history.events.get(cursor) {
                    let stack = if event.stack.is_empty() {
                        vec![super::FrameSnapshot {
                            function: event.function.clone(),
                            line: event.line,
                        }]
                    } else {
                        event.stack.clone()
                    };
                    for (frame, value) in stack.iter().rev().enumerate() {
                        let _ = writeln!(
                            output,
                            "#{}  {}()  at {}:{} (recorded act {})",
                            frame, value.function, jet_file, value.line, event.sequence
                        );
                    }
                }
            }
            "fix" => {
                let Ok(source_on_disk) = std::fs::read_to_string(jet_file) else {
                    let _ = writeln!(
                        output,
                        "fix rejected: the debug source could not be read"
                    );
                    continue;
                };
                if source_on_disk == active_src {
                    let _ = writeln!(
                        output,
                        "fix unavailable: source is unchanged; edit the file and retry"
                    );
                    continue;
                }
                let (_, candidate_source, candidate_shape) =
                    match super::checked_bundle_for_fix(jet_file) {
                        Ok(value) => value,
                        Err(reason) => {
                            let _ = writeln!(output, "fix rejected: {reason}");
                            continue;
                        }
                    };
                let expected_shape = history
                    .checkpoint
                    .as_ref()
                    .map(|checkpoint| checkpoint.shape.as_str());
                if expected_shape.is_some_and(|shape| {
                    !matches!(shape, "native-debug-artifact" | "native-dap-debug-artifact")
                        && shape != candidate_shape
                }) {
                    let _ = writeln!(
                        output,
                        "fix rejected: checkpoint incompatible (source type, layout, or authority changed); restart required"
                    );
                    continue;
                }
                let identity = jet_foundation::SHA256::sha256_hex(
                    format!("{}:{}", jet_file, candidate_source).as_bytes(),
                );
                if let Some(checkpoint) = history.checkpoint.as_mut() {
                    checkpoint.source_identity = identity;
                    checkpoint.shape = candidate_shape;
                } else {
                    history.checkpoint = Some(DebugCheckpoint {
                        source_identity: identity,
                        shape: candidate_shape,
                        sequence: history
                            .events
                            .last()
                            .map_or(0, |event| event.sequence),
                    });
                }
                active_src = candidate_source;
                history.cursor = Some(cursor);
                let _ = writeln!(output, "checkpoint compatible");
            }
            "quit" | "q" => {
                let _ = writeln!(output, "error [E2204]: debug session ended before the program finished");
                finished = true;
                break;
            }
            _ => {}
        }
    }
    history.cursor = Some(cursor);
    if !finished && pause_on_input_end {
        return (ExitCodes::OK, output, true, snapshot, history);
    }
    if !finished {
        output.push_str("program finished\n");
    }
    let code = if finished && inputs.iter().any(|input| input.trim() == "quit") {
        ExitCodes::USER_ERROR
    } else {
        ExitCodes::OK
    };
    (code, output, false, snapshot, history)
}

fn replay_event(
    output: &mut String,
    jet_file: &str,
    jet_src: &str,
    history: &DebugHistory,
    index: usize,
) -> super::DebugSnapshot {
    let event = history
        .events
        .get(index)
        .expect("replay event index checked by caller");
    let stack = if event.stack.is_empty() {
        vec![super::FrameSnapshot {
            function: event.function.clone(),
            line: event.line,
        }]
    } else {
        event.stack.clone()
    };
    let _ = writeln!(
        output,
        "breakpoint hit  {}:{}  in {}()  (recorded act {})",
        jet_file, event.line, event.function, event.sequence
    );
    for line in event.line.saturating_sub(1)..=event.line {
        let text = jet_src.lines().nth(line.saturating_sub(1)).unwrap_or("");
        if text.is_empty() && line != event.line {
            continue;
        }
        let caret = if line == event.line {
            "        <- here"
        } else {
            ""
        };
        let _ = writeln!(output, "   {} | {}{}", line, text.trim_end(), caret);
    }
    let locals = event
        .locals
        .iter()
        .map(|value| format!("{} = {}", value.name, value.value))
        .collect::<Vec<_>>();
    let _ = writeln!(
        output,
        "locals:  {}",
        if locals.is_empty() {
            "(none)".to_string()
        } else {
            locals.join("   ")
        }
    );
    for write in &event.writes {
        let _ = writeln!(
            output,
            "write: {}:{}  {} -> {}",
            jet_file, write.line, write.old_value, write.new_value
        );
    }
    super::DebugSnapshot {
        function: event.function.clone(),
        line: event.line,
        locals: event.locals.clone(),
        call_stack: stack,
    }
}


struct Session {
    inf: Inferior,
    map: LineMap,
    /// The generated Rust file's own name (what lldb's debug info points at —
    /// NOT the Jet file), for `breakpoint set -f <rust_file> -l <rust_line>`.
    rust_file: String,
    /// The generated Rust source currently loaded in the inferior.
    rust_src: String,
    jet_file: String,
    jet_src: String,
    raw_frames: bool,
    started: bool,
    /// Temporary breakpoint used to stop at `fn run`'s first statement. It
    /// must be retired after the initial stop or `continue` can immediately
    /// return to the same source line forever.
    entry_breakpoint_id: Option<usize>,
    exited: bool,
    exit_code: i32,
    io: IO,
    /// Safe Jet facts read directly from the stopped inferior.
    snapshot: Option<DebugSnapshot>,
    /// Canonical source event history. Reverse commands only navigate these
    /// snapshots; they never start a second native execution.
    history: DebugHistory,
    history_cursor: Option<usize>,
    watches: HashSet<String>,
    breakpoints: HashSet<usize>,
    /// Candidate shared objects stay alive for as long as LLDB can execute
    /// their function pointers.
    candidate_artifacts: Vec<NativeCandidateArtifact>,
    /// A failed or unavailable source repair may browse retained history, but
    /// it must never resume stale machine code.
    replay_only_after_fix: bool,
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

    fn take_transcript(&mut self) -> String {
        match &mut self.io {
            IO::Scripted { out, .. } => std::mem::take(out),
            IO::Interactive => String::new(),
        }
    }

    fn run_single_command(&mut self, command: &str) -> (i32, bool) {
        let previous = std::mem::replace(
            &mut self.io,
            IO::Scripted {
                inputs: std::collections::VecDeque::new(),
                out: String::new(),
                pause_on_input_end: true,
            },
        );
        let inherited = match previous {
            IO::Scripted { out, .. } => out,
            IO::Interactive => String::new(),
        };
        self.io = IO::Scripted {
            inputs: std::collections::VecDeque::from([command.to_string()]),
            out: inherited,
            pause_on_input_end: true,
        };
        self.prompt_loop()
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
                self.snapshot = None;
                self.exit_code = status.unwrap_or(if signal.is_some() { 128 } else { 0 });
                if let Some(signal) = signal {
                    self.emit(&format!("program terminated by signal {}", signal));
                } else if self.exit_code == ExitCodes::OK {
                    self.emit("program finished");
                } else {
                    self.emit(&format!("program exited with status {}", self.exit_code));
                }
            }
            ResumeResult::Stopped(bt_text) => {
                if !self.started {
                    self.retire_entry_breakpoint();
                    if self.exited {
                        self.snapshot = None;
                        return;
                    }
                }
                match Inferior::parse_top_frame(&bt_text) {
                    Some(frame) => {
                        if self
                            .map
                            .jet_line_for_file(&frame.rust_file, &self.rust_file, frame.rust_line)
                            .is_some()
                        {
                            self.capture_snapshot(&bt_text);
                        } else {
                            self.snapshot = None;
                        }
                        let func = if self.raw_frames {
                            frame.func.clone()
                        } else {
                            Inferior::safe_jet_func(&frame.func)
                        };
                        self.show_frame(&func, &frame.rust_file, frame.rust_line, true)
                    }
                    None => {
                        self.snapshot = None;
                        // A raw debugger transcript is an expert view only. The
                        // default projection reports an honest, Jet-level state.
                        let trimmed = bt_text.trim_end();
                        if self.raw_frames && !trimmed.is_empty() {
                            for line in trimmed.lines() {
                                self.emit(&format!("[raw] {}", line));
                            }
                        } else {
                            self.no_jet_frame();
                        }
                    }
                }
            }
        }
    }

    /// Capture the current stop from typed LLDB queries, not from the human
    /// transcript. Only source-mapped Jet frames and safe local values leave
    /// the native backend.
    fn capture_snapshot(&mut self, bt_text: &str) {
        let frames = Inferior::parse_frames(bt_text);
        let mut call_stack = frames
            .into_iter()
            .filter_map(|frame| {
                let line = self
                    .map
                    .jet_line_for_file(&frame.rust_file, &self.rust_file, frame.rust_line)?;
                Some(FrameSnapshot {
                    function: Inferior::safe_jet_func(&frame.func),
                    line,
                })
            })
            .collect::<Vec<_>>();
        if call_stack.is_empty() {
            self.snapshot = None;
            return;
        }
        call_stack.reverse();
        let Some(current) = call_stack.last() else {
            self.snapshot = None;
            return;
        };
        let locals = self
            .inf
            .locals()
            .ok()
            .map(|output| {
                Inferior::parse_typed_locals(&output)
                    .into_iter()

                    .filter_map(|(raw_type, raw_name, raw_value)| {
                        if !Inferior::rust_local_is_jet_visible(&raw_name) {
                            return None;
                        }
                        let name = Inferior::rust_local_to_jet(&raw_name)?;
                        let type_name = Inferior::jet_type_name(&raw_type)
                            .unwrap_or("Unknown")
                            .to_string();
                        let value = Inferior::safe_value(&raw_type, &raw_value);
                        Some(ValueSnapshot {
                            name,
                            type_name,
                            value,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.snapshot = Some(DebugSnapshot {
            function: current.function.clone(),
            line: current.line,
            locals,
            call_stack,
        });
        self.record_event();
    }
    fn record_event(&mut self) {
        let Some(snapshot) = self.snapshot.clone() else {
            return;
        };
        let sequence = u64::try_from(self.history.events.len() + 1).unwrap_or(u64::MAX);
        let previous = self.history.events.last();
        let mut writes = Vec::new();
        for value in &snapshot.locals {
            let old = previous.and_then(|event| {
                event.locals.iter().find(|candidate| candidate.name == value.name)
            });
            if old.is_some_and(|old| old.type_name == value.type_name && old.value == value.value) {
                continue;
            }
            writes.push(DebugWrite {
                line: snapshot.line,
                place: value.name.clone(),
                old_value: old.map_or_else(|| "<unbound>".to_string(), |old| old.value.clone()),
                new_value: value.value.clone(),
            });
        }
        let event = DebugEvent {
            sequence,
            function: snapshot.function.clone(),
            line: snapshot.line,
            depth: snapshot.call_stack.len().saturating_sub(1),
            stack: snapshot.call_stack.clone(),
            locals: snapshot.locals,
            writes,
        };
        if !self.history.push(event) {
            self.emit("debug history limit reached; older native stops are no longer retained");
        }
        self.history_cursor = None;
    }

    fn history_count(&self) -> usize {
        self.history
            .cursor
            .map_or(self.history.events.len(), |index| index.saturating_add(1))
    }

    fn show_history_stop(&mut self, index: usize) {
        let Some(event) = self.history.events.get(index).cloned() else {
            self.emit("reverse unavailable: native stop has been evicted");
            return;
        };
        let stack = if event.stack.is_empty() {
            vec![FrameSnapshot {
                function: event.function.clone(),
                line: event.line,
            }]
        } else {
            event.stack.clone()
        };
        self.snapshot = Some(DebugSnapshot {
            function: event.function.clone(),
            line: event.line,
            locals: event.locals.clone(),
            call_stack: stack,
        });
        self.emit(&format!(
            "breakpoint hit  {}:{}  in {}()  (recorded act {})",
            self.jet_file, event.line, event.function, event.sequence
        ));
        for line in event.line.saturating_sub(1)..=event.line {
            if line == 0 {
                continue;
            }
            let text = self.source_line(line);
            if text.is_empty() && line != event.line {
                continue;
            }
            let caret = if line == event.line {
                "        <- here"
            } else {
                ""
            };
            self.emit(&format!("   {} | {}{}", line, text.trim_end(), caret));
        }
        let locals = event
            .locals
            .iter()
            .map(|value| format!("{} = {}", value.name, value.value))
            .collect::<Vec<_>>();
        self.emit(&format!(
            "locals:  {}",
            if locals.is_empty() {
                "(none)".to_string()
            } else {
                locals.join("   ")
            }
        ));
        for write in event.writes {
            self.emit(&format!(
                "write: {}:{}  {} -> {}",
                self.jet_file, write.line, write.old_value, write.new_value
            ));
        }
    }

    fn reverse_step(&mut self, steps: usize) {
        let boundary = self.history_count();
        let steps = steps.max(1);
        if boundary < steps {
            self.emit("reverse unavailable: no earlier native stop exists");
            return;
        }
        let index = boundary - steps;
        self.history.cursor = Some(index);
        self.history_cursor = Some(index);
        self.show_history_stop(index);
    }

    fn reverse_continue(&mut self) {
        let boundary = self.history_count();
        let start = self.history.cursor.map_or(boundary, |index| index);
        let target = (0..start).rev().find(|index| {
            self.history.events.get(*index).is_some_and(|event| {
                self.breakpoints.contains(&event.line)
                    || event.writes.iter().any(|write| {
                        self.watches.iter().any(|watch| {
                            watch == &write.place
                                || write.place.starts_with(&format!("{watch}."))
                                || watch.starts_with(&format!("{}.", write.place))
                        })
                    })
            })
        });
        let Some(index) = target else {
            self.emit("reverse unavailable: no earlier native breakpoint or watchpoint");
            return;
        };
        self.history.cursor = Some(index);
        self.history_cursor = Some(index);
        self.show_history_stop(index);
    }

    fn advance_history(&mut self) -> bool {
        let Some(current) = self.history.cursor.or(self.history_cursor) else {
            return false;
        };
        let next = current.saturating_add(1);
        if next >= self.history.events.len() {
            if self.replay_only_after_fix {
                self.emit(
                    "fix unavailable: the checked source repair needs a rebuilt native artifact before live continue",
                );
                return true;
            }
            self.history.cursor = None;
            self.history_cursor = None;
            return false;
        }
        self.history.cursor = Some(next);
        self.history_cursor = Some(next);
        self.show_history_stop(next);
        true
    }


    fn retire_entry_breakpoint(&mut self) {
        let Some(id) = self.entry_breakpoint_id.take() else {
            return;
        };
        if self.inf.delete_breakpoint(id).is_err() {
            self.backend_lost("could not retire the temporary entry breakpoint");
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
        match self
            .map
            .jet_line_for_file(rust_file, &self.rust_file, rust_line)
        {
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
                    self.backend_lost("step failed; debugger session is unavailable");
                    return;
                }
            };
            self.drain_program_output();
            match result {
                ResumeResult::Exited { status, signal } => {
                    self.exited = true;
                    self.snapshot = None;
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
                        if let Some(jline) = self.map.jet_line_for_file(
                            &frame.rust_file,
                            &self.rust_file,
                            frame.rust_line,
                        ) {
                            self.capture_snapshot(&bt_text);
                            let file = self.jet_file.clone();
                            let func = Inferior::safe_jet_func(&frame.func);
                            self.print_stop_banner(&func, &file, jline);
                            return;
                        }
                        self.snapshot = None;
                    }
                    None => {
                        self.snapshot = None;
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
            Err(_) => {
                if self.raw_frames {
                    "[raw] locals:  (none)".to_string()
                } else {
                    "locals:  (none)".to_string()
                }
            }
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
                match self.inf.set_breakpoint(&rust_file, rust_line) {
                    Ok(breakpoint) if breakpoint.resolved => {}
                    Ok(_) => {
                        self.emit("couldn't set the breakpoint");
                        return;
                    }
                    Err(_) => {
                        self.emit("couldn't set the breakpoint");
                        return;
                    }
                }
                self.breakpoints.insert(n);
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
        if self.history_cursor.is_some() {
            self.cmd_backtrace_history();
            return;
        }
        let out = match self.inf.backtrace() {
            Ok(o) => o,
            Err(_) => {
                self.emit("couldn't get the backtrace");
                return;
            }
        };
        let frames = Inferior::parse_frames(&out);
        let jet_file = self.jet_file.clone();
        if frames.is_empty() {
            if self.raw_frames {
                for line in out.trim_end().lines() {
                    self.emit(&format!("[raw] {}", line));
                }
            } else {
                self.no_jet_frame();
            }
            return;
        }
        for (i, f) in frames.iter().enumerate() {
            if self.raw_frames {
                self.emit(&format!(
                    "[raw] #{}  {}()  at {}:{}",
                    i, f.func, f.rust_file, f.rust_line
                ));
            } else if let Some(jline) =
                self.map
                    .jet_line_for_file(&f.rust_file, &self.rust_file, f.rust_line)
            {
                let func = Inferior::safe_jet_func(&f.func);
                self.emit(&format!("#{}  {}()  at {}:{}", i, func, jet_file, jline));
            }
            // A no-Jet-line frame is skipped (I2) — it never had a source line
            // to show, and `--raw-frames` is the expert opt-in that would show it.
        }
    }

    fn cmd_print(&mut self, arg: Option<&str>) {
        if self.history_cursor.is_some() {
            self.cmd_print_history(arg);
            return;
        }
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
    fn cmd_watch(&mut self, arg: Option<&str>) {
        let Some(place) = arg
            .filter(|value| !value.is_empty())
            .filter(|value| super::split_debug_expression(value).is_some())
        else {
            self.emit("watch needs a bounded Jet local path, e.g. `watch account.balance`");
            return;
        };
        if self.watches.contains(place) {
            self.emit(&format!("watchpoint already set  {place}"));
            return;
        }
        if self.watches.len() >= 32 {
            self.emit("watchpoint limit reached (32 session watchpoints)");
            return;
        }
        self.watches.insert(place.to_string());
        self.emit(&format!("watchpoint set  {place} (session scope)"));
    }

    fn cmd_unwatch(&mut self, arg: Option<&str>) {
        let Some(place) = arg.filter(|value| !value.is_empty()) else {
            self.emit("unwatch needs a place, e.g. `unwatch total`");
            return;
        };
        if self.watches.remove(place) {
            self.emit(&format!("watchpoint cleared  {place}"));
        } else {
            self.emit(&format!("no watchpoint named `{place}`"));
        }
    }

    fn cmd_print_history(&mut self, arg: Option<&str>) {
        let Some(place) = arg else {
            self.emit("print needs a name, e.g. `print total`");
            return;
        };
        let Some(index) = self.history_cursor else {
            self.cmd_print(Some(place));
            return;
        };
        let Some(event) = self.history.events.get(index) else {
            self.emit("reverse unavailable: native stop has been evicted");
            return;
        };
        if let Some(value) = event.locals.iter().find(|value| value.name == place) {
            self.emit(&format!("{} = {}", value.name, value.value));
        } else {
            self.emit(&format!("no local named `{place}` in this recorded frame"));
        }
    }

    fn cmd_locals_history(&mut self) {
        let Some(index) = self.history_cursor else {
            let rendered = self.render_locals();
            self.emit(&rendered);
            return;
        };
        let Some(event) = self.history.events.get(index) else {
            self.emit("reverse unavailable: native stop has been evicted");
            return;
        };
        let values = event
            .locals
            .iter()
            .map(|value| format!("{} = {}", value.name, value.value))
            .collect::<Vec<_>>();
        self.emit(&format!(
            "locals:  {}",
            if values.is_empty() {
                "(none)".to_string()
            } else {
                values.join("   ")
            }
        ));
    }

    fn cmd_backtrace_history(&mut self) {
        let Some(index) = self.history_cursor else {
            self.cmd_backtrace();
            return;
        };
        let Some(event) = self.history.events.get(index) else {
            self.emit("reverse unavailable: native stop has been evicted");
            return;
        };
        let stack = if event.stack.is_empty() {
            vec![FrameSnapshot {
                function: event.function.clone(),
                line: event.line,
            }]
        } else {
            event.stack.clone()
        };
        let sequence = event.sequence;
        let jet_file = self.jet_file.clone();
        for (frame, value) in stack.iter().rev().enumerate() {
            self.emit(&format!(
                "#{}  {}()  at {}:{} (recorded act {})",
                frame, value.function, jet_file, value.line, sequence
            ));
        }
    }

    fn cmd_fix(&mut self) {
        if self.history.events.is_empty() {
            self.emit("fix unavailable: native session contains no source history");
            return;
        }
        let Ok(source_on_disk) = std::fs::read_to_string(&self.jet_file) else {
            self.emit("fix rejected: the debug source could not be read");
            return;
        };
        if source_on_disk == self.jet_src {
            self.emit("fix unavailable: source is unchanged; edit the file and retry");
            return;
        }
        let (_, candidate_source, candidate_shape) =
            match super::checked_bundle_for_fix(&self.jet_file) {
                Ok(value) => value,
                Err(reason) => {
                    self.emit(&format!("fix rejected: {reason}"));
                    return;
                }
            };
        let expected_shape = self
            .history
            .checkpoint
            .as_ref()
            .map(|checkpoint| checkpoint.shape.as_str());
        if expected_shape.is_some_and(|shape| {
            !matches!(shape, "native-debug-artifact" | "native-dap-debug-artifact")
                && shape != candidate_shape
        }) {
            self.emit(
                "fix rejected: checkpoint incompatible (source type, layout, or authority changed); restart required",
            );
            return;
        }
        let candidate = match super::Native::build_native_candidate(
            &self.jet_file,
            &self.rust_file,
            &candidate_source,
        ) {
            Ok(candidate) => candidate,
            Err(reason) => {
                self.emit(&format!("fix rejected: native candidate was not built: {reason}"));
                return;
            }
        };
        let receipt = match self.inf.rebind_native_candidate(
            &candidate.binary,
            &self.rust_src,
            &candidate.rust_source,
        ) {
            Ok(receipt) => receipt,
            Err(error) => {
                self.emit(&format!(
                    "fix rejected: native candidate was not activated: {error}"
                ));
                return;
            }
        };
        let index = self
            .history
            .cursor
            .or(self.history_cursor)
            .unwrap_or_else(|| self.history.events.len().saturating_sub(1));
        let identity = jet_foundation::SHA256::sha256_hex(
            format!("{}:{}", self.jet_file, candidate_source).as_bytes(),
        );
        if let Some(checkpoint) = self.history.checkpoint.as_mut() {
            checkpoint.source_identity = identity;
            checkpoint.shape = candidate_shape;
        }
        self.history.cursor = Some(index);
        self.history_cursor = Some(index);
        self.rust_src = candidate.rust_source.clone();
        self.map = LineMap::build(&self.rust_src);
        self.jet_src = candidate_source;
        self.replay_only_after_fix = false;
        let changed = receipt.changed.join(", ");
        let build_id = candidate.build_id.clone();
        self.candidate_artifacts.push(candidate);
        self.emit(&format!(
            "checkpoint compatible; native candidate committed ({build_id}); changed: {changed}"
        ));
    }
    fn event_location(&self, event: &DebugEvent) -> String {
        let source = self.source_line(event.line).trim();
        format!(
            "act {}: {}() at {}:{} — {}",
            event.sequence, event.function, self.jet_file, event.line, source
        )
    }

    fn cmd_why_history(&mut self, query: &str) {
        let query = query.trim();
        if query.is_empty() {
            self.emit("why needs a query, e.g. `why total == 0`");
            return;
        }
        let (name, expected) = query
            .split_once("==")
            .map(|(name, value)| (name.trim(), Some(value.trim())))
            .unwrap_or((query, None));
        let mut previous: Option<String> = None;
        let mut answer = Vec::new();
        for event in &self.history.events {
            let Some(value) = event.locals.iter().find(|value| value.name == name) else {
                continue;
            };
            let changed = previous.as_deref() != Some(value.value.as_str());
            if changed && expected.map_or(true, |expected| expected == value.value) {
                answer.push(format!(
                    "{} = {} because {}",
                    value.name,
                    value.value,
                    self.event_location(event)
                ));
            }
            previous = Some(value.value.clone());
        }
        if answer.is_empty() {
            self.emit(&format!("no recorded act explains `{query}`"));
        } else {
            self.emit(&answer.join("\n"));
        }
    }

    fn cmd_when_history(&mut self, query: &str) {
        let name = query.trim();
        if name.is_empty() {
            self.emit("when needs a place name, e.g. `when total`");
            return;
        }
        let mut previous: Option<String> = None;
        let mut changes = Vec::new();
        for event in &self.history.events {
            let Some(value) = event.locals.iter().find(|value| value.name == name) else {
                continue;
            };
            if previous.as_deref() != Some(value.value.as_str()) {
                changes.push(format!(
                    "{} = {} at {}",
                    value.name,
                    value.value,
                    self.event_location(event)
                ));
                previous = Some(value.value.clone());
            }
        }
        match changes.last() {
            Some(last) => self.emit(&format!("{}\nlast change: {}", changes.join("\n"), last)),
            None => self.emit(&format!("no recorded changes for `{name}`")),
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
  back [N]       reverse N recorded source stops
  reverse-continue
                 reverse to a recorded breakpoint or watchpoint
  watch X        stop/history-match a source-session watchpoint
  unwatch X      clear a source-session watchpoint
  fix            validate the edited source against the native checkpoint
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
            if self.history_cursor.is_some() {
                let handled = match verb {
                    v if v == Syntax::DBG_STEP
                        || v == Syntax::DBG_NEXT
                        || v == Syntax::DBG_FINISH
                        || v == Syntax::DBG_CONTINUE =>
                    {
                        self.advance_history()
                    }
                    "back" | "reverse" | "reverse-step" | "rstep" => {
                        let steps = arg
                            .and_then(|value| value.parse::<usize>().ok())
                            .unwrap_or(1);
                        self.reverse_step(steps);
                        true
                    }
                    "reverse-continue" | "rcontinue" | "back-continue" => {
                        self.reverse_continue();
                        true
                    }
                    _ => false,
                };
                if handled {
                    continue;
                }
            }
            match verb {
                v if v == Syntax::DBG_STEP || v == "s" => self.step(true),
                v if v == Syntax::DBG_NEXT || v == "n" => self.step(false),
                v if v == Syntax::DBG_FINISH || v == "f" => self.finish(),
                v if v == Syntax::DBG_CONTINUE || v == "c" => self.cont(),
                "back" | "reverse" | "reverse-step" | "rstep" => {
                    let steps = arg
                        .and_then(|value| value.parse::<usize>().ok())
                        .unwrap_or(1);
                    self.reverse_step(steps);
                }
                "reverse-continue" | "rcontinue" | "back-continue" => self.reverse_continue(),
                "watch" => self.cmd_watch(arg),
                "unwatch" => self.cmd_unwatch(arg),
                "fix" => self.cmd_fix(),
                v if v == Syntax::DBG_BREAK || v == "b" => self.cmd_break(arg),
                v if v == Syntax::DBG_LIST || v == "l" => {
                    let cur = self
                        .history_cursor
                        .and_then(|index| self.history.events.get(index).map(|event| event.line))
                        .or_else(|| self.current_jet_line());
                    self.cmd_list(cur);
                }
                v if v == Syntax::DBG_PRINT || v == "p" => self.cmd_print(arg),
                v if v == Syntax::DBG_LOCALS => self.cmd_locals_history(),
                v if v == Syntax::DBG_BACKTRACE || v == "bt" => self.cmd_backtrace(),
                v if v == Syntax::DBG_WHY => {
                    self.cmd_why_history(cmd.strip_prefix(verb).unwrap_or("").trim());
                }
                v if v == Syntax::DBG_WHEN => {
                    self.cmd_when_history(cmd.strip_prefix(verb).unwrap_or("").trim());
                }
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
        self.map
            .jet_line_for_file(&top.rust_file, &self.rust_file, top.rust_line)
    }

    fn step(&mut self, into: bool) {
        let cmd = if into {
            "thread step-in"
        } else {
            "thread step-over"
        };
        match self.inf.resume_and_locate(cmd) {
            Ok(r) => self.handle_resume(r),
            Err(_) => self.backend_lost("step failed; debugger session is unavailable"),
        }
    }

    fn finish(&mut self) {
        match self.inf.resume_and_locate("thread step-out") {
            Ok(r) => self.handle_resume(r),
            Err(_) => self.backend_lost("finish failed; debugger session is unavailable"),
        }
    }

    fn cont(&mut self) {
        match self.inf.resume_and_locate("continue") {
            Ok(r) => self.handle_resume(r),
            Err(_) => self.backend_lost("continue failed; debugger session is unavailable"),
        }
    }

    fn backend_lost(&mut self, message: &str) {
        self.snapshot = None;
        self.exit_code = ExitCodes::ICE;
        self.exited = true;
        self.emit(message);
    }

    fn run_to_completion(&mut self) {
        while !self.exited {
            match self.inf.resume_and_locate("continue") {
                Ok(result) => self.handle_resume(result),
                Err(_) => {
                    self.backend_lost("error: native debugger lost the running session");
                }
            }
        }
    }
}

