//! D-DBG3 step 2 (dap-debugger): the Debug Adapter Protocol server — the
//! "editor wiring" half of the native backend. Same [`super::Inferior`]/
//! [`super::LineMap`] the terminal `(jet)` session (`Native.rs`) uses; this
//! module only adds the DAP wire format (Content-Length framed JSON on stdio,
//! the same convention `Source/LSP/Server.rs` already speaks) so a trusted
//! editor such as VS Code can launch `jet debug --dap <file>` as a debug
//! adapter. The Zed extension uses the same stdio adapter and derives the
//! source from the launch file or the verified attach map.
//!
//! I6: reuses the foundation hand-rolled JSON codec (no serde, no
//! DAP crate). I2: every `stackTrace`/`variables` response is translated to
//! Jet terms through `LineMap` before it reaches the editor by default. A
//! launch or stack/scope request may opt into `showRawFrames`; those entries
//! are clearly marked and do not change Jet stepping or evaluation.
//!
//! The adapter owns the DAP lifecycle and translates every backend result into
//! a bounded Jet-facing response. Backend text is never a user-facing
//! diagnostic or source location.

use std::collections::{HashMap, HashSet};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use super::Inferior::{parse_current_thread_id, parse_exit_signal, Inferior, ResumeResult};
use super::LineMap::LineMap;
#[cfg(test)]
use jet_foundation::JSON::parse_json;
use jet_foundation::JSON::{
    json_escape, json_get, json_str, json_u32, parse_json_with_limit, JSONValue,
    MAX_PROTOCOL_HEADER_BYTES, MAX_PROTOCOL_HEADER_COUNT,
};

/// D-DBG-DAP1=A: DAP accepts larger framed messages than LSP, but never an
/// unbounded body. The JSON tree is still depth-bounded by the foundation
/// parser.
const MAX_DAP_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    New,
    Ready,
    Configuring,
    Running,
    Stopped,
    Terminated,
}

impl State {
    fn is_stopped(self) -> bool {
        self == Self::Stopped
    }
}

pub fn run(binary: &Path, rust_file: &str, rust_src: &str, jet_file: &str, jet_src: &str) -> i32 {
    let map_path = binary.with_extension("jetmap");
    if let Err(error) =
        LineMap::write_artifact(&map_path, jet_file, jet_src, rust_file, rust_src, binary)
    {
        eprintln!("error: cannot write native debugger map: {error}");
        return crate::ExitCodes::USER_ERROR;
    }
    let map =
        match LineMap::load_verified(&map_path, jet_file, jet_src, rust_file, rust_src, binary) {
            Ok(map) => map,
            Err(error) => {
                eprintln!("error: native debugger map is not usable: {error}");
                return crate::ExitCodes::USER_ERROR;
            }
        };
    let mut server = DapServer {
        map,
        rust_file: rust_file.to_string(),
        rust_src: rust_src.to_string(),
        jet_file: jet_file.to_string(),
        jet_src: jet_src.to_string(),
        binary: binary.to_path_buf(),
        inf: None,
        pending_breakpoints: Vec::new(),
        pending_specs: Vec::new(),
        hit_counts: Vec::new(),
        source_breakpoint_ids: Vec::new(),
        client_breakpoint_ids: Vec::new(),
        next_client_breakpoint_id: 1,
        entry_breakpoint_id: None,
        stop_on_entry: true,
        target: None,
        launch_args: Vec::new(),
        launch_cwd: None,
        launch_env: Vec::new(),
        current_thread: 1,
        last_signal: None,
        exception_filters: default_exception_filters(),
        show_raw_frames: false,
        lines_start_at_1: true,
        columns_start_at_1: true,
        supports_ansi_styling: false,
        unmapped_steps: 0,
        references: ObjectReferences::default(),
        state: State::New,
        client_sequences: HashSet::new(),
        seq: 1,
    };
    let stdin = std::io::stdin();
    let mut reader = std::io::BufReader::new(stdin.lock());
    let mut stdout = std::io::stdout();
    let result = run_io(&mut server, &mut reader, &mut stdout);
    result
}

fn run_io(server: &mut DapServer, reader: &mut impl BufRead, out: &mut impl Write) -> i32 {
    loop {
        let body = match read_message(reader) {
            Ok(Some(body)) => body,
            Ok(None) => break,
            Err(error) => {
                // A framing or UTF-8 failure cannot be correlated to a
                // request. Fail closed without emitting a fabricated reply.
                eprintln!("error: invalid DAP frame: {error}");
                return server.finish(crate::ExitCodes::USER_ERROR);
            }
        };
        let msg = match parse_dap_request(&body) {
            Ok(msg) => msg,
            Err(error) => {
                eprintln!("error: invalid DAP request: {error}");
                return server.finish(crate::ExitCodes::USER_ERROR);
            }
        };
        if server.handle(&msg, out).is_none() {
            break;
        }
    }
    server.finish(crate::ExitCodes::OK)
}

struct DapServer {
    map: LineMap,
    rust_file: String,
    rust_src: String,
    jet_file: String,
    jet_src: String,
    binary: std::path::PathBuf,
    inf: Option<Inferior>,
    /// Jet lines requested via `setBreakpoints`, applied once the inferior
    /// exists (a `launch` may arrive before or after `setBreakpoints`).
    pending_breakpoints: Vec<usize>,
    pending_specs: Vec<BreakpointSpec>,
    hit_counts: Vec<u32>,
    source_breakpoint_ids: Vec<usize>,
    client_breakpoint_ids: Vec<u32>,
    next_client_breakpoint_id: u32,
    entry_breakpoint_id: Option<usize>,
    stop_on_entry: bool,
    target: Option<TargetKind>,
    launch_args: Vec<String>,
    launch_cwd: Option<String>,
    launch_env: Vec<(String, String)>,
    current_thread: u32,
    last_signal: Option<String>,
    exception_filters: HashSet<String>,
    show_raw_frames: bool,
    lines_start_at_1: bool,
    columns_start_at_1: bool,
    supports_ansi_styling: bool,
    unmapped_steps: usize,
    references: ObjectReferences,
    state: State,
    client_sequences: HashSet<u32>,
    seq: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TargetKind {
    Launched,
    Attached,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct BreakpointSpec {
    line: usize,
    condition: Option<String>,
    hit_condition: Option<String>,
    log_message: Option<String>,
}

enum BreakpointAction {
    Stop(Vec<u32>),
    Continue,
    Log(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResumeMode {
    /// The first resume after `launch`.  A resolved entry breakpoint stops
    /// once; otherwise ordinary source breakpoints control the run.
    Launch,
    Continue,
    Step,
    Pause,
}

struct AttachArguments {
    pid: u32,
    program: PathBuf,
    map: PathBuf,
    show_raw_frames: bool,
}

#[derive(Clone, Copy)]
enum AttachFailure {
    InvalidArguments,
    MapMismatch,
    Denied,
    Unavailable,
}

impl AttachFailure {
    fn error(self) -> (i64, &'static str) {
        match self {
            Self::InvalidArguments => (22032, "debug attach arguments are invalid"),
            Self::MapMismatch => (22037, "debugger map does not match the attach target"),
            Self::Denied => (22037, "debug attach target is not authorized"),
            Self::Unavailable => (22034, "the local attach target is unavailable"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReferenceKind {
    Frame,
    Scope,
    RawScope,
    Value,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FrameLocation {
    thread_id: u32,
    position: usize,
}

struct ValueReference {
    frame: FrameLocation,
    expression: String,
    raw: bool,
}

/// DAP object ids are handles into one stopped snapshot.  The generation is
/// deliberately kept server-side: an editor can retain an old integer, but
/// it can never make that integer live again after execution resumes.
struct ObjectReferences {
    generation: u64,
    next_id: u32,
    live: HashMap<u32, (u64, ReferenceKind)>,
    frame_ids: HashMap<(u32, usize), u32>,
    frame_positions: HashMap<u32, FrameLocation>,
    frame_raw: HashMap<u32, bool>,
    scope_positions: HashMap<u32, FrameLocation>,
    values: HashMap<u32, ValueReference>,
}

impl Default for ObjectReferences {
    fn default() -> Self {
        Self {
            generation: 1,
            next_id: 1,
            live: HashMap::new(),
            frame_ids: HashMap::new(),
            frame_positions: HashMap::new(),
            frame_raw: HashMap::new(),
            scope_positions: HashMap::new(),
            values: HashMap::new(),
        }
    }
}

impl ObjectReferences {
    fn issue(&mut self, kind: ReferenceKind) -> u32 {
        // Zero is reserved for DAP's "no children" value.  On the
        // practically unreachable u32 wrap, expire the old snapshot before
        // reusing an integer.
        if self.next_id == 0 {
            self.invalidate();
            self.next_id = 1;
        }
        let id = self.next_id;
        self.next_id = self.next_id.checked_add(1).unwrap_or(0);
        self.live.insert(id, (self.generation, kind));
        id
    }

    fn is_live(&self, id: u32, kind: ReferenceKind) -> bool {
        self.live.get(&id) == Some(&(self.generation, kind))
    }

    fn issue_frame(&mut self, thread_id: u32, position: usize) -> u32 {
        if let Some(id) = self.frame_ids.get(&(thread_id, position)) {
            return *id;
        }
        let id = self.issue(ReferenceKind::Frame);
        self.frame_ids.insert((thread_id, position), id);
        self.frame_positions.insert(
            id,
            FrameLocation {
                thread_id,
                position,
            },
        );
        self.frame_raw.insert(id, false);
        id
    }

    fn issue_raw_frame(&mut self, thread_id: u32, position: usize) -> u32 {
        let id = self.issue(ReferenceKind::Frame);
        self.frame_positions.insert(
            id,
            FrameLocation {
                thread_id,
                position,
            },
        );
        self.frame_raw.insert(id, true);
        id
    }

    fn frame_is_raw(&self, id: u32) -> Option<bool> {
        self.frame_raw.get(&id).copied()
    }

    fn issue_scope(&mut self, frame_id: u32, kind: ReferenceKind) -> u32 {
        let id = self.issue(kind);
        if let Some(location) = self.frame_positions.get(&frame_id) {
            self.scope_positions.insert(id, *location);
        }
        id
    }

    fn frame_location(&self, id: u32) -> Option<FrameLocation> {
        self.frame_positions.get(&id).copied()
    }

    fn scope_location(&self, id: u32) -> Option<FrameLocation> {
        self.scope_positions.get(&id).copied()
    }

    fn issue_value_at(&mut self, frame: FrameLocation, expression: String, raw: bool) -> u32 {
        let id = self.issue(ReferenceKind::Value);
        self.values.insert(
            id,
            ValueReference {
                frame,
                expression,
                raw,
            },
        );
        id
    }

    fn value(&self, id: u32) -> Option<&ValueReference> {
        self.values.get(&id)
    }

    fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1).max(1);
        self.live.clear();
        self.frame_ids.clear();
        self.frame_positions.clear();
        self.frame_raw.clear();
        self.scope_positions.clear();
        self.values.clear();
    }
}

impl DapServer {
    fn finish(&mut self, code: i32) -> i32 {
        self.state = State::Terminated;
        self.invalidate_references();
        let terminate = self.target == Some(TargetKind::Launched);
        match self.close_target(terminate) {
            Ok(()) => code,
            Err(error) => {
                let (id, message) = cleanup_error_details(&error);
                let diagnostic = match id {
                    22037 => "E2231",
                    22038 => "E2237",
                    _ => "E2234",
                };
                eprintln!("error [{diagnostic}]: {message}");
                crate::ExitCodes::USER_ERROR
            }
        }
    }

    fn close_target(&mut self, terminate: bool) -> io::Result<()> {
        let Some(mut inf) = self.inf.take() else {
            self.target = None;
            return Ok(());
        };
        let _entry_breakpoint = self.entry_breakpoint_id.take();
        let action = if terminate {
            inf.terminate_debuggee()
        } else {
            // An explicit non-terminating disconnect leaves either target
            // running.  LLDB must detach before its control process exits.
            inf.detach()
        };
        inf.quit();
        self.target = None;
        self.source_breakpoint_ids.clear();
        action
    }

    fn cleanup_after_terminal_state(&mut self, out: &mut impl Write) {
        let terminate = self.target == Some(TargetKind::Launched);
        if let Err(error) = self.close_target(terminate) {
            let (id, message) = cleanup_error_details(&error);
            let diagnostic = match id {
                22037 => "E2231",
                22038 => "E2237",
                _ => "E2234",
            };
            let body = format!(
                "{{\"category\":\"stderr\",\"output\":\"[{}] {}\\n\"}}",
                diagnostic, message
            );
            self.event(out, "output", &body);
        }
    }

    fn source_line(&self, line: usize) -> &str {
        self.jet_src
            .lines()
            .nth(line.saturating_sub(1))
            .unwrap_or("")
    }

    fn dap_line(&self, jet_line: usize) -> usize {
        if self.lines_start_at_1 {
            jet_line
        } else {
            jet_line.saturating_sub(1)
        }
    }

    fn dap_column(&self, jet_column: usize) -> usize {
        if self.columns_start_at_1 {
            jet_column
        } else {
            jet_column.saturating_sub(1)
        }
    }

    fn source_path(&self) -> String {
        std::fs::canonicalize(&self.jet_file)
            .unwrap_or_else(|_| PathBuf::from(&self.jet_file))
            .to_string_lossy()
            .into_owned()
    }

    fn assign_client_breakpoint_ids(&mut self, specs: &[BreakpointSpec]) -> Vec<u32> {
        let old_specs = self.pending_specs.clone();
        let old_ids = self.client_breakpoint_ids.clone();
        let mut assigned_specs = Vec::new();
        let mut assigned_ids = Vec::new();
        for spec in specs {
            if let Some(position) = assigned_specs.iter().position(|old| old == spec) {
                assigned_ids.push(assigned_ids[position]);
                continue;
            }
            let id = old_specs
                .iter()
                .zip(old_ids.iter().copied())
                .find_map(|(old, id)| (old == spec).then_some(id))
                .unwrap_or_else(|| {
                    let id = self.next_client_breakpoint_id.max(1);
                    self.next_client_breakpoint_id = id.checked_add(1).unwrap_or(1);
                    id
                });
            assigned_specs.push(spec.clone());
            assigned_ids.push(id);
        }
        assigned_ids
    }

    fn emit_target_started_events(&mut self, out: &mut impl Write) {
        let start_method = if self.target == Some(TargetKind::Attached) {
            "attach"
        } else {
            "launch"
        };
        self.event(
            out,
            "process",
            &format!(
                "{{\"name\":\"Jet\",\"startMethod\":\"{}\",\"isLocalProcess\":true}}",
                start_method
            ),
        );
        self.event(out, "thread", "{\"reason\":\"started\",\"threadId\":1}");
    }

    fn raw_frames_for_request(&self, args: Option<&JSONValue>) -> Result<bool, &'static str> {
        match args.and_then(|args| json_get(args, "showRawFrames")) {
            None => Ok(self.show_raw_frames),
            Some(value) => match json_bool(value) {
                Some(value) if value == self.show_raw_frames => Ok(value),
                Some(_) => Err("showRawFrames cannot change during a debug session"),
                None => Err("showRawFrames must be a boolean"),
            },
        }
    }

    fn output_text(&self, text: &str) -> String {
        if self.supports_ansi_styling {
            return text.to_string();
        }
        let mut clean = String::with_capacity(text.len());
        let mut escape = false;
        for ch in text.chars() {
            if escape {
                if ch.is_ascii_alphabetic() {
                    escape = false;
                }
                continue;
            }
            if ch == '\u{1b}' {
                escape = true;
            } else {
                clean.push(ch);
            }
        }
        clean
    }

    fn spawn_target(&self) -> io::Result<(Inferior, Option<usize>)> {
        let mut inf = Inferior::spawn(&self.binary)?;
        let setup = if self.stop_on_entry {
            self.map
                .main_entry_line(&self.rust_src)
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "entry has no source line")
                })
                .and_then(|entry_line| {
                    inf.set_breakpoint(&self.rust_file, entry_line)
                        .map(|breakpoint| breakpoint.resolved.then_some(breakpoint.id))
                })
        } else {
            Ok(None)
        };
        match setup {
            Ok(entry) => Ok((inf, entry)),
            Err(error) => {
                inf.quit();
                Err(error)
            }
        }
    }

    fn attach_target(
        &self,
        args: Option<&JSONValue>,
    ) -> Result<(Inferior, LineMap, bool), AttachFailure> {
        let attach = parse_attach_arguments(args).map_err(|_| AttachFailure::InvalidArguments)?;
        let map = LineMap::load_verified(
            &attach.map,
            &self.jet_file,
            &self.jet_src,
            &self.rust_file,
            &self.rust_src,
            &attach.program,
        )
        .map_err(|_| AttachFailure::MapMismatch)?;
        let inferior = Inferior::attach(&attach.program, attach.pid).map_err(|error| {
            if matches!(
                error.kind(),
                io::ErrorKind::PermissionDenied | io::ErrorKind::InvalidInput
            ) {
                AttachFailure::Denied
            } else {
                AttachFailure::Unavailable
            }
        })?;
        Ok((inferior, map, attach.show_raw_frames))
    }

    fn install_breakpoints(
        map: &LineMap,
        rust_file: &str,
        lines: &[usize],
        inf: &mut Inferior,
    ) -> io::Result<(Vec<usize>, Vec<bool>)> {
        let mut ids = Vec::new();
        let mut verified = Vec::new();
        for jline in lines {
            if let Some(rust_line) = map.rust_line_for(*jline) {
                let breakpoint = inf.set_breakpoint(rust_file, rust_line)?;
                ids.push(breakpoint.id);
                verified.push(breakpoint.resolved);
            } else {
                verified.push(false);
            }
        }
        Ok((ids, verified))
    }

    fn replace_source_breakpoints(&mut self) -> io::Result<Vec<bool>> {
        let old = std::mem::take(&mut self.source_breakpoint_ids);
        let lines = self.pending_breakpoints.clone();
        let map = &self.map;
        let rust_file = self.rust_file.clone();
        let inf = self
            .inf
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "target is not launched"))?;
        for id in old {
            inf.delete_breakpoint(id)?;
        }
        let (ids, verified) = Self::install_breakpoints(map, &rust_file, &lines, inf)?;
        self.source_breakpoint_ids = ids;
        Ok(verified)
    }

    fn invalidate_references(&mut self) {
        self.references.invalidate();
    }

    fn restart_target(&mut self, out: &mut impl Write, request_seq: i64) {
        self.invalidate_references();
        if let Some(inf) = self.inf.take() {
            inf.quit();
        }
        self.current_thread = 1;
        self.last_signal = None;
        self.unmapped_steps = 0;

        let map_path = self.binary.with_extension("jetmap");
        self.map = match LineMap::load_verified(
            &map_path,
            &self.jet_file,
            &self.jet_src,
            &self.rust_file,
            &self.rust_src,
            &self.binary,
        ) {
            Ok(map) => map,
            Err(_error) => {
                self.target = None;
                self.entry_breakpoint_id = None;
                self.source_breakpoint_ids.clear();
                self.state = State::Ready;
                self.respond_jet_error(
                    out,
                    request_seq,
                    "restart",
                    22037,
                    "restart target no longer matches the verified Jet build",
                );
                return;
            }
        };

        let (mut inf, entry_id) = match self.spawn_target() {
            Ok(target) => target,
            Err(_error) => {
                self.target = None;
                self.entry_breakpoint_id = None;
                self.source_breakpoint_ids.clear();
                self.state = State::Ready;
                self.respond_jet_error(
                    out,
                    request_seq,
                    "restart",
                    22034,
                    "restart could not create a fresh native target",
                );
                return;
            }
        };
        if let Err(_error) = inf.configure_launch(
            &self.launch_args,
            self.launch_cwd.as_deref(),
            &self.launch_env,
        ) {
            inf.quit();
            self.target = None;
            self.entry_breakpoint_id = None;
            self.source_breakpoint_ids.clear();
            self.state = State::Ready;
            self.respond_jet_error(
                out,
                request_seq,
                "restart",
                22034,
                "restart could not restore launch arguments",
            );
            return;
        }
        let (source_ids, _) = match Self::install_breakpoints(
            &self.map,
            &self.rust_file,
            &self.pending_breakpoints,
            &mut inf,
        ) {
            Ok(result) => result,
            Err(_error) => {
                inf.quit();
                self.target = None;
                self.entry_breakpoint_id = None;
                self.source_breakpoint_ids.clear();
                self.state = State::Ready;
                self.respond_jet_error(
                    out,
                    request_seq,
                    "restart",
                    22034,
                    "restart could not restore source breakpoints",
                );
                return;
            }
        };
        self.inf = Some(inf);
        self.entry_breakpoint_id = entry_id;
        self.source_breakpoint_ids = source_ids;
        self.hit_counts = vec![0; self.pending_specs.len()];
        self.target = Some(TargetKind::Launched);
        self.respond(out, request_seq, "restart", true, "{}");
        self.emit_target_started_events(out);
        self.start_target(out);
    }

    fn start_target(&mut self, out: &mut impl Write) {
        self.state = State::Running;
        let result = self.inf.as_mut().map(|inf| inf.resume_and_locate("run"));
        match result {
            Some(Ok(result)) => self.emit_resume(out, result, ResumeMode::Launch),
            Some(Err(error)) => self.terminate_backend(
                out,
                &error,
                "the native debugger could not start the program",
            ),
            None => self.terminate_with_diagnostic(
                out,
                "E2234",
                "the native debugger has no target to start",
            ),
        }
    }

    /// Dispatch one DAP request. `Some(())` to keep the loop going; `None` to
    /// stop (a `disconnect`/`terminate` request, or an unrecoverable error).
    fn handle(&mut self, msg: &JSONValue, out: &mut impl Write) -> Option<()> {
        let command = json_get(msg, "command").and_then(json_str)?;
        let client_seq = json_get(msg, "seq").and_then(json_u32)?;
        let request_seq = i64::from(client_seq);
        if !self.client_sequences.insert(client_seq) {
            self.respond_jet_error(
                out,
                request_seq,
                command,
                22032,
                "request sequence numbers must be unique",
            );
            return Some(());
        }
        let args = json_get(msg, "arguments");
        match command {
            "initialize" => {
                if self.state != State::New {
                    self.respond_jet_error(
                        out,
                        request_seq,
                        command,
                        22031,
                        "initialize is only legal once at the start of a DAP session",
                    );
                    return Some(());
                }
                let initialize = match parse_initialize_arguments(args) {
                    Ok(arguments) => arguments,
                    Err(message) => {
                        self.respond_jet_error(out, request_seq, command, 22032, message);
                        return Some(());
                    }
                };
                self.lines_start_at_1 = initialize.lines_start_at_1;
                self.columns_start_at_1 = initialize.columns_start_at_1;
                self.supports_ansi_styling = initialize.supports_ansi_styling;
                self.state = State::Ready;
                self.respond(
                    out,
                    request_seq,
                    "initialize",
                    true,
                    "{\"supportsConfigurationDoneRequest\":true,\"supportsTerminateRequest\":true,\"supportsRestartRequest\":true,\"supportsConditionalBreakpoints\":true,\"supportsHitConditionalBreakpoints\":true,\"supportsLogPoints\":true,\"supportsEvaluateForHovers\":true,\"supportsExceptionInfoRequest\":true,\"supportsLoadedSourcesRequest\":true,\"supportsProgressReporting\":true,\"supportsVariablePaging\":true}",
                );
                Some(())
            }
            "launch" | "attach" => {
                if self.state != State::Ready {
                    self.respond_jet_error(
                        out,
                        request_seq,
                        command,
                        22031,
                        "launch or attach requires a successful initialize request",
                    );
                    return Some(());
                }
                let progress_id = format!("jet-debug-{request_seq}");
                self.progress_start(out, &progress_id, "Starting Jet debug target");
                let target = if command == "launch" {
                    let launch = match parse_launch_arguments(args, &self.jet_file) {
                        Ok(launch) => launch,
                        Err(message) => {
                            self.progress_end(out, &progress_id, "Invalid launch configuration");
                            self.respond_jet_error(out, request_seq, command, 22032, message);
                            return Some(());
                        }
                    };
                    if let Some(value) = args.and_then(|args| json_get(args, "stopOnEntry")) {
                        let Some(value) = json_bool(value) else {
                            self.progress_end(out, &progress_id, "Invalid launch configuration");
                            self.respond_jet_error(
                                out,
                                request_seq,
                                command,
                                22032,
                                "stopOnEntry must be a boolean",
                            );
                            return Some(());
                        };
                        self.stop_on_entry = value;
                    }
                    let target = self.spawn_target().and_then(|(mut inf, id)| {
                        inf.configure_launch(&launch.args, launch.cwd.as_deref(), &launch.env)?;
                        Ok((inf, id))
                    });
                    if target.is_ok() {
                        self.show_raw_frames = launch.show_raw_frames;
                        self.launch_args = launch.args;
                        self.launch_cwd = launch.cwd;
                        self.launch_env = launch.env;
                    }
                    target
                } else {
                    match self.attach_target(args) {
                        Ok((inf, map, show_raw_frames)) => {
                            self.map = map;
                            self.show_raw_frames = show_raw_frames;
                            Ok((inf, None))
                        }
                        Err(failure) => {
                            let (id, format) = failure.error();
                            self.progress_end(out, &progress_id, "Attach rejected");
                            self.respond_jet_error(out, request_seq, command, id, format);
                            return Some(());
                        }
                    }
                };
                match target {
                    Ok((inf, entry_id)) => {
                        self.progress_end(out, &progress_id, "Jet debug target ready");
                        self.inf = Some(inf);
                        self.entry_breakpoint_id = entry_id;
                        self.source_breakpoint_ids.clear();
                        self.target = Some(if command == "launch" {
                            TargetKind::Launched
                        } else {
                            TargetKind::Attached
                        });
                        self.state = State::Configuring;
                        self.respond(out, request_seq, command, true, "{}");
                        self.event(out, "initialized", "{}");
                    }
                    Err(_error) => {
                        self.progress_end(out, &progress_id, "Target setup failed");
                        self.respond_jet_error(
                            out,
                            request_seq,
                            command,
                            22034,
                            "the native debugger could not create the requested target",
                        );
                    }
                }
                Some(())
            }
            "restart" => {
                if !matches!(
                    self.state,
                    State::Configuring | State::Running | State::Stopped
                ) {
                    self.respond_jet_error(
                        out,
                        request_seq,
                        command,
                        22031,
                        "restart requires an active launched Jet target",
                    );
                    return Some(());
                }
                match self.target {
                    Some(TargetKind::Launched) if self.inf.is_some() => {
                        self.restart_target(out, request_seq);
                    }
                    Some(TargetKind::Attached) => self.respond_jet_error(
                        out,
                        request_seq,
                        command,
                        22033,
                        "restart is only supported for a launched Jet target",
                    ),
                    _ => self.respond_jet_error(
                        out,
                        request_seq,
                        command,
                        22031,
                        "restart is not legal before a launched Jet target exists",
                    ),
                }
                Some(())
            }
            "setBreakpoints" => {
                if !matches!(
                    self.state,
                    State::Ready | State::Configuring | State::Running | State::Stopped
                ) {
                    self.respond_jet_error(
                        out,
                        request_seq,
                        command,
                        22031,
                        "source breakpoints are not legal after the target terminated",
                    );
                    return Some(());
                }
                if let Err(message) = breakpoint_source(args, &self.jet_file) {
                    self.respond_jet_error(out, request_seq, command, 22032, message);
                    return Some(());
                }
                let specs = match breakpoint_specs_for_origin(args, self.lines_start_at_1) {
                    Ok(specs) => specs,
                    Err(message) => {
                        self.respond_err(out, request_seq, command, message);
                        return Some(());
                    }
                };
                let lines: Vec<usize> = specs.iter().map(|spec| spec.line).collect();
                let client_ids = self.assign_client_breakpoint_ids(&specs);
                self.pending_breakpoints = lines.clone();
                self.pending_specs = specs;
                self.client_breakpoint_ids = client_ids.clone();
                self.hit_counts = vec![0; lines.len()];
                let statuses = if matches!(self.state, State::Running | State::Stopped) {
                    match self.replace_source_breakpoints() {
                        Ok(statuses) => statuses,
                        Err(_error) => {
                            self.respond_err(
                                out,
                                request_seq,
                                command,
                                "the native debugger could not replace source breakpoints",
                            );
                            return Some(());
                        }
                    }
                } else {
                    lines
                        .iter()
                        .map(|line| self.map.rust_line_for(*line).is_some())
                        .collect()
                };
                let verified: Vec<String> = lines
                    .iter()
                    .zip(client_ids)
                    .zip(statuses)
                    .zip(self.pending_specs.iter())
                    .map(|(((line, id), verified), spec)| {
                        let verified = verified
                            && spec
                                .condition
                                .as_deref()
                                .is_none_or(valid_condition)
                            && spec
                                .hit_condition
                                .as_deref()
                                .is_none_or(valid_hit_condition);
                        if verified {
                            format!(
                                "{{\"id\":{id},\"verified\":true,\"line\":{}}}",
                                self.dap_line(*line)
                            )
                        } else {
                            format!(
                                "{{\"id\":{id},\"verified\":false,\"line\":{},\"message\":\"E2235: no stoppable Jet statement at {}:{}\"}}",
                                self.dap_line(*line),
                                json_escape(&self.source_path()),
                                self.dap_line(*line)
                            )
                        }
                    })
                    .collect();
                let body = format!("{{\"breakpoints\":[{}]}}", verified.join(","));
                self.respond(out, request_seq, command, true, &body);
                Some(())
            }
            "configurationDone" => {
                if self.state != State::Configuring {
                    self.respond_jet_error(
                        out,
                        request_seq,
                        command,
                        22031,
                        "configurationDone requires a launched or attached target",
                    );
                    return Some(());
                }
                self.respond(out, request_seq, command, true, "{}");
                self.emit_target_started_events(out);
                self.apply_pending_breakpoints_and_run(out);
                Some(())
            }
            "threads" => {
                if !self.state.is_stopped() {
                    self.respond_jet_error(
                        out,
                        request_seq,
                        command,
                        22031,
                        "threads requires a stopped target",
                    );
                    return Some(());
                }
                let threads = match self.inf.as_mut().map(Inferior::threads) {
                    Some(Ok(threads)) if !threads.is_empty() => threads,
                    Some(Ok(_)) => vec![super::Inferior::ThreadInfo {
                        id: 1,
                        name: "main".to_string(),
                    }],
                    Some(Err(error)) => {
                        self.respond_backend_error(
                            out,
                            request_seq,
                            command,
                            &error,
                            "the native debugger could not list Jet tasks",
                        );
                        return Some(());
                    }
                    None => {
                        self.respond_jet_error(
                            out,
                            request_seq,
                            command,
                            22031,
                            "threads requires a stopped target",
                        );
                        return Some(());
                    }
                };
                let thread_json = threads
                    .iter()
                    .map(|thread| {
                        let name = if thread.id == 1 {
                            "main".to_string()
                        } else {
                            format!("task {} (stopped)", thread.id)
                        };
                        format!(
                            "{{\"id\":{},\"name\":\"{}\"}}",
                            thread.id,
                            json_escape(&name)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                self.respond(
                    out,
                    request_seq,
                    command,
                    true,
                    &format!("{{\"threads\":[{}]}}", thread_json),
                );
                Some(())
            }
            "stackTrace" => {
                if !self.state.is_stopped() {
                    self.respond_jet_error(
                        out,
                        request_seq,
                        command,
                        22031,
                        "stackTrace requires a stopped target",
                    );
                    return Some(());
                }
                let Some(thread_id) = args
                    .and_then(|args| json_get(args, "threadId"))
                    .and_then(json_u32)
                    .filter(|thread_id| *thread_id > 0)
                else {
                    self.respond_jet_error(
                        out,
                        request_seq,
                        command,
                        22032,
                        "stackTrace threadId must be a positive integer",
                    );
                    return Some(());
                };
                if let Some(inf) = self.inf.as_mut() {
                    if let Err(_error) = inf.select_thread(thread_id) {
                        self.respond_jet_error(
                            out,
                            request_seq,
                            command,
                            22034,
                            "the requested Jet task is no longer stopped",
                        );
                        return Some(());
                    }
                    self.current_thread = thread_id;
                }
                let show_raw_frames = match self.raw_frames_for_request(args) {
                    Ok(value) => value,
                    Err(message) => {
                        self.respond_jet_error(out, request_seq, command, 22032, message);
                        return Some(());
                    }
                };
                let frames = match self.inf.as_mut().map(Inferior::backtrace) {
                    Some(Ok(out_text)) => Inferior::parse_frames(&out_text),
                    Some(Err(error)) => {
                        self.respond_backend_error(
                            out,
                            request_seq,
                            command,
                            &error,
                            "the native debugger could not read the Jet stack",
                        );
                        return Some(());
                    }
                    None => {
                        self.respond_jet_error(
                            out,
                            request_seq,
                            command,
                            22031,
                            "stackTrace requires a stopped target",
                        );
                        return Some(());
                    }
                };
                let start_frame = args
                    .and_then(|args| json_get(args, "startFrame"))
                    .and_then(json_u32)
                    .unwrap_or(0) as usize;
                let levels = args
                    .and_then(|args| json_get(args, "levels"))
                    .and_then(json_u32)
                    .map(|levels| levels as usize);
                let mut entries = Vec::new();
                for (position, frame) in frames
                    .iter()
                    .enumerate()
                    .skip(start_frame)
                    .take(levels.unwrap_or(usize::MAX))
                {
                    if let Some(jline) = self.map.jet_line_for_file(
                        &frame.rust_file,
                        &self.rust_file,
                        frame.rust_line,
                    ) {
                        let id = self.references.issue_frame(thread_id, position);
                        entries.push(format!(
                            "{{\"id\":{},\"name\":\"{}\",\"source\":{{\"path\":\"{}\"}},\"line\":{},\"column\":{}}}",
                            id,
                            json_escape(&Inferior::safe_jet_func(&frame.func)),
                            json_escape(&self.source_path()),
                            self.dap_line(jline),
                            self.dap_column(1)
                        ));
                    }
                    if show_raw_frames {
                        let id = self.references.issue_raw_frame(thread_id, position);
                        entries.push(format!(
                            "{{\"id\":{},\"name\":\"[raw] {}\",\"source\":{{\"path\":\"{}\"}},\"line\":{},\"column\":{}}}",
                            id,
                            json_escape(&frame.func),
                            json_escape(&self.rust_file),
                            self.dap_line(frame.rust_line),
                            self.dap_column(1)
                        ));
                    }
                }
                let body = format!(
                    "{{\"stackFrames\":[{}],\"totalFrames\":{}}}",
                    entries.join(","),
                    frames
                        .iter()
                        .map(|frame| {
                            if show_raw_frames {
                                if self
                                    .map
                                    .jet_line_for_file(
                                        &frame.rust_file,
                                        &self.rust_file,
                                        frame.rust_line,
                                    )
                                    .is_some()
                                {
                                    2
                                } else {
                                    1
                                }
                            } else if self
                                .map
                                .jet_line_for_file(
                                    &frame.rust_file,
                                    &self.rust_file,
                                    frame.rust_line,
                                )
                                .is_some()
                            {
                                1
                            } else {
                                0
                            }
                        })
                        .sum::<usize>()
                );
                self.respond(out, request_seq, command, true, &body);
                Some(())
            }
            "scopes" => {
                if !self.state.is_stopped() {
                    self.respond_jet_error(
                        out,
                        request_seq,
                        command,
                        22031,
                        "scopes requires a stopped target",
                    );
                    return Some(());
                }
                let Some(frame_id) = args
                    .and_then(|args| json_get(args, "frameId"))
                    .and_then(json_u32)
                else {
                    self.respond_err(out, request_seq, command, "frameId is required");
                    return Some(());
                };
                if !self.references.is_live(frame_id, ReferenceKind::Frame) {
                    self.respond_stale_reference(out, request_seq, command);
                    return Some(());
                }
                let Some(_location) = self.references.frame_location(frame_id) else {
                    self.respond_stale_reference(out, request_seq, command);
                    return Some(());
                };
                let requested_raw = match args.and_then(|args| json_get(args, "showRawFrames")) {
                    None => false,
                    Some(value) => match json_bool(value) {
                        Some(value) => value,
                        None => {
                            self.respond_jet_error(
                                out,
                                request_seq,
                                command,
                                22032,
                                "showRawFrames must be a boolean",
                            );
                            return Some(());
                        }
                    },
                };
                let raw_frame = self.references.frame_is_raw(frame_id).unwrap_or(false);
                let show_raw_frames = requested_raw || self.show_raw_frames || raw_frame;
                let mut scopes = Vec::new();
                if raw_frame {
                    let scope_id = self
                        .references
                        .issue_scope(frame_id, ReferenceKind::RawScope);
                    scopes.push(format!(
                        "{{\"name\":\"[raw] Locals\",\"variablesReference\":{},\"expensive\":false}}",
                        scope_id
                    ));
                } else {
                    let scope_id = self.references.issue_scope(frame_id, ReferenceKind::Scope);
                    scopes.push(format!(
                        "{{\"name\":\"Locals\",\"variablesReference\":{},\"expensive\":false}}",
                        scope_id
                    ));
                    if show_raw_frames {
                        let raw_scope_id = self
                            .references
                            .issue_scope(frame_id, ReferenceKind::RawScope);
                        scopes.push(format!(
                            "{{\"name\":\"[raw] Locals\",\"variablesReference\":{},\"expensive\":false}}",
                            raw_scope_id
                        ));
                    }
                    for scope_name in ["Arguments", "Captures"] {
                        let scope_id = self.references.issue_scope(frame_id, ReferenceKind::Scope);
                        scopes.push(format!(
                            "{{\"name\":\"{}\",\"variablesReference\":{},\"expensive\":false}}",
                            scope_name, scope_id
                        ));
                    }
                }
                let body = format!("{{\"scopes\":[{}]}}", scopes.join(","));
                self.respond(out, request_seq, command, true, &body);
                Some(())
            }
            "variables" => {
                if !self.state.is_stopped() {
                    self.respond_jet_error(
                        out,
                        request_seq,
                        command,
                        22031,
                        "variables requires a stopped target",
                    );
                    return Some(());
                }
                let Some(scope_id) = args
                    .and_then(|args| json_get(args, "variablesReference"))
                    .and_then(json_u32)
                else {
                    self.respond_err(out, request_seq, command, "variablesReference is required");
                    return Some(());
                };
                let value_reference = if self.references.is_live(scope_id, ReferenceKind::Value) {
                    let Some(value) = self.references.value(scope_id) else {
                        self.respond_stale_reference(out, request_seq, command);
                        return Some(());
                    };
                    Some((value.frame, value.expression.clone(), value.raw))
                } else {
                    None
                };
                let raw_scope = value_reference
                    .as_ref()
                    .map(|(_, _, raw)| *raw)
                    .or_else(|| {
                        if self.references.is_live(scope_id, ReferenceKind::RawScope) {
                            Some(true)
                        } else if self.references.is_live(scope_id, ReferenceKind::Scope) {
                            Some(false)
                        } else {
                            None
                        }
                    });
                let Some(raw_scope) = raw_scope else {
                    self.respond_stale_reference(out, request_seq, command);
                    return Some(());
                };
                let Some(frame) = value_reference
                    .as_ref()
                    .map(|(frame, _, _)| *frame)
                    .or_else(|| self.references.scope_location(scope_id))
                else {
                    self.respond_stale_reference(out, request_seq, command);
                    return Some(());
                };
                self.current_thread = frame.thread_id;
                if let Some(inf) = self.inf.as_mut() {
                    if inf.select_thread(frame.thread_id).is_err()
                        || inf.select_frame(frame.position).is_err()
                    {
                        self.respond_stale_reference(out, request_seq, command);
                        return Some(());
                    }
                }
                let locals = match self.inf.as_mut().map(|inf| {
                    if let Some((_, expression, _)) = value_reference.as_ref() {
                        inf.frame_variable(expression)
                    } else {
                        inf.locals()
                    }
                }) {
                    Some(Ok(output)) => {
                        if value_reference.is_some() {
                            Inferior::parse_variable_children(&output)
                        } else {
                            Inferior::parse_typed_locals(&output)
                        }
                    }
                    Some(Err(error)) => {
                        self.respond_backend_error(
                            out,
                            request_seq,
                            command,
                            &error,
                            "the native debugger could not read Jet variables",
                        );
                        return Some(());
                    }
                    None => {
                        self.respond_jet_error(
                            out,
                            request_seq,
                            command,
                            22031,
                            "variables requires a stopped target",
                        );
                        return Some(());
                    }
                };
                // I2: only safe Jet bindings cross the adapter boundary. Rust
                // temporaries, addresses, layouts, and optimized storage stay
                // inside the backend.
                let start = match args.and_then(|args| json_get(args, "start")) {
                    None => 0usize,
                    Some(value) => {
                        match json_u32(value).and_then(|value| usize::try_from(value).ok()) {
                            Some(value) => value,
                            None => {
                                self.respond_jet_error(
                                    out,
                                    request_seq,
                                    command,
                                    22032,
                                    "variables start must be a nonnegative integer",
                                );
                                return Some(());
                            }
                        }
                    }
                };
                let count = match args.and_then(|args| json_get(args, "count")) {
                    None => None,
                    Some(value) => {
                        match json_u32(value).and_then(|value| usize::try_from(value).ok()) {
                            Some(value) => Some(value),
                            None => {
                                self.respond_jet_error(
                                    out,
                                    request_seq,
                                    command,
                                    22032,
                                    "variables count must be a nonnegative integer",
                                );
                                return Some(());
                            }
                        }
                    }
                };
                let entries: Vec<String> = locals
                    .iter()
                    .filter_map(|(ty, name, value)| {
                        if value_reference.is_none()
                            && !raw_scope
                            && !Inferior::rust_local_is_jet_visible(name)
                        {
                            return None;
                        }
                        let display_name = if raw_scope {
                            name.clone()
                        } else if value_reference.is_some() {
                            Inferior::rust_member_to_jet(name)?
                        } else {
                            Inferior::rust_local_to_jet(name)?
                        };
                        let safe_value = if raw_scope {
                            value.clone()
                        } else {
                            Inferior::safe_value(ty, value)
                        };
                        let type_json = if raw_scope {
                            format!(",\"type\":\"{}\"", json_escape(ty))
                        } else {
                            Inferior::jet_type_name(ty)
                                .map(|ty| format!(",\"type\":\"{}\"", json_escape(ty)))
                                .unwrap_or_default()
                        };
                        let variables_reference = if Inferior::has_nested_value(value) {
                            let expression = if let Some((_, parent, _)) = value_reference.as_ref()
                            {
                                if name.starts_with('[') {
                                    format!("{parent}{name}")
                                } else {
                                    format!("{parent}.{name}")
                                }
                            } else if raw_scope {
                                name.clone()
                            } else {
                                Inferior::jet_local_to_rust(&display_name)
                            };
                            self.references.issue_value_at(frame, expression, raw_scope)
                        } else {
                            0
                        };
                        Some(format!(
                            "{{\"name\":\"{}\",\"value\":\"{}\"{},\"variablesReference\":{}}}",
                            json_escape(&display_name),
                            json_escape(&safe_value),
                            type_json,
                            variables_reference
                        ))
                    })
                    .collect();
                let total = entries.len();
                let entries = entries
                    .into_iter()
                    .skip(start)
                    .take(count.unwrap_or(usize::MAX))
                    .collect::<Vec<_>>();
                let body = format!(
                    "{{\"variables\":[{}],\"namedVariables\":{}}}",
                    entries.join(","),
                    total
                );
                self.respond(out, request_seq, command, true, &body);
                Some(())
            }
            "evaluate" => {
                if !self.state.is_stopped() {
                    self.respond_jet_error(
                        out,
                        request_seq,
                        command,
                        22031,
                        "evaluate requires a stopped target",
                    );
                    return Some(());
                }
                if let Some(context) = args.and_then(|args| json_get(args, "context")) {
                    if !matches!(json_str(context), Some("hover" | "watch" | "repl")) {
                        self.respond_jet_error(
                            out,
                            request_seq,
                            command,
                            22032,
                            "evaluate context must be hover, watch, or repl",
                        );
                        return Some(());
                    }
                }
                let Some(expression) = args
                    .and_then(|args| json_get(args, "expression"))
                    .and_then(json_str)
                else {
                    self.respond_jet_error(
                        out,
                        request_seq,
                        command,
                        22032,
                        "evaluate requires a Jet expression",
                    );
                    return Some(());
                };
                let Some((root, suffix)) = jet_expression(expression) else {
                    self.respond_jet_error(
                        out,
                        request_seq,
                        command,
                        22033,
                        "only bounded read-only Jet local paths can be evaluated",
                    );
                    return Some(());
                };
                if let Some(frame_id) = args
                    .and_then(|args| json_get(args, "frameId"))
                    .and_then(json_u32)
                {
                    if !self.references.is_live(frame_id, ReferenceKind::Frame) {
                        self.respond_stale_reference(out, request_seq, command);
                        return Some(());
                    }
                    let Some(frame) = self.references.frame_location(frame_id) else {
                        self.respond_stale_reference(out, request_seq, command);
                        return Some(());
                    };
                    self.current_thread = frame.thread_id;
                    if let Some(inf) = self.inf.as_mut() {
                        if inf.select_thread(frame.thread_id).is_err()
                            || inf.select_frame(frame.position).is_err()
                        {
                            self.respond_stale_reference(out, request_seq, command);
                            return Some(());
                        }
                    }
                }
                let Some(rust_expression) = Inferior::jet_path_to_rust(&root, &suffix) else {
                    self.respond_jet_error(
                        out,
                        request_seq,
                        command,
                        22033,
                        "only bounded read-only Jet local paths can be evaluated",
                    );
                    return Some(());
                };
                let output = match self
                    .inf
                    .as_mut()
                    .map(|inf| inf.frame_variable(&rust_expression))
                {
                    Some(Ok(output)) => output,
                    Some(Err(error)) => {
                        self.respond_backend_error(
                            out,
                            request_seq,
                            command,
                            &error,
                            "the Jet expression is unavailable at this stop",
                        );
                        return Some(());
                    }
                    None => {
                        self.respond_jet_error(
                            out,
                            request_seq,
                            command,
                            22034,
                            "the Jet expression is unavailable at this stop",
                        );
                        return Some(());
                    }
                };
                let Some((ty, _name, value)) =
                    Inferior::parse_typed_locals(&output).into_iter().next()
                else {
                    self.respond_jet_error(
                        out,
                        request_seq,
                        command,
                        22034,
                        "the Jet expression has no readable value at this stop",
                    );
                    return Some(());
                };
                let safe = Inferior::safe_value(&ty, &value);
                let type_json = Inferior::jet_type_name(&ty)
                    .map(|ty| format!(",\"type\":\"{}\"", json_escape(ty)))
                    .unwrap_or_default();
                self.respond(
                    out,
                    request_seq,
                    command,
                    true,
                    &format!(
                        "{{\"result\":\"{}\"{} ,\"variablesReference\":0}}",
                        json_escape(&safe),
                        type_json
                    ),
                );
                Some(())
            }
            "continue" | "next" | "stepIn" | "stepOut" => {
                if self.state != State::Stopped {
                    self.respond_jet_error(
                        out,
                        request_seq,
                        command,
                        22031,
                        "run control requires a stopped target",
                    );
                    return Some(());
                }
                if self.inf.is_none() {
                    self.invalidate_references();
                    self.respond_jet_error(
                        out,
                        request_seq,
                        command,
                        22034,
                        "Jet debugger has no target",
                    );
                    return Some(());
                }
                self.invalidate_references();
                let resume_cmd = match command {
                    "continue" => "continue",
                    "next" => "thread step-over",
                    "stepIn" => "thread step-in",
                    _ => "thread step-out",
                };
                self.state = State::Running;
                self.respond(
                    out,
                    request_seq,
                    command,
                    true,
                    "{\"allThreadsContinued\":true}",
                );
                let result = self
                    .inf
                    .as_mut()
                    .map(|inf| inf.resume_and_locate(resume_cmd));
                match result {
                    Some(Ok(result)) => {
                        let mode = if command == "continue" {
                            ResumeMode::Continue
                        } else {
                            ResumeMode::Step
                        };
                        self.emit_resume(out, result, mode);
                    }
                    Some(Err(error)) => {
                        self.terminate_backend(
                            out,
                            &error,
                            "the native debugger lost the running session",
                        );
                    }
                    None => {
                        self.terminate_with_diagnostic(out, "E2234", "Jet debugger has no target");
                    }
                }
                Some(())
            }
            "pause" => {
                if self.state != State::Running {
                    self.respond_jet_error(
                        out,
                        request_seq,
                        command,
                        22031,
                        "pause requires a running target",
                    );
                    return Some(());
                }
                if self.inf.is_none() {
                    self.respond_jet_error(
                        out,
                        request_seq,
                        command,
                        22034,
                        "Jet debugger has no target",
                    );
                    return Some(());
                }
                self.invalidate_references();
                self.respond(out, request_seq, command, true, "{}");
                let result = self.inf.as_mut().map(Inferior::interrupt);
                match result {
                    Some(Ok(bt_text)) => {
                        self.emit_resume(out, ResumeResult::Stopped(bt_text), ResumeMode::Pause);
                    }
                    Some(Err(error)) => {
                        self.terminate_backend(
                            out,
                            &error,
                            "the native debugger could not pause the program",
                        );
                    }
                    None => {
                        self.terminate_with_diagnostic(out, "E2234", "Jet debugger has no target");
                    }
                }
                Some(())
            }
            "setExceptionBreakpoints" => {
                if !matches!(
                    self.state,
                    State::Ready | State::Configuring | State::Running | State::Stopped
                ) {
                    self.respond_jet_error(
                        out,
                        request_seq,
                        command,
                        22031,
                        "exception filters are not legal before initialize or after termination",
                    );
                    return Some(());
                }
                let Some(filters) = args.and_then(|args| json_get(args, "filters")) else {
                    self.exception_filters.clear();
                    self.respond(out, request_seq, command, true, "{}");
                    return Some(());
                };
                let JSONValue::Array(values) = filters else {
                    self.respond_jet_error(
                        out,
                        request_seq,
                        command,
                        22032,
                        "exception filters must be a bounded array of Jet panic, error, signal, or all",
                    );
                    return Some(());
                };
                let mut selected = HashSet::new();
                for value in values {
                    let Some(filter) = json_str(value) else {
                        self.respond_jet_error(
                            out,
                            request_seq,
                            command,
                            22032,
                            "exception filters must be Jet panic, error, signal, or all",
                        );
                        return Some(());
                    };
                    if !matches!(filter, "all" | "error" | "panic" | "signal") {
                        self.respond_jet_error(
                            out,
                            request_seq,
                            command,
                            22032,
                            "exception filters must be Jet panic, error, signal, or all",
                        );
                        return Some(());
                    }
                    selected.insert(filter.to_string());
                }
                self.exception_filters = selected;
                self.respond(out, request_seq, command, true, "{}");
                Some(())
            }
            "exceptionInfo" => {
                if !self.state.is_stopped() {
                    self.respond_jet_error(
                        out,
                        request_seq,
                        command,
                        22031,
                        "exceptionInfo requires a stopped target",
                    );
                    return Some(());
                }
                if let Some(signal) = self.last_signal.as_deref() {
                    self.respond(
                        out,
                        request_seq,
                        command,
                        true,
                        &format!(
                            "{{\"exceptionId\":\"{}\",\"description\":\"Jet target stopped on {}\"}}",
                            json_escape(signal),
                            json_escape(signal)
                        ),
                    );
                } else {
                    self.respond_jet_error(
                        out,
                        request_seq,
                        command,
                        22034,
                        "the stopped Jet target has no exception information",
                    );
                }
                Some(())
            }
            "loadedSources" => {
                if !matches!(
                    self.state,
                    State::Ready | State::Configuring | State::Running | State::Stopped
                ) {
                    self.respond_jet_error(
                        out,
                        request_seq,
                        command,
                        22031,
                        "loadedSources requires an initialized debugger session",
                    );
                    return Some(());
                }
                self.respond(
                    out,
                    request_seq,
                    command,
                    true,
                    &format!(
                        "{{\"sources\":[{{\"name\":\"{}\",\"path\":\"{}\"}}]}}",
                        json_escape(&self.source_path()),
                        json_escape(&self.source_path())
                    ),
                );
                Some(())
            }
            "cancel" => {
                if !matches!(
                    self.state,
                    State::Ready | State::Configuring | State::Running | State::Stopped
                ) {
                    self.respond_jet_error(
                        out,
                        request_seq,
                        command,
                        22031,
                        "cancel requires an initialized debugger session",
                    );
                    return Some(());
                }
                self.respond(out, request_seq, command, true, "{}");
                Some(())
            }
            "disconnect" | "terminate" => {
                let terminate = if command == "terminate" {
                    true
                } else {
                    match args.and_then(|args| json_get(args, "terminateDebuggee")) {
                        None => self.target == Some(TargetKind::Launched),
                        Some(value) => match json_bool(value) {
                            Some(value) => value,
                            None => {
                                self.respond_jet_error(
                                    out,
                                    request_seq,
                                    command,
                                    22032,
                                    "terminateDebuggee must be a boolean",
                                );
                                return Some(());
                            }
                        },
                    }
                };
                self.invalidate_references();
                self.state = State::Terminated;
                match self.close_target(terminate) {
                    Ok(()) => {
                        self.respond(out, request_seq, command, true, "{}");
                        if command == "terminate" {
                            self.event(out, "terminated", "{}");
                        }
                    }
                    Err(error) => {
                        let (id, message) = cleanup_error_details(&error);
                        self.respond_jet_error(out, request_seq, command, id, message)
                    }
                }
                None
            }
            _ => {
                self.respond_jet_error(
                    out,
                    request_seq,
                    command,
                    22033,
                    "this Jet debugger operation is unsupported; use read-only Jet stack, scope, or evaluation requests",
                );
                Some(())
            }
        }
    }

    fn apply_pending_breakpoints_and_run(&mut self, out: &mut impl Write) {
        // Keep the source-level requests after the first run.  Restart uses
        // this same list to recreate backend breakpoints in the new inferior.
        let lines = self.pending_breakpoints.clone();
        let old = std::mem::take(&mut self.source_breakpoint_ids);
        let map = &self.map;
        let rust_file = self.rust_file.clone();
        let Some(inf) = self.inf.as_mut() else {
            self.terminate_with_diagnostic(out, "E2234", "Jet debugger has no target");
            return;
        };
        for id in old {
            if let Err(error) = inf.delete_breakpoint(id) {
                // The DAP body stays a fixed string: this file has no JSON
                // escaper, and an arbitrary error text would break the frame.
                eprintln!("error: native debugger could not remove a source breakpoint: {error}");
                self.terminate_with_diagnostic(
                    out,
                    "E2234",
                    "the native debugger could not remove a source breakpoint",
                );
                return;
            }
        }
        let (ids, _) = match Self::install_breakpoints(map, &rust_file, &lines, inf) {
            Ok(result) => result,
            Err(_error) => {
                self.terminate_with_diagnostic(
                    out,
                    "E2234",
                    "the native debugger could not install source breakpoints",
                );
                return;
            }
        };
        self.source_breakpoint_ids = ids;
        self.state = State::Running;
        let resume = if self.target == Some(TargetKind::Attached) {
            "continue"
        } else {
            "run"
        };
        let result = self.inf.as_mut().map(|inf| inf.resume_and_locate(resume));
        match result {
            Some(Ok(result)) => {
                let mode = if self.target == Some(TargetKind::Launched) {
                    ResumeMode::Launch
                } else {
                    ResumeMode::Continue
                };
                self.emit_resume(out, result, mode);
            }
            Some(Err(error)) => {
                self.terminate_backend(out, &error, "the native debugger lost the running session")
            }
            None => self.terminate_with_diagnostic(
                out,
                "E2234",
                "the native debugger has no target to run",
            ),
        }
    }

    /// Send a DAP `output` event for any Jet `print()`/`eprint()` the debuggee
    /// wrote since the last check (redirected off lldb's own control channel —
    /// `Inferior.rs`'s module doc point 3).
    fn emit_program_output(&mut self, out: &mut impl Write) {
        let Some(inf) = &mut self.inf else { return };
        let (stdout, stderr) = inf.drain_program_output();
        if !stdout.is_empty() {
            let body = format!(
                "{{\"category\":\"stdout\",\"output\":\"{}\"}}",
                json_escape(&self.output_text(&stdout))
            );
            self.event(out, "output", &body);
        }
        if !stderr.is_empty() {
            let body = format!(
                "{{\"category\":\"stderr\",\"output\":\"{}\"}}",
                json_escape(&self.output_text(&stderr))
            );
            self.event(out, "output", &body);
        }
    }

    fn emit_resume(&mut self, out: &mut impl Write, result: ResumeResult, mode: ResumeMode) {
        self.emit_resume_inner(out, result, mode, 0);
    }

    fn emit_resume_inner(
        &mut self,
        out: &mut impl Write,
        result: ResumeResult,
        mode: ResumeMode,
        depth: usize,
    ) {
        let mut result = result;
        let mut mode = mode;
        let mut depth = depth;
        loop {
            if depth >= 200 && matches!(&result, ResumeResult::Stopped(_)) {
                self.terminate_with_diagnostic(
                    out,
                    "E2235",
                    "native debugger produced too many stops without a verified Jet source mapping",
                );
                return;
            }
            self.emit_program_output(out);
            let bt_text = match result {
                ResumeResult::Exited { status, signal } => {
                    self.invalidate_references();
                    self.state = State::Terminated;
                    self.last_signal = signal.clone();
                    let body = match (status, signal) {
                        (Some(code), Some(signal)) => {
                            format!(
                                "{{\"exitCode\":{},\"signal\":\"{}\"}}",
                                code,
                                json_escape(&signal)
                            )
                        }
                        (Some(code), None) => format!("{{\"exitCode\":{code}}}"),
                        (None, Some(signal)) => {
                            format!("{{\"signal\":\"{}\"}}", json_escape(&signal))
                        }
                        (None, None) => "{}".to_string(),
                    };
                    self.event(out, "exited", &body);
                    self.cleanup_after_terminal_state(out);
                    self.event(out, "terminated", "{}");
                    return;
                }
                ResumeResult::Stopped(text) => text,
            };
            self.last_signal = parse_exit_signal(&bt_text);
            if mode != ResumeMode::Pause {
                if let Some(kind) = self.exception_kind(&bt_text) {
                    if !self.exception_filter_enabled(kind) {
                        match self.resume_hidden_stop("continue") {
                            Ok(next) => {
                                result = next;
                                mode = ResumeMode::Continue;
                                depth += 1;
                                continue;
                            }
                            Err(error) => {
                                self.terminate_backend(
                                out,
                                &error,
                                "the native debugger lost the running session while skipping a non-Jet stop",
                            );
                                return;
                            }
                        }
                    }
                }
            }
            if let Some(thread) = parse_current_thread_id(&bt_text) {
                self.current_thread = thread;
            }
            self.state = State::Stopped;
            let entry_stop = mode == ResumeMode::Launch
                && self.entry_breakpoint_id.is_some()
                && bt_text.contains("stop reason = breakpoint");
            if entry_stop {
                let entry_id = self
                    .entry_breakpoint_id
                    .take()
                    .expect("entry breakpoint checked");
                let removed = self.inf.as_mut().map(|inf| inf.delete_breakpoint(entry_id));
                if !matches!(removed, Some(Ok(()))) {
                    self.terminate_with_diagnostic(
                        out,
                        "E2234",
                        "the native debugger could not retire the entry breakpoint",
                    );
                    return;
                }
            }
            let reason = if self.last_signal.is_some()
                || bt_text.contains("signal")
                || bt_text.contains("exception")
            {
                "exception"
            } else if entry_stop {
                "entry"
            } else if mode == ResumeMode::Step {
                "step"
            } else if mode == ResumeMode::Pause {
                "pause"
            } else {
                "breakpoint"
            };
            let mut hit_breakpoint_ids = Vec::new();
            if let Some(frame) = Inferior::parse_top_frame(&bt_text) {
                match self
                    .map
                    .jet_line_for_file(&frame.rust_file, &self.rust_file, frame.rust_line)
                {
                    None if !self.show_raw_frames && mode != ResumeMode::Pause => {
                        if self.unmapped_steps >= 200 {
                            self.terminate_with_diagnostic(
                            out,
                            "E2235",
                            "native debugger produced too many stops without a verified Jet source mapping",
                        );
                            return;
                        }
                        self.unmapped_steps += 1;
                        match self.resume_hidden_stop("thread step-over") {
                            Ok(next) => {
                                result = next;
                                depth += 1;
                                continue;
                            }
                            Err(error) => {
                                self.terminate_backend(
                                out,
                                &error,
                                "the native debugger lost the running session while skipping a non-Jet stop",
                            );
                                return;
                            }
                        }
                    }
                    Some(jline)
                        if !entry_stop && mode != ResumeMode::Step && mode != ResumeMode::Pause =>
                    {
                        self.unmapped_steps = 0;
                        match self.breakpoint_action(jline) {
                            BreakpointAction::Stop(ids) => {
                                hit_breakpoint_ids = ids;
                            }
                            BreakpointAction::Continue => {
                                match self.resume_hidden_stop("continue") {
                                    Ok(next) => {
                                        result = next;
                                        mode = ResumeMode::Continue;
                                        // A verified Jet breakpoint that did not
                                        // match is normal control flow, not a
                                        // backend stop storm.
                                        depth = 0;
                                        continue;
                                    }
                                    Err(error) => {
                                        self.terminate_backend(
                                        out,
                                        &error,
                                        "the native debugger lost the running session while skipping a non-Jet stop",
                                    );
                                        return;
                                    }
                                }
                            }
                            BreakpointAction::Log(message) => {
                                self.event(
                                    out,
                                    "output",
                                    &format!(
                                        "{{\"category\":\"console\",\"output\":\"{}\\n\"}}",
                                        json_escape(&message)
                                    ),
                                );
                                match self.resume_hidden_stop("continue") {
                                    Ok(next) => {
                                        result = next;
                                        mode = ResumeMode::Continue;
                                        depth = 0;
                                        continue;
                                    }
                                    Err(error) => {
                                        self.terminate_backend(
                                        out,
                                        &error,
                                        "the native debugger lost the running session while skipping a non-Jet stop",
                                    );
                                        return;
                                    }
                                }
                            }
                        }
                    }
                    None => {
                        self.unmapped_steps = 0;
                    }
                    Some(_) => {
                        self.unmapped_steps = 0;
                    }
                }
            }
            match Inferior::parse_top_frame(&bt_text) {
                Some(frame) => {
                    let _ = self.source_line(
                        self.map
                            .jet_line_for_file(&frame.rust_file, &self.rust_file, frame.rust_line)
                            .unwrap_or(1),
                    );
                    let hit_ids = if hit_breakpoint_ids.is_empty() {
                        String::new()
                    } else {
                        format!(
                            ",\"hitBreakpointIds\":[{}]",
                            hit_breakpoint_ids
                                .iter()
                                .map(u32::to_string)
                                .collect::<Vec<_>>()
                                .join(",")
                        )
                    };
                    let body = format!(
                        "{{\"reason\":\"{}\",\"threadId\":{},\"allThreadsStopped\":true{}}}",
                        reason, self.current_thread, hit_ids
                    );
                    self.event(out, "stopped", &body);
                }
                None => {
                    // No parseable frame cannot be represented as a Jet stop. A
                    // raw backend transcript is never a substitute for a Jet
                    // frame, even when the stop was caused by a signal.
                    self.terminate_with_diagnostic(
                        out,
                        "E2235",
                        "native debugger stopped without a verified Jet frame",
                    );
                }
            }
            return;
        }
    }

    fn resume_hidden_stop(&mut self, resume_command: &str) -> Result<ResumeResult, io::Error> {
        self.state = State::Running;
        let Some(inf) = self.inf.as_mut() else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "the native debugger has no target while skipping a non-Jet stop",
            ));
        };
        inf.resume_and_locate(resume_command)
    }

    fn exception_kind(&self, backtrace: &str) -> Option<&'static str> {
        if self.last_signal.is_some() {
            return Some("signal");
        }
        let text = backtrace.to_ascii_lowercase();
        if text.contains("panic") || text.contains("unwind") {
            Some("panic")
        } else if text.contains("exception") || text.contains("fatal error") {
            Some("error")
        } else {
            None
        }
    }

    fn exception_filter_enabled(&self, kind: &str) -> bool {
        self.exception_filters.contains("all") || self.exception_filters.contains(kind)
    }

    fn terminate_backend(&mut self, out: &mut impl Write, error: &io::Error, message: &str) {
        let id = backend_error_id(error);
        let (diagnostic, _, _) = diagnostic_details(id, message);
        self.terminate_with_diagnostic(out, diagnostic, message);
    }

    fn terminate_with_diagnostic(&mut self, out: &mut impl Write, diagnostic: &str, message: &str) {
        self.state = State::Terminated;
        self.invalidate_references();
        self.cleanup_after_terminal_state(out);
        let body = format!(
            "{{\"category\":\"stderr\",\"output\":\"[{}] {}\\n\"}}",
            json_escape(diagnostic),
            json_escape(message)
        );
        self.event(out, "output", &body);
        self.event(out, "terminated", "{}");
    }

    fn breakpoint_action(&mut self, line: usize) -> BreakpointAction {
        if self.pending_specs.is_empty() {
            return BreakpointAction::Continue;
        }
        let mut log = None;
        let mut matched = false;
        let mut stop_ids = Vec::new();
        let mut should_stop = false;
        for index in 0..self.pending_specs.len() {
            let spec = self.pending_specs[index].clone();
            if spec.line != line {
                continue;
            }
            matched = true;
            let count = self
                .hit_counts
                .get_mut(index)
                .map(|count| {
                    *count = count.saturating_add(1);
                    *count
                })
                .unwrap_or(1);
            if spec
                .condition
                .as_deref()
                .is_some_and(|condition| !self.condition_matches(condition))
                || spec
                    .hit_condition
                    .as_deref()
                    .is_some_and(|condition| !hit_condition_matches(condition, count))
            {
                continue;
            }
            if let Some(message) = spec.log_message.as_deref() {
                log = Some(self.expand_log_message(message));
            } else {
                should_stop = true;
                if let Some(id) = self.client_breakpoint_ids.get(index).copied() {
                    stop_ids.push(id);
                }
            }
        }
        if should_stop {
            return BreakpointAction::Stop(stop_ids);
        }
        if let Some(message) = log {
            BreakpointAction::Log(message)
        } else if matched {
            BreakpointAction::Continue
        } else {
            // A `continue` request stops only at a requested source
            // breakpoint.  Every other mapped statement is ordinary program
            // execution and must stay hidden from the editor.
            BreakpointAction::Continue
        }
    }

    fn condition_matches(&mut self, condition: &str) -> bool {
        let operators = ["==", "!=", ">=", "<=", ">", "<"];
        let Some((left, operator, right)) = operators.iter().find_map(|operator| {
            condition
                .split_once(operator)
                .map(|parts| (parts.0, *operator, parts.1))
        }) else {
            return self
                .read_jet_value(condition)
                .is_some_and(|value| value == "true");
        };
        let Some(value) = self.read_jet_value(left.trim()) else {
            return false;
        };
        let right = right.trim();
        if let (Ok(left), Ok(right)) = (value.parse::<f64>(), right.parse::<f64>()) {
            return match operator {
                "==" => left == right,
                "!=" => left != right,
                ">=" => left >= right,
                "<=" => left <= right,
                ">" => left > right,
                "<" => left < right,
                _ => false,
            };
        }
        match operator {
            "==" => value == right,
            "!=" => value != right,
            _ => false,
        }
    }

    fn read_jet_value(&mut self, expression: &str) -> Option<String> {
        let (root, suffix) = jet_expression(expression)?;
        let rust_expression = Inferior::jet_path_to_rust(&root, &suffix)?;
        let output = self.inf.as_mut()?.frame_variable(&rust_expression).ok()?;
        Inferior::parse_typed_locals(&output)
            .into_iter()
            .next()
            .map(|(ty, _, value)| Inferior::safe_value(&ty, &value))
    }

    fn expand_log_message(&mut self, message: &str) -> String {
        let mut rendered = String::new();
        let mut rest = message;
        while let Some(open) = rest.find('{') {
            rendered.push_str(&rest[..open]);
            let after_open = &rest[open + 1..];
            let Some(close) = after_open.find('}') else {
                rendered.push_str(&rest[open..]);
                return rendered;
            };
            let expression = &after_open[..close];
            rendered.push_str(
                &self
                    .read_jet_value(expression)
                    .unwrap_or_else(|| "<unavailable>".to_string()),
            );
            rest = &after_open[close + 1..];
        }
        rendered.push_str(rest);
        rendered
    }

    fn respond(
        &mut self,
        out: &mut impl Write,
        request_seq: i64,
        command: &str,
        success: bool,
        body: &str,
    ) {
        let seq = self.next_seq();
        let json = format!(
            "{{\"seq\":{},\"type\":\"response\",\"request_seq\":{},\"success\":{},\"command\":\"{}\",\"body\":{}}}",
            seq,
            request_seq,
            success,
            json_escape(command),
            body
        );
        let _ = write_message(out, &json);
    }

    fn respond_err(
        &mut self,
        out: &mut impl Write,
        request_seq: i64,
        command: &str,
        message: &str,
    ) {
        let id = if message.contains("must")
            || message.contains("required")
            || message.contains("positive")
            || message.contains("invalid")
            || message.contains("array")
        {
            22032
        } else {
            22034
        };
        self.respond_jet_error(out, request_seq, command, id, message);
    }

    fn respond_backend_error(
        &mut self,
        out: &mut impl Write,
        request_seq: i64,
        command: &str,
        error: &io::Error,
        message: &str,
    ) {
        let id = backend_error_id(error);
        self.respond_jet_error(out, request_seq, command, id, message);
    }

    fn respond_stale_reference(&mut self, out: &mut impl Write, request_seq: i64, command: &str) {
        self.respond_jet_error(
            out,
            request_seq,
            command,
            22036,
            "frame, scope, or variable reference expired when execution resumed",
        );
    }

    fn respond_jet_error(
        &mut self,
        out: &mut impl Write,
        request_seq: i64,
        command: &str,
        id: i64,
        format: &str,
    ) {
        let message = match id {
            22031 => "jet.invalidState",
            22032 => "jet.invalidArguments",
            22033 => "jet.unsupported",
            22034 => "jet.unavailable",
            22035 => "jet.timeout",
            22036 => "jet.staleReference",
            22037 => "jet.mapMismatch",
            22038 => "jet.cancelled",
            _ => "jet.unavailable",
        };
        let (diagnostic, why, fix) = diagnostic_details(id, format);
        let body = format!(
            "{{\"error\":{{\"id\":{},\"format\":\"{}\",\"showUser\":true,\"variables\":{{\"retryable\":\"false\"}}}},\"jetDiagnostic\":{{\"schema\":\"jet.diagnostic/v1\",\"code\":\"{}\",\"what\":\"{}\",\"why\":\"{}\",\"fix\":\"{}\"}}}}",
            id,
            json_escape(format),
            diagnostic,
            json_escape(format),
            json_escape(why),
            json_escape(fix),
        );
        let seq = self.next_seq();
        let json = format!(
            "{{\"seq\":{},\"type\":\"response\",\"request_seq\":{},\"success\":false,\"command\":\"{}\",\"message\":\"{}\",\"body\":{}}}",
            seq,
            request_seq,
            json_escape(command),
            json_escape(message),
            body
        );
        let _ = write_message(out, &json);
    }

    fn event(&mut self, out: &mut impl Write, event: &str, body: &str) {
        let seq = self.next_seq();
        let json = format!(
            "{{\"seq\":{},\"type\":\"event\",\"event\":\"{}\",\"body\":{}}}",
            seq, event, body
        );
        let _ = write_message(out, &json);
    }

    fn progress_start(&mut self, out: &mut impl Write, progress_id: &str, title: &str) {
        self.event(
            out,
            "progressStart",
            &format!(
                "{{\"progressId\":\"{}\",\"title\":\"{}\",\"cancellable\":false}}",
                json_escape(progress_id),
                json_escape(title)
            ),
        );
    }

    fn progress_end(&mut self, out: &mut impl Write, progress_id: &str, message: &str) {
        self.event(
            out,
            "progressEnd",
            &format!(
                "{{\"progressId\":\"{}\",\"message\":\"{}\"}}",
                json_escape(progress_id),
                json_escape(message)
            ),
        );
    }

    fn next_seq(&mut self) -> i64 {
        let s = self.seq;
        self.seq += 1;
        s
    }
}

fn backend_error_id(error: &io::Error) -> i64 {
    match error.kind() {
        io::ErrorKind::TimedOut => 22035,
        io::ErrorKind::BrokenPipe
        | io::ErrorKind::UnexpectedEof
        | io::ErrorKind::ConnectionAborted
        | io::ErrorKind::ConnectionReset => 22038,
        _ => 22034,
    }
}

fn cleanup_error_details(error: &io::Error) -> (i64, &'static str) {
    match error.kind() {
        io::ErrorKind::PermissionDenied => {
            (22037, "debugger could not verify attach target identity")
        }
        io::ErrorKind::BrokenPipe
        | io::ErrorKind::UnexpectedEof
        | io::ErrorKind::ConnectionAborted
        | io::ErrorKind::ConnectionReset => (
            22038,
            "debug session connection closed before cleanup was proven",
        ),
        _ => (22034, "the debugger could not cleanly close the target"),
    }
}

fn default_exception_filters() -> HashSet<String> {
    ["panic".to_string()].into_iter().collect()
}

fn diagnostic_details(id: i64, format: &str) -> (&'static str, &'static str, &'static str) {
    match id {
        22031 => (
            "E2232",
            "the DAP lifecycle or stop generation does not permit the request",
            "wait for the stated lifecycle event, then retry",
        ),
        22032 => (
            "E2236",
            "a request field violates the debugger's bounded local contract",
            "correct the field named in the diagnostic, then retry",
        ),
        22033 => (
            "E2234",
            "the supported local backend is missing or returned an unsupported result",
            "install or repair the supported backend, then retry",
        ),
        22034 if format.contains("attach") || format.contains("identity") => (
            "E2231",
            "the process identity, executable, source hashes, or `.jetmap` did not remain verified",
            "stop the target or choose the matching local Jet build, then attach again",
        ),
        22034 => (
            "E2234",
            "the supported local backend is missing, lost, or returned an unsupported result",
            "install or repair the supported backend, then retry",
        ),
        22035 => (
            "E2233",
            "the backend did not answer before the bounded read deadline",
            "check the target process and start a new session",
        ),
        22036 => (
            "E2238",
            "a frame, scope, or variable reference expired when execution resumed",
            "refresh the editor's stack and variables after the next stop",
        ),
        22037 => (
            "E2231",
            "the process identity, executable, source hashes, or `.jetmap` did not remain verified",
            "stop the target or choose the matching local Jet build, then attach again",
        ),
        22038 => (
            "E2237",
            "the editor, adapter, or debugger control channel closed before cleanup was proven",
            "check the target process, then start a new debug session",
        ),
        _ => (
            "E2239",
            "Jet could not preserve a debugger invariant",
            "start a new session and report the diagnostic if it repeats",
        ),
    }
}

fn parse_dap_request(body: &str) -> Result<JSONValue, &'static str> {
    let message =
        parse_json_with_limit(body, MAX_DAP_MESSAGE_BYTES).map_err(|()| "invalid JSON")?;
    let JSONValue::Object(_) = &message else {
        return Err("request must be an object");
    };
    if json_get(&message, "type").and_then(json_str) != Some("request") {
        return Err("type must be request");
    }
    let seq = json_get(&message, "seq")
        .and_then(json_u32)
        .ok_or("seq must be a nonnegative integer")?;
    if seq == 0 {
        return Err("seq must be positive");
    }
    let command = json_get(&message, "command")
        .and_then(json_str)
        .ok_or("command must be a string")?;
    if command.is_empty() {
        return Err("command must not be empty");
    }
    if !matches!(
        json_get(&message, "arguments"),
        None | Some(JSONValue::Object(_))
    ) {
        return Err("arguments must be an object");
    }
    Ok(message)
}

fn jet_expression(value: &str) -> Option<(String, String)> {
    let value = value.trim();
    if value.is_empty() || value.len() > 256 || value.chars().any(|c| c.is_control()) {
        return None;
    }
    let root_len = value
        .char_indices()
        .find(|(_, c)| !c.is_ascii_alphanumeric() && *c != '_')
        .map(|(index, _)| index)
        .unwrap_or(value.len());
    let root = &value[..root_len];
    if root.is_empty()
        || root.starts_with('_')
        || !root.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
    {
        return None;
    }
    let suffix = &value[root_len..];
    if !suffix
        .chars()
        .all(|c| c == '.' || c == '[' || c == ']' || c.is_ascii_digit())
    {
        return None;
    }
    Some((root.to_string(), suffix.to_string()))
}

struct InitializeArguments {
    lines_start_at_1: bool,
    columns_start_at_1: bool,
    supports_ansi_styling: bool,
}

fn parse_initialize_arguments(
    args: Option<&JSONValue>,
) -> Result<InitializeArguments, &'static str> {
    let args = args.ok_or("initialize arguments are required")?;
    if json_get(args, "adapterID").and_then(json_str) != Some("jet") {
        return Err("initialize adapterID must be `jet`");
    }
    if let Some(path_format) = json_get(args, "pathFormat") {
        if json_str(path_format) != Some("path") {
            return Err("initialize pathFormat must be `path`");
        }
    }
    let lines_start_at_1 = match json_get(args, "linesStartAt1") {
        None => true,
        Some(value) => json_bool(value).ok_or("initialize linesStartAt1 must be a boolean")?,
    };
    let columns_start_at_1 = match json_get(args, "columnsStartAt1") {
        None => true,
        Some(value) => json_bool(value).ok_or("initialize columnsStartAt1 must be a boolean")?,
    };
    let supports_ansi_styling = match json_get(args, "supportsANSIStyling") {
        None => false,
        Some(value) => {
            json_bool(value).ok_or("initialize supportsANSIStyling must be a boolean")?
        }
    };
    Ok(InitializeArguments {
        lines_start_at_1,
        columns_start_at_1,
        supports_ansi_styling,
    })
}

struct LaunchArguments {
    args: Vec<String>,
    cwd: Option<String>,
    env: Vec<(String, String)>,
    show_raw_frames: bool,
}

fn parse_launch_arguments(
    args: Option<&JSONValue>,
    jet_file: &str,
) -> Result<LaunchArguments, &'static str> {
    if let Some(program) = args
        .and_then(|args| json_get(args, "program"))
        .and_then(json_str)
    {
        if !same_path(program, jet_file) {
            return Err("launch program must be the selected Jet source file");
        }
    }
    let values = match args.and_then(|args| json_get(args, "args")) {
        None => Vec::new(),
        Some(JSONValue::Array(values)) => values
            .iter()
            .map(|value| json_str(value).ok_or("launch args must be strings"))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(str::to_string)
            .collect(),
        Some(_) => return Err("launch args must be an array"),
    };
    let cwd = match args.and_then(|args| json_get(args, "cwd")) {
        None => std::fs::canonicalize(jet_file)
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .map(|path| path.to_string_lossy().into_owned()),
        Some(value) => {
            let raw = json_str(value).ok_or("launch cwd must be a string")?;
            let path =
                std::fs::canonicalize(raw).map_err(|_| "launch cwd must be a local directory")?;
            if !path.is_dir() {
                return Err("launch cwd must be a local directory");
            }
            if let Ok(source) = std::fs::canonicalize(jet_file) {
                if !source.starts_with(&path) {
                    return Err("launch cwd must contain the selected Jet source");
                }
            }
            Some(path.to_string_lossy().into_owned())
        }
    };
    let env = match args.and_then(|args| json_get(args, "env")) {
        None => Vec::new(),
        Some(JSONValue::Object(values)) => values
            .iter()
            .map(|(key, value)| {
                if !valid_launch_env_key(key) {
                    return Err("launch environment keys must be ASCII letters, digits, or `_`");
                }
                Ok((
                    key.clone(),
                    json_str(value)
                        .ok_or("launch environment values must be strings")?
                        .to_string(),
                ))
            })
            .collect::<Result<Vec<_>, &'static str>>()?,
        Some(_) => return Err("launch env must be an object of strings"),
    };
    let show_raw_frames = match args.and_then(|args| json_get(args, "showRawFrames")) {
        None => false,
        Some(value) => json_bool(value).ok_or("showRawFrames must be a boolean")?,
    };
    Ok(LaunchArguments {
        args: values,
        cwd,
        env,
        show_raw_frames,
    })
}

fn valid_launch_env_key(key: &str) -> bool {
    !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn same_path(left: &str, right: &str) -> bool {
    let left_path = Path::new(left);
    let right_path = Path::new(right);
    if left_path == right_path {
        return true;
    }
    match (
        std::fs::canonicalize(left_path),
        std::fs::canonicalize(right_path),
    ) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

#[cfg(test)]
fn breakpoint_lines(args: Option<&JSONValue>) -> Result<Vec<usize>, &'static str> {
    breakpoint_lines_for_origin(args, true)
}

#[cfg(test)]
fn breakpoint_lines_for_origin(
    args: Option<&JSONValue>,
    lines_start_at_1: bool,
) -> Result<Vec<usize>, &'static str> {
    let Some(value) = args.and_then(|args| json_get(args, "breakpoints")) else {
        return Ok(Vec::new());
    };
    let JSONValue::Array(items) = value else {
        return Err("breakpoints must be an array");
    };
    items
        .iter()
        .map(|item| {
            let line = json_get(item, "line")
                .and_then(json_u32)
                .ok_or("breakpoint line must be a nonnegative integer")?;
            if lines_start_at_1 && line == 0 {
                return Err("breakpoint line must be a positive integer");
            }
            let line = if lines_start_at_1 {
                line
            } else {
                line.checked_add(1).ok_or("breakpoint line is too large")?
            };
            usize::try_from(line).map_err(|_| "breakpoint line is too large")
        })
        .collect()
}

fn breakpoint_source(args: Option<&JSONValue>, jet_file: &str) -> Result<(), &'static str> {
    let source = args
        .and_then(|args| json_get(args, "source"))
        .ok_or("setBreakpoints requires a source object")?;
    let path = json_get(source, "path")
        .and_then(json_str)
        .filter(|path| !path.is_empty() && !path.chars().any(char::is_control))
        .ok_or("setBreakpoints source.path must be a non-empty string")?;
    if !same_path(path, jet_file) {
        return Err("setBreakpoints source.path is not the selected Jet file");
    }
    Ok(())
}

fn breakpoint_specs_for_origin(
    args: Option<&JSONValue>,
    lines_start_at_1: bool,
) -> Result<Vec<BreakpointSpec>, &'static str> {
    let Some(value) = args.and_then(|args| json_get(args, "breakpoints")) else {
        return Ok(Vec::new());
    };
    let JSONValue::Array(items) = value else {
        return Err("breakpoints must be an array");
    };
    items
        .iter()
        .map(|item| {
            let line = json_get(item, "line")
                .and_then(json_u32)
                .ok_or("breakpoint line must be a nonnegative integer")?;
            if lines_start_at_1 && line == 0 {
                return Err("breakpoint line must be a positive integer");
            }
            let line = if lines_start_at_1 {
                line
            } else {
                line.checked_add(1).ok_or("breakpoint line is too large")?
            };
            let line = usize::try_from(line).map_err(|_| "breakpoint line is too large")?;
            let condition = breakpoint_text(item, "condition")?;
            let hit_condition = breakpoint_text(item, "hitCondition")?;
            let log_message = breakpoint_text(item, "logMessage")?;
            Ok(BreakpointSpec {
                line,
                condition,
                hit_condition,
                log_message,
            })
        })
        .try_fold(Vec::new(), |mut specs, spec| {
            let spec = spec?;
            if !specs.contains(&spec) {
                specs.push(spec);
            }
            Ok(specs)
        })
}

fn breakpoint_text(item: &JSONValue, key: &str) -> Result<Option<String>, &'static str> {
    let Some(value) = json_get(item, key) else {
        return Ok(None);
    };
    let text = json_str(value).ok_or("breakpoint conditions and log messages must be strings")?;
    if text.len() > 512 || text.chars().any(char::is_control) {
        return Err("breakpoint condition or log message is too long or contains control text");
    }
    Ok(Some(text.to_string()))
}

fn hit_condition_matches(condition: &str, count: u32) -> bool {
    let condition = condition.trim();
    if let Some(value) = condition.strip_prefix(">=") {
        return value
            .trim()
            .parse::<u32>()
            .ok()
            .filter(|value| *value > 0)
            .is_some_and(|value| count >= value);
    }
    condition
        .parse::<u32>()
        .ok()
        .filter(|value| *value > 0)
        .map(|value| count == value)
        .unwrap_or(false)
}

fn valid_hit_condition(condition: &str) -> bool {
    let condition = condition.trim();
    let value = condition.strip_prefix(">=").unwrap_or(condition).trim();
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<u32>().ok().is_some_and(|value| value > 0)
}

fn valid_condition(condition: &str) -> bool {
    let condition = condition.trim();
    if jet_expression(condition).is_some() {
        return true;
    }
    ["==", "!=", ">=", "<=", ">", "<"]
        .iter()
        .find_map(|operator| condition.split_once(operator))
        .is_some_and(|(left, right)| {
            jet_expression(left.trim()).is_some()
                && !right.trim().is_empty()
                && !right.chars().any(char::is_control)
        })
}

fn parse_attach_arguments(args: Option<&JSONValue>) -> Result<AttachArguments, &'static str> {
    let args = args.ok_or("attach arguments are required")?;
    let pid = json_get(args, "processId")
        .and_then(json_u32)
        .filter(|pid| *pid > 0)
        .ok_or("attach processId must be a positive integer")?;
    let program = attach_path(args, "program")?;
    let map = attach_path(args, "map")?;
    let show_raw_frames = match json_get(args, "showRawFrames") {
        None => false,
        Some(value) => json_bool(value).ok_or("showRawFrames must be a boolean")?,
    };
    Ok(AttachArguments {
        pid,
        program,
        map,
        show_raw_frames,
    })
}

fn attach_path(args: &JSONValue, key: &str) -> Result<PathBuf, &'static str> {
    let raw = json_get(args, key)
        .and_then(json_str)
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_control))
        .ok_or("attach paths must be non-empty strings")?;
    let metadata = std::fs::symlink_metadata(raw).map_err(|_| "attach path is not a local file")?;
    if metadata.file_type().is_symlink() {
        return Err("attach paths must not be symbolic links");
    }
    let path = std::fs::canonicalize(raw).map_err(|_| "attach path is not a local file")?;
    if !path.is_file() {
        return Err("attach path is not a local file");
    }
    Ok(path)
}

fn json_bool(value: &JSONValue) -> Option<bool> {
    match value {
        JSONValue::Bool(value) => Some(*value),
        _ => None,
    }
}

/// Content-Length framed read, the same convention `Source/LSP/Server.rs` uses
/// for LSP (DAP shares the exact same header-framing rule).
fn read_message(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut content_length = None;
    let mut total = 0usize;
    let mut fields = 0usize;
    let mut saw_header = false;
    let len = loop {
        let remaining = MAX_PROTOCOL_HEADER_BYTES.saturating_sub(total);
        let mut line = String::new();
        let read =
            std::io::Read::take(&mut *reader, (remaining + 1) as u64).read_line(&mut line)?;
        if read == 0 {
            if saw_header {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "protocol headers ended before the frame body",
                ));
            }
            return Ok(None);
        }
        saw_header = true;
        if read > remaining {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "protocol headers exceed the 8192-byte limit",
            ));
        }
        total += read;
        if line == "\r\n" || line == "\n" {
            break content_length.ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "protocol frame has no Content-Length",
                )
            })?;
        }
        fields += 1;
        if fields > MAX_PROTOCOL_HEADER_COUNT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "protocol headers exceed the 64-field limit",
            ));
        }
        let header = line
            .strip_suffix("\r\n")
            .or_else(|| line.strip_suffix('\n'))
            .unwrap_or(&line);
        let Some((name, value)) = header.split_once(':') else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "protocol header is malformed",
            ));
        };
        if !name.is_ascii() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "protocol header name is not ASCII",
            ));
        }
        if name.trim().eq_ignore_ascii_case("Content-Length") {
            if content_length.is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "protocol frame has duplicate Content-Length headers",
                ));
            }
            let value = value.trim();
            if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "protocol Content-Length is invalid",
                ));
            }
            content_length = Some(value.parse::<usize>().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "protocol Content-Length is invalid",
                )
            })?);
        }
    };
    if len > MAX_DAP_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "protocol message exceeds the 16777216-byte limit",
        ));
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    String::from_utf8(body)
        .map(Some)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "protocol message is not UTF-8"))
}

fn write_message<W: Write>(w: &mut W, json: &str) -> std::io::Result<()> {
    write!(
        w,
        "Content-Length: {}\r\n\r\n{}",
        json.as_bytes().len(),
        json
    )?;
    w.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_server(state: State) -> DapServer {
        DapServer {
            map: LineMap::build(""),
            rust_file: "main.rs".to_string(),
            rust_src: String::new(),
            jet_file: "main.jet".to_string(),
            jet_src: String::new(),
            binary: PathBuf::from("missing-debug-binary"),
            inf: None,
            pending_breakpoints: Vec::new(),
            pending_specs: Vec::new(),
            hit_counts: Vec::new(),
            source_breakpoint_ids: Vec::new(),
            client_breakpoint_ids: Vec::new(),
            next_client_breakpoint_id: 1,
            entry_breakpoint_id: None,
            stop_on_entry: true,
            target: None,
            launch_args: Vec::new(),
            launch_cwd: None,
            launch_env: Vec::new(),
            current_thread: 1,
            last_signal: None,
            exception_filters: default_exception_filters(),
            show_raw_frames: false,
            lines_start_at_1: true,
            columns_start_at_1: true,
            supports_ansi_styling: false,
            unmapped_steps: 0,
            references: ObjectReferences::default(),
            state,
            client_sequences: HashSet::new(),
            seq: 1,
        }
    }

    fn frame(body: &str) -> String {
        format!("Content-Length: {}\r\n\r\n{body}", body.len())
    }

    #[test]
    fn dap_request_envelope_rejects_wrong_shapes_and_numbers() {
        assert!(parse_dap_request(
            r#"{"seq":1,"type":"request","command":"initialize","arguments":{"adapterID":"jet"}}"#
        )
        .is_ok());
        for raw in [
            r#"{"seq":1,"type":"event","command":"initialize"}"#,
            r#"{"seq":1.5,"type":"request","command":"initialize"}"#,
            r#"{"seq":-1,"type":"request","command":"initialize"}"#,
            r#"{"seq":0,"type":"request","command":"initialize"}"#,
            r#"{"seq":1,"type":"request","command":""}"#,
            r#"{"seq":1,"type":"request","command":2}"#,
            r#"{"seq":1,"type":"request","command":"initialize","arguments":[]}"#,
            r#"["not","a","request"]"#,
        ] {
            assert!(
                parse_dap_request(raw).is_err(),
                "accepted hostile DAP: {raw}"
            );
        }
    }

    #[test]
    fn initialize_requires_jet_and_negotiates_origins() {
        let mut server = test_server(State::New);
        let rejected = dap_call(
            &mut server,
            r#"{"seq":1,"type":"request","command":"initialize","arguments":{"adapterID":"other"}}"#,
        );
        assert!(rejected.contains("\"success\":false"), "{rejected}");
        assert!(rejected.contains("\"id\":22032"), "{rejected}");
        assert_eq!(server.state, State::New);

        let accepted = dap_call(
            &mut server,
            r#"{"seq":2,"type":"request","command":"initialize","arguments":{"adapterID":"jet","pathFormat":"path","linesStartAt1":false,"columnsStartAt1":false,"supportsANSIStyling":true}}"#,
        );
        assert!(accepted.contains("\"success\":true"), "{accepted}");
        assert!(!accepted.contains("supportsPauseRequest"), "{accepted}");
        assert!(accepted.contains("supportsVariablePaging"), "{accepted}");
        assert!(!server.lines_start_at_1);
        assert!(!server.columns_start_at_1);
        assert!(server.supports_ansi_styling);
    }

    #[test]
    fn dap_framing_accepts_case_insensitive_length_and_rejects_duplicates() {
        let body = r#"{"seq":1,"type":"request","command":"threads"}"#;
        let wire = format!(
            "X-Editor: test\r\ncontent-length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        assert_eq!(
            read_message(&mut std::io::Cursor::new(wire)).unwrap(),
            Some(body.to_string())
        );

        let duplicate = format!("Content-Length: 1\r\ncontent-length: 1\r\n\r\na");
        assert!(read_message(&mut std::io::Cursor::new(duplicate)).is_err());
        let invalid = "Content-Length: +1\r\n\r\na";
        assert!(read_message(&mut std::io::Cursor::new(invalid)).is_err());
    }

    #[test]
    fn breakpoint_ids_are_stable_and_client_lines_follow_negotiation() {
        let mut server = test_server(State::New);
        let initialized = dap_call(
            &mut server,
            r#"{"seq":1,"type":"request","command":"initialize","arguments":{"adapterID":"jet"}}"#,
        );
        assert!(initialized.contains("\"success\":true"), "{initialized}");
        let first = dap_call(
            &mut server,
            r#"{"seq":2,"type":"request","command":"setBreakpoints","arguments":{"source":{"path":"main.jet"},"breakpoints":[{"line":2}]}}"#,
        );
        let first_id = dap_body_id(&first, "breakpoints", "id");
        let second = dap_call(
            &mut server,
            r#"{"seq":3,"type":"request","command":"setBreakpoints","arguments":{"source":{"path":"main.jet"},"breakpoints":[{"line":2}]}}"#,
        );
        assert_eq!(dap_body_id(&second, "breakpoints", "id"), first_id);

        let mut zero_based = test_server(State::New);
        let initialized = dap_call(
            &mut zero_based,
            r#"{"seq":1,"type":"request","command":"initialize","arguments":{"adapterID":"jet","linesStartAt1":false}}"#,
        );
        assert!(initialized.contains("\"success\":true"), "{initialized}");
        let response = dap_call(
            &mut zero_based,
            r#"{"seq":2,"type":"request","command":"setBreakpoints","arguments":{"source":{"path":"main.jet"},"breakpoints":[{"line":0}]}}"#,
        );
        assert!(response.contains("\"line\":0"), "{response}");
        assert_eq!(zero_based.pending_breakpoints, vec![1]);
    }

    #[test]
    fn dap_breakpoint_lines_require_positive_integers() {
        for raw in [
            r#"{"breakpoints":[{"line":-1}]}"#,
            r#"{"breakpoints":[{"line":1.5}]}"#,
            r#"{"breakpoints":[{"line":0}]}"#,
            r#"{"breakpoints":[{"line":"2"}]}"#,
        ] {
            let args = parse_json(raw).unwrap();
            assert!(
                breakpoint_lines(Some(&args)).is_err(),
                "accepted hostile line: {raw}"
            );
        }
        let args = parse_json(r#"{"breakpoints":[{"line":2},{"line":9}]}"#).unwrap();
        assert_eq!(breakpoint_lines(Some(&args)).unwrap(), vec![2, 9]);
    }

    #[test]
    fn launch_raw_frames_are_explicit_and_reference_scopes_are_marked() {
        let args = parse_json(r#"{"program":"main.jet","showRawFrames":true,"args":[],"env":{}}"#)
            .unwrap();
        let launch = parse_launch_arguments(Some(&args), "main.jet").unwrap();
        assert!(launch.show_raw_frames);

        let mut refs = ObjectReferences::default();
        let frame = refs.issue_frame(7, 0);
        let scope = refs.issue_scope(frame, ReferenceKind::RawScope);
        assert!(refs.is_live(scope, ReferenceKind::RawScope));
        assert_eq!(
            refs.scope_location(scope),
            Some(FrameLocation {
                thread_id: 7,
                position: 0,
            })
        );
    }

    #[test]
    fn raw_scopes_append_to_jet_scopes_and_raw_frame_handles_stay_raw() {
        let mut server = test_server(State::Stopped);
        let jet_frame = server.references.issue_frame(1, 0);
        let scopes = dap_call(
            &mut server,
            &format!(
                "{{\"seq\":1,\"type\":\"request\",\"command\":\"scopes\",\"arguments\":{{\"frameId\":{jet_frame},\"showRawFrames\":true}}}}"
            ),
        );
        assert!(scopes.contains("\"name\":\"Locals\""), "{scopes}");
        assert!(scopes.contains("\"name\":\"[raw] Locals\""), "{scopes}");
        assert!(server.references.is_live(2, ReferenceKind::Scope));
        assert!(server.references.is_live(3, ReferenceKind::RawScope));

        let raw_frame = server.references.issue_raw_frame(1, 1);
        let raw_scopes = dap_call(
            &mut server,
            &format!(
                "{{\"seq\":2,\"type\":\"request\",\"command\":\"scopes\",\"arguments\":{{\"frameId\":{raw_frame},\"showRawFrames\":false}}}}"
            ),
        );
        assert!(
            raw_scopes.contains("\"name\":\"[raw] Locals\""),
            "{raw_scopes}"
        );
        assert!(!raw_scopes.contains("\"name\":\"Locals\""), "{raw_scopes}");
    }

    #[test]
    fn attach_arguments_require_local_program_map_and_explicit_raw_frame_shape() {
        let root = std::env::temp_dir().join(format!(
            "jet-dap-attach-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        std::fs::create_dir_all(&root).unwrap();
        let program = root.join("program");
        let map = root.join("program.jetmap");
        std::fs::write(&program, b"program").unwrap();
        std::fs::write(&map, b"map").unwrap();
        let request = parse_json(&format!(
            "{{\"processId\":1,\"program\":\"{}\",\"map\":\"{}\",\"showRawFrames\":false}}",
            json_escape(&program.to_string_lossy()),
            json_escape(&map.to_string_lossy())
        ))
        .unwrap();
        let parsed = parse_attach_arguments(Some(&request)).unwrap();
        assert_eq!(parsed.pid, 1);
        assert_eq!(parsed.program, std::fs::canonicalize(&program).unwrap());
        assert_eq!(parsed.map, std::fs::canonicalize(&map).unwrap());
        assert!(!parsed.show_raw_frames);

        let raw_request = parse_json(&format!(
            "{{\"processId\":1,\"program\":\"{}\",\"map\":\"{}\",\"showRawFrames\":true}}",
            json_escape(&program.to_string_lossy()),
            json_escape(&map.to_string_lossy())
        ))
        .unwrap();
        assert!(
            parse_attach_arguments(Some(&raw_request))
                .unwrap()
                .show_raw_frames
        );

        for raw in [
            "{\"processId\":0}",
            "{\"processId\":1,\"program\":\"missing\",\"map\":\"missing\"}",
        ] {
            let args = parse_json(raw).unwrap();
            assert!(
                parse_attach_arguments(Some(&args)).is_err(),
                "accepted {raw}"
            );
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn dap_attach_invalid_arguments_fails_through_wire_before_backend() {
        let mut server = test_server(State::New);
        let input = format!(
            "{}{}",
            frame(
                r#"{"seq":1,"type":"request","command":"initialize","arguments":{"adapterID":"jet"}}"#
            ),
            frame(r#"{"seq":2,"type":"request","command":"attach","arguments":{"processId":0}}"#),
        );
        let mut output = Vec::new();
        let code = run_io(&mut server, &mut std::io::Cursor::new(input), &mut output);
        let text = String::from_utf8(output).unwrap();
        assert_eq!(code, crate::ExitCodes::OK);
        assert!(text.contains("\"command\":\"attach\""));
        assert!(text.contains("\"success\":false"));
        assert!(text.contains("\"id\":22032"));
        assert!(server.inf.is_none());
    }

    #[test]
    fn dap_attach_rejects_a_real_process_running_another_binary() {
        let root = std::env::temp_dir().join(format!(
            "jet-debug-attach-denied-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        std::fs::create_dir_all(&root).unwrap();
        let program = std::env::current_exe().unwrap();
        let map = root.join("program.jetmap");
        LineMap::write_artifact(&map, "main.jet", "", "main.rs", "", &program).unwrap();
        let mut target = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn real attach target");
        let input = format!(
            "{}{}",
            frame(r#"{"seq":1,"type":"request","command":"initialize","arguments":{"adapterID":"jet"}}"#),
            frame(&format!(
                "{{\"seq\":2,\"type\":\"request\",\"command\":\"attach\",\"arguments\":{{\"processId\":{},\"program\":\"{}\",\"map\":\"{}\"}}}}",
                target.id(),
                json_escape(&program.to_string_lossy()),
                json_escape(&map.to_string_lossy())
            ))
        );
        let mut server = test_server(State::New);
        let mut output = Vec::new();
        let code = run_io(&mut server, &mut std::io::Cursor::new(input), &mut output);
        let text = String::from_utf8(output).unwrap();
        let _ = target.kill();
        let _ = target.wait();
        let _ = std::fs::remove_dir_all(root);
        assert_eq!(code, crate::ExitCodes::OK);
        assert!(text.contains("\"command\":\"attach\""), "{text}");
        assert!(text.contains("\"success\":false"), "{text}");
        assert!(text.contains("\"id\":22037"), "{text}");
        assert!(text.contains("\"code\":\"E2231\""), "{text}");
        assert!(server.inf.is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn dap_attaches_and_disconnects_without_terminating_a_real_target() {
        if !Inferior::available() {
            return;
        }
        let root = std::env::temp_dir().join(format!(
            "jet-dap-attach-disconnect-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create debugger test directory");
        let mut target = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn real attach target");
        let pid = target.id();
        let program = std::fs::canonicalize(format!("/proc/{pid}/exe"))
            .expect("resolve real attach target binary");
        let map = root.join("target.jetmap");
        LineMap::write_artifact(&map, "main.jet", "", "main.rs", "", &program)
            .expect("write matching attach map");
        let input = format!(
            "{}{}{}",
            frame(r#"{"seq":1,"type":"request","command":"initialize","arguments":{"adapterID":"jet"}}"#),
            frame(&format!(
                "{{\"seq\":2,\"type\":\"request\",\"command\":\"attach\",\"arguments\":{{\"processId\":{},\"program\":\"{}\",\"map\":\"{}\"}}}}",
                pid,
                json_escape(&program.to_string_lossy()),
                json_escape(&map.to_string_lossy())
            )),
            frame(r#"{"seq":3,"type":"request","command":"disconnect","arguments":{}}"#),
        );
        let mut server = test_server(State::New);
        let mut output = Vec::new();
        let code = run_io(&mut server, &mut std::io::Cursor::new(input), &mut output);
        let text = String::from_utf8(output).expect("DAP output is UTF-8");
        let target_alive = target.try_wait().expect("check detached target").is_none();
        let _ = target.kill();
        let _ = target.wait();
        let _ = std::fs::remove_dir_all(root);

        assert_eq!(code, crate::ExitCodes::OK, "{text}");
        assert!(text.contains("\"command\":\"attach\""), "{text}");
        assert!(text.contains("\"command\":\"disconnect\""), "{text}");
        assert!(!text.contains("\"success\":false"), "{text}");
        assert!(
            target_alive,
            "disconnect must leave an attached target alive"
        );
        assert_eq!(server.state, State::Terminated);
        assert!(server.inf.is_none());
    }

    #[test]
    fn dap_rejects_oversized_frame_before_reading_a_body() {
        let frame = format!("Content-Length: {}\r\n\r\n", MAX_DAP_MESSAGE_BYTES + 1);
        let error = read_message(&mut std::io::Cursor::new(frame)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            error.to_string(),
            "protocol message exceeds the 16777216-byte limit"
        );
    }

    #[test]
    fn object_references_expire_on_generation_change() {
        let mut refs = ObjectReferences::default();
        let frame = refs.issue(ReferenceKind::Frame);
        let scope = refs.issue(ReferenceKind::Scope);
        assert!(refs.is_live(frame, ReferenceKind::Frame));
        assert!(refs.is_live(scope, ReferenceKind::Scope));

        refs.invalidate();

        assert!(!refs.is_live(frame, ReferenceKind::Frame));
        assert!(!refs.is_live(scope, ReferenceKind::Scope));
        let replacement = refs.issue(ReferenceKind::Frame);
        assert_ne!(replacement, frame);
        assert!(refs.is_live(replacement, ReferenceKind::Frame));

        let stable = refs.issue_frame(1, 0);
        assert_eq!(stable, refs.issue_frame(1, 0));
        assert_ne!(stable, refs.issue_frame(2, 0));
    }

    #[test]
    fn continue_only_stops_at_requested_source_breakpoints() {
        let mut server = test_server(State::Stopped);
        assert!(matches!(
            server.breakpoint_action(3),
            BreakpointAction::Continue
        ));
        server.pending_specs = vec![BreakpointSpec {
            line: 7,
            ..BreakpointSpec::default()
        }];
        server.client_breakpoint_ids = vec![42];
        assert!(matches!(
            server.breakpoint_action(3),
            BreakpointAction::Continue
        ));
        assert!(matches!(
            server.breakpoint_action(7),
            BreakpointAction::Stop(_)
        ));
    }

    fn dap_call(server: &mut DapServer, body: &str) -> String {
        let request = parse_dap_request(body).expect("valid DAP request");
        let mut output = Vec::new();
        assert!(server.handle(&request, &mut output).is_some());
        String::from_utf8(output).expect("DAP output is UTF-8")
    }

    fn dap_message(output: &str) -> JSONValue {
        let mut reader = std::io::Cursor::new(output.as_bytes());
        let body = read_message(&mut reader)
            .expect("DAP response frame")
            .expect("DAP response body");
        parse_json(&body).expect("DAP response JSON")
    }

    fn dap_body_id(output: &str, collection: &str, field: &str) -> u32 {
        let message = dap_message(output);
        let body = json_get(&message, "body").expect("DAP response body object");
        let values = match json_get(body, collection).expect("DAP response collection") {
            JSONValue::Array(values) => values,
            _ => panic!("DAP response collection is not an array"),
        };
        json_get(values.first().expect("DAP response collection item"), field)
            .and_then(json_u32)
            .expect("DAP response id")
    }

    #[test]
    fn dap_restart_rebuilds_target_preserves_breakpoints_and_expires_references() {
        if !Inferior::available() {
            return;
        }
        let root = std::env::temp_dir().join(format!(
            "jet-dap-restart-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create debugger test directory");
        let jet_file = root.join("program.jet");
        let rust_file = root.join("program.rs");
        let binary = root.join("program");
        let jet_src = "fn run() {\n    value := 7\n    print(value)\n}\n";
        let rust_src = "fn main() {\n    // jet:line 2\n    let __jet_value: i32 = 7;\n    // jet:line 3\n    println!(\"{}\", __jet_value);\n}\n";
        std::fs::write(&jet_file, jet_src).expect("write Jet fixture");
        std::fs::write(&rust_file, rust_src).expect("write Rust fixture");
        let rustc = std::process::Command::new("rustc")
            .args(["--edition", "2021", "-C", "debuginfo=2"])
            .arg(&rust_file)
            .arg("-o")
            .arg(&binary)
            .output()
            .expect("run rustc");
        assert!(
            rustc.status.success(),
            "rustc rejected DAP fixture: {}",
            String::from_utf8_lossy(&rustc.stderr)
        );
        let map_path = binary.with_extension("jetmap");
        LineMap::write_artifact(
            &map_path,
            &jet_file.to_string_lossy(),
            jet_src,
            &rust_file.to_string_lossy(),
            rust_src,
            &binary,
        )
        .expect("write debugger map");

        let mut server = test_server(State::New);
        server.map = LineMap::load_verified(
            &map_path,
            &jet_file.to_string_lossy(),
            jet_src,
            &rust_file.to_string_lossy(),
            rust_src,
            &binary,
        )
        .expect("load debugger map");
        server.rust_file = rust_file.to_string_lossy().into_owned();
        server.rust_src = rust_src.to_string();
        server.jet_file = jet_file.to_string_lossy().into_owned();
        server.jet_src = jet_src.to_string();
        server.binary = binary.clone();

        assert!(dap_call(
            &mut server,
            r#"{"seq":1,"type":"request","command":"initialize","arguments":{"adapterID":"jet"}}"#
        )
        .contains("\"success\":true"));
        let breakpoint = dap_call(
            &mut server,
            &format!(
                "{{\"seq\":2,\"type\":\"request\",\"command\":\"setBreakpoints\",\"arguments\":{{\"source\":{{\"path\":\"{}\"}},\"breakpoints\":[{{\"line\":3}}]}}}}",
                json_escape(&jet_file.to_string_lossy())
            ),
        );
        assert!(breakpoint.contains("\"verified\":true"), "{breakpoint}");
        let launch = dap_call(
            &mut server,
            &format!(
                "{{\"seq\":3,\"type\":\"request\",\"command\":\"launch\",\"arguments\":{{\"program\":\"{}\",\"stopOnEntry\":true}}}}",
                json_escape(&jet_file.to_string_lossy())
            ),
        );
        assert!(launch.contains("\"event\":\"initialized\""), "{launch}");
        let configured = dap_call(
            &mut server,
            r#"{"seq":4,"type":"request","command":"configurationDone","arguments":{}}"#,
        );
        assert!(configured.contains("\"event\":\"stopped\""), "{configured}");
        assert!(server.state == State::Stopped);

        let stack = dap_call(
            &mut server,
            r#"{"seq":5,"type":"request","command":"stackTrace","arguments":{"threadId":1}}"#,
        );
        let old_frame = dap_body_id(&stack, "stackFrames", "id");
        let scopes = dap_call(
            &mut server,
            &format!(
                "{{\"seq\":6,\"type\":\"request\",\"command\":\"scopes\",\"arguments\":{{\"frameId\":{old_frame}}}}}"
            ),
        );
        let old_scope = dap_body_id(&scopes, "scopes", "variablesReference");
        let variables = dap_call(
            &mut server,
            &format!(
                "{{\"seq\":7,\"type\":\"request\",\"command\":\"variables\",\"arguments\":{{\"variablesReference\":{old_scope}}}}}"
            ),
        );
        assert!(variables.contains("\"success\":true"), "{variables}");

        let restarted = dap_call(
            &mut server,
            r#"{"seq":8,"type":"request","command":"restart","arguments":{}}"#,
        );
        assert!(restarted.contains("\"command\":\"restart\""), "{restarted}");
        assert!(restarted.contains("\"success\":true"), "{restarted}");
        assert!(restarted.contains("\"event\":\"stopped\""), "{restarted}");
        assert!(server.state == State::Stopped);

        let stale_scope = dap_call(
            &mut server,
            &format!(
                "{{\"seq\":9,\"type\":\"request\",\"command\":\"variables\",\"arguments\":{{\"variablesReference\":{old_scope}}}}}"
            ),
        );
        assert!(stale_scope.contains("\"id\":22036"), "{stale_scope}");
        assert!(stale_scope.contains("\"code\":\"E2238\""), "{stale_scope}");

        let continued = dap_call(
            &mut server,
            r#"{"seq":10,"type":"request","command":"continue","arguments":{"threadId":1}}"#,
        );
        assert!(continued.contains("\"event\":\"stopped\""), "{continued}");
        assert!(server.state == State::Stopped);
        let breakpoint_stack = dap_call(
            &mut server,
            r#"{"seq":101,"type":"request","command":"stackTrace","arguments":{"threadId":1}}"#,
        );
        assert!(
            breakpoint_stack.contains("\"line\":3"),
            "{breakpoint_stack}"
        );
        assert!(
            breakpoint_stack.contains(&json_escape(&jet_file.to_string_lossy())),
            "{breakpoint_stack}"
        );

        // Linux keeps the running executable inode busy. Rename a replacement
        // over the path so the stale-artifact check remains live without an
        // ETXTBSY overwrite failure.
        let replacement = root.join("replacement-debug-binary");
        std::fs::write(&replacement, b"replaced binary").expect("write replacement binary");
        std::fs::rename(&replacement, &binary).expect("replace debugger binary");
        let stale_binary = dap_call(
            &mut server,
            r#"{"seq":11,"type":"request","command":"restart","arguments":{}}"#,
        );
        assert!(stale_binary.contains("\"success\":false"), "{stale_binary}");
        assert!(stale_binary.contains("\"id\":22037"), "{stale_binary}");
        assert!(
            stale_binary.contains("\"code\":\"E2231\""),
            "{stale_binary}"
        );
        assert!(server.state == State::Ready);
        assert!(server.inf.is_none());
        assert_eq!(server.finish(crate::ExitCodes::OK), crate::ExitCodes::OK);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn dap_rejects_stale_scope_after_resume_generation_is_invalidated() {
        let mut server = test_server(State::Stopped);
        let frame_id = server.references.issue_frame(1, 0);
        let scopes = parse_dap_request(&format!(
            "{{\"seq\":1,\"type\":\"request\",\"command\":\"scopes\",\"arguments\":{{\"frameId\":{frame_id}}}}}"
        ))
        .unwrap();
        let mut out = Vec::new();
        server.handle(&scopes, &mut out);
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("\"success\":true"));
        assert!(text.contains("\"variablesReference\":2"));

        let continue_request = parse_dap_request(
            r#"{"seq":2,"type":"request","command":"continue","arguments":{"threadId":1}}"#,
        )
        .unwrap();
        let mut out = Vec::new();
        server.handle(&continue_request, &mut out);
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("\"success\":false"));
        server.state = State::Stopped;

        let variables = parse_dap_request(
            r#"{"seq":3,"type":"request","command":"variables","arguments":{"variablesReference":2}}"#,
        )
        .unwrap();
        let mut out = Vec::new();
        server.handle(&variables, &mut out);
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("\"success\":false"));
        assert!(text.contains("\"message\":\"jet.staleReference\""));
        assert!(text.contains("\"id\":22036"));
        assert!(text.contains("\"schema\":\"jet.diagnostic/v1\""));
        assert!(text.contains("\"code\":\"E2238\""));

        server.target = Some(TargetKind::Attached);
        let restart =
            parse_dap_request(r#"{"seq":4,"type":"request","command":"restart","arguments":{}}"#)
                .unwrap();
        let mut out = Vec::new();
        server.handle(&restart, &mut out);
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("\"success\":false"));
        assert!(text.contains("\"message\":\"jet.unsupported\""));
        assert!(text.contains("\"id\":22033"));
    }

    #[test]
    fn dap_exception_filters_and_loaded_sources_obey_lifecycle() {
        let mut server = test_server(State::New);
        let before_initialize = dap_call(
            &mut server,
            r#"{"seq":1,"type":"request","command":"setExceptionBreakpoints","arguments":{"filters":["panic"]}}"#,
        );
        assert!(before_initialize.contains("\"id\":22031"));

        let initialized = dap_call(
            &mut server,
            r#"{"seq":2,"type":"request","command":"initialize","arguments":{"adapterID":"jet"}}"#,
        );
        assert!(initialized.contains("\"success\":true"));
        let selected = dap_call(
            &mut server,
            r#"{"seq":3,"type":"request","command":"setExceptionBreakpoints","arguments":{"filters":["panic","signal"]}}"#,
        );
        assert!(selected.contains("\"success\":true"));
        assert!(server.exception_filters.contains("panic"));
        assert!(server.exception_filters.contains("signal"));
        assert!(!server.exception_filters.contains("all"));

        let invalid = dap_call(
            &mut server,
            r#"{"seq":4,"type":"request","command":"setExceptionBreakpoints","arguments":{"filters":["generated-rust"]}}"#,
        );
        assert!(invalid.contains("\"id\":22032"));

        server.state = State::Terminated;
        let after_termination = dap_call(
            &mut server,
            r#"{"seq":5,"type":"request","command":"loadedSources","arguments":{}}"#,
        );
        assert!(after_termination.contains("\"id\":22031"));
    }

    #[test]
    fn dap_unmapped_stop_terminates_with_a_jet_diagnostic() {
        let mut server = test_server(State::Running);
        let mut output = Vec::new();
        server.emit_resume(
            &mut output,
            ResumeResult::Stopped("stop reason = breakpoint\n".to_string()),
            ResumeMode::Continue,
        );
        let text = String::from_utf8(output).expect("DAP output is UTF-8");
        assert_eq!(server.state, State::Terminated);
        assert!(text.contains("E2235"));
        assert!(text.contains("\"event\":\"terminated\""));
        assert!(!text.contains("\"event\":\"stopped\""));
    }

    #[test]
    fn exited_target_releases_server_target_state() {
        let mut server = test_server(State::Running);
        server.target = Some(TargetKind::Launched);
        let mut output = Vec::new();
        server.emit_resume(
            &mut output,
            ResumeResult::Exited {
                status: Some(0),
                signal: None,
            },
            ResumeMode::Continue,
        );
        assert_eq!(server.state, State::Terminated);
        assert!(server.inf.is_none());
        assert!(server.target.is_none());
        let text = String::from_utf8(output).expect("DAP output is UTF-8");
        assert!(text.contains("\"event\":\"exited\""), "{text}");
        assert!(text.contains("\"event\":\"terminated\""), "{text}");
    }

    #[test]
    fn breakpoint_conditions_fail_closed_when_jet_value_is_unavailable() {
        let mut server = test_server(State::Stopped);
        assert!(!server.condition_matches("missing == 1"));
        assert!(!server.condition_matches("missing > 1"));
    }
}
