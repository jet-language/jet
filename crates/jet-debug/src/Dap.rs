//! D-DBG3 step 2 (dap-debugger): the Debug Adapter Protocol server — the
//! "editor wiring" half of the native backend. Same [`super::Inferior`]/
//! [`super::LineMap`] the terminal `(jet)` session (`Native.rs`) uses; this
//! module only adds the DAP wire format (Content-Length framed JSON on stdio,
//! the same convention `Source/LSP/Server.rs` already speaks) so a trusted
//! editor such as VS Code can launch `jet debug --dap <file>` as a debug
//! adapter. Zed registration remains disabled until its trust/authorization
//! API can report the required authority state.
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

use super::Inferior::{Inferior, ResumeResult};
use super::LineMap::LineMap;
use jet_foundation::JSON::{
    json_escape, json_get, json_str, json_u32, parse_json, JSONValue, MAX_PROTOCOL_HEADER_BYTES,
    MAX_PROTOCOL_HEADER_COUNT, MAX_PROTOCOL_MESSAGE_BYTES,
};

#[derive(Clone, Copy, PartialEq, Eq)]
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
        entry_breakpoint_id: None,
        stop_on_entry: true,
        target: None,
        launch_args: Vec::new(),
        launch_cwd: None,
        launch_env: Vec::new(),
        current_thread: 1,
        last_signal: None,
        show_raw_frames: false,
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
    entry_breakpoint_id: Option<usize>,
    stop_on_entry: bool,
    target: Option<TargetKind>,
    launch_args: Vec<String>,
    launch_cwd: Option<String>,
    launch_env: Vec<(String, String)>,
    current_thread: u32,
    last_signal: Option<String>,
    show_raw_frames: bool,
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

#[derive(Clone, Debug, Default)]
struct BreakpointSpec {
    line: usize,
    condition: Option<String>,
    hit_condition: Option<String>,
    log_message: Option<String>,
}

enum BreakpointAction {
    Stop,
    Continue,
    Log(String),
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
    Unavailable,
}

impl AttachFailure {
    fn error(self) -> (i64, &'static str) {
        match self {
            Self::InvalidArguments => (22032, "debug attach arguments are invalid"),
            Self::MapMismatch => (22037, "debugger map does not match the attach target"),
            Self::Unavailable => (22034, "the local attach target is unavailable"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReferenceKind {
    Frame,
    Scope,
    RawScope,
}

/// DAP object ids are handles into one stopped snapshot.  The generation is
/// deliberately kept server-side: an editor can retain an old integer, but
/// it can never make that integer live again after execution resumes.
struct ObjectReferences {
    generation: u64,
    next_id: u32,
    live: HashMap<u32, (u64, ReferenceKind)>,
    frame_ids: HashMap<usize, u32>,
    frame_positions: HashMap<u32, usize>,
    scope_positions: HashMap<u32, usize>,
}

impl Default for ObjectReferences {
    fn default() -> Self {
        Self {
            generation: 1,
            next_id: 1,
            live: HashMap::new(),
            frame_ids: HashMap::new(),
            frame_positions: HashMap::new(),
            scope_positions: HashMap::new(),
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

    fn issue_frame(&mut self, position: usize) -> u32 {
        if let Some(id) = self.frame_ids.get(&position) {
            return *id;
        }
        let id = self.issue(ReferenceKind::Frame);
        self.frame_ids.insert(position, id);
        self.frame_positions.insert(id, position);
        id
    }

    fn issue_raw_frame(&mut self, position: usize) -> u32 {
        let id = self.issue(ReferenceKind::Frame);
        self.frame_positions.insert(id, position);
        id
    }

    fn issue_scope(&mut self, frame_id: u32, kind: ReferenceKind) -> u32 {
        let id = self.issue(kind);
        if let Some(position) = self.frame_positions.get(&frame_id) {
            self.scope_positions.insert(id, *position);
        }
        id
    }

    fn frame_position(&self, id: u32) -> Option<usize> {
        self.frame_positions.get(&id).copied()
    }

    fn scope_position(&self, id: u32) -> Option<usize> {
        self.scope_positions.get(&id).copied()
    }

    fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1).max(1);
        self.live.clear();
        self.frame_ids.clear();
        self.frame_positions.clear();
        self.scope_positions.clear();
    }
}

impl DapServer {
    fn finish(&mut self, code: i32) -> i32 {
        self.state = State::Terminated;
        self.invalidate_references();
        let terminate = self.target == Some(TargetKind::Launched);
        if self.close_target(terminate).is_err() {
            crate::ExitCodes::USER_ERROR
        } else {
            code
        }
    }

    fn close_target(&mut self, terminate: bool) -> io::Result<()> {
        let Some(mut inf) = self.inf.take() else {
            return Ok(());
        };
        let _entry_breakpoint = self.entry_breakpoint_id.take();
        let action = if terminate {
            if self.target == Some(TargetKind::Attached) {
                inf.terminate_debuggee()
            } else {
                Ok(())
            }
        } else {
            // Both an attached target and an explicitly non-terminating
            // launched target leave the debuggee running after disconnect.
            inf.detach()
        };
        inf.quit();
        action
    }

    fn source_line(&self, line: usize) -> &str {
        self.jet_src
            .lines()
            .nth(line.saturating_sub(1))
            .unwrap_or("")
    }

    fn spawn_target(&self) -> io::Result<(Inferior, Option<usize>)> {
        let mut inf = Inferior::spawn(&self.binary)?;
        let setup = if self.stop_on_entry {
            self.map
                .main_entry_line(&self.rust_src)
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "entry has no source line")
                })
                .and_then(|entry_line| inf.set_breakpoint(&self.rust_file, entry_line))
                .map(Some)
        } else {
            Ok(None)
        };
        match setup {
            Ok(entry) => Ok((inf, entry.map(|breakpoint| breakpoint.id))),
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
        let inferior = Inferior::attach(&attach.program, attach.pid)
            .map_err(|_| AttachFailure::Unavailable)?;
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
        self.start_target(out);
    }

    fn start_target(&mut self, out: &mut impl Write) {
        self.state = State::Running;
        let result = self.inf.as_mut().map(|inf| inf.resume_and_locate("run"));
        match result {
            Some(Ok(result)) => self.emit_resume(out, result),
            Some(Err(_error)) => {
                self.state = State::Terminated;
                let body = format!(
                    "{{\"category\":\"stderr\",\"output\":\"{}\\n\"}}",
                    "the native debugger could not start the program"
                );
                self.event(out, "output", &body);
                self.event(out, "terminated", "{}");
            }
            None => {
                self.state = State::Terminated;
                self.event(out, "terminated", "{}");
            }
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
                self.state = State::Ready;
                self.respond(
                    out,
                    request_seq,
                    "initialize",
                    true,
                    "{\"supportsConfigurationDoneRequest\":true,\"supportsTerminateRequest\":true,\"supportsPauseRequest\":true,\"supportsRestartRequest\":true,\"supportsConditionalBreakpoints\":true,\"supportsHitConditionalBreakpoints\":true,\"supportsLogPoints\":true,\"supportsEvaluateForHovers\":true,\"supportsExceptionInfoRequest\":true,\"supportsLoadedSourcesRequest\":true}",
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
                let target = if command == "launch" {
                    let launch = match parse_launch_arguments(args, &self.jet_file) {
                        Ok(launch) => launch,
                        Err(message) => {
                            self.respond_jet_error(out, request_seq, command, 22032, message);
                            return Some(());
                        }
                    };
                    if let Some(value) = args.and_then(|args| json_get(args, "stopOnEntry")) {
                        let Some(value) = json_bool(value) else {
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
                            self.respond_jet_error(out, request_seq, command, id, format);
                            return Some(());
                        }
                    }
                };
                match target {
                    Ok((inf, entry_id)) => {
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
                    Err(_error) => self.respond_jet_error(
                        out,
                        request_seq,
                        command,
                        22034,
                        "the native debugger could not create the requested target",
                    ),
                }
                Some(())
            }
            "restart" => {
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
                let lines = match breakpoint_lines(args) {
                    Ok(lines) => lines,
                    Err(message) => {
                        self.respond_err(out, request_seq, command, message);
                        return Some(());
                    }
                };
                let specs = match breakpoint_specs(args) {
                    Ok(specs) => specs,
                    Err(message) => {
                        self.respond_err(out, request_seq, command, message);
                        return Some(());
                    }
                };
                self.pending_breakpoints = lines.clone();
                self.pending_specs = specs;
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
                    .zip(statuses)
                    .map(|(line, verified)| {
                        if verified {
                            format!("{{\"verified\":true,\"line\":{line}}}")
                        } else {
                            format!(
                                "{{\"verified\":false,\"line\":{},\"message\":\"no stoppable Jet statement at {}:{}\"}}",
                                line,
                                json_escape(&self.jet_file),
                                line
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
                    Some(Err(_)) => {
                        self.respond_jet_error(
                            out,
                            request_seq,
                            command,
                            22034,
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
                        format!(
                            "{{\"id\":{},\"name\":\"Jet task {}\"}}",
                            thread.id, thread.id
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
                let thread_id = args
                    .and_then(|args| json_get(args, "threadId"))
                    .and_then(json_u32)
                    .unwrap_or(self.current_thread);
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
                let show_raw_frames = match args.and_then(|args| json_get(args, "showRawFrames")) {
                    None => self.show_raw_frames,
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
                let frames = match self.inf.as_mut().map(Inferior::backtrace) {
                    Some(Ok(out_text)) => Inferior::parse_frames(&out_text),
                    Some(Err(_)) => {
                        self.respond_jet_error(
                            out,
                            request_seq,
                            command,
                            22034,
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
                let total_frames = frames.len();
                let mut entries = Vec::new();
                for (position, frame) in frames
                    .iter()
                    .enumerate()
                    .skip(start_frame)
                    .take(levels.unwrap_or(usize::MAX))
                {
                    if let Some(jline) = self.map.jet_line_for(frame.rust_line) {
                        let id = self.references.issue_frame(position);
                        entries.push(format!(
                            "{{\"id\":{},\"name\":\"{}\",\"source\":{{\"path\":\"{}\"}},\"line\":{},\"column\":1}}",
                            id,
                            json_escape(&Inferior::safe_jet_func(&frame.func)),
                            json_escape(&self.jet_file),
                            jline
                        ));
                    }
                    if show_raw_frames {
                        let id = self.references.issue_raw_frame(position);
                        entries.push(format!(
                            "{{\"id\":{},\"name\":\"[raw] {}\",\"source\":{{\"path\":\"{}\"}},\"line\":{},\"column\":1}}",
                            id,
                            json_escape(&frame.func),
                            json_escape(&self.rust_file),
                            frame.rust_line
                        ));
                    }
                }
                let body = format!(
                    "{{\"stackFrames\":[{}],\"totalFrames\":{}}}",
                    entries.join(","),
                    total_frames
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
                let Some(_position) = self.references.frame_position(frame_id) else {
                    self.respond_stale_reference(out, request_seq, command);
                    return Some(());
                };
                let show_raw_frames = match args.and_then(|args| json_get(args, "showRawFrames")) {
                    None => self.show_raw_frames,
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
                let kind = if show_raw_frames {
                    ReferenceKind::RawScope
                } else {
                    ReferenceKind::Scope
                };
                let scope_id = self.references.issue_scope(frame_id, kind);
                let scope_name = if show_raw_frames {
                    "[raw] Locals"
                } else {
                    "Locals"
                };
                let body = format!(
                    "{{\"scopes\":[{{\"name\":\"{}\",\"variablesReference\":{},\"expensive\":false}}]}}",
                    json_escape(scope_name),
                    scope_id
                );
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
                let raw_scope = if self.references.is_live(scope_id, ReferenceKind::RawScope) {
                    true
                } else if self.references.is_live(scope_id, ReferenceKind::Scope) {
                    false
                } else {
                    self.respond_stale_reference(out, request_seq, command);
                    return Some(());
                };
                let Some(frame_position) = self.references.scope_position(scope_id) else {
                    self.respond_stale_reference(out, request_seq, command);
                    return Some(());
                };
                if let Some(inf) = self.inf.as_mut() {
                    if inf.select_frame(frame_position).is_err() {
                        self.respond_stale_reference(out, request_seq, command);
                        return Some(());
                    }
                }
                let locals = match self.inf.as_mut().map(Inferior::locals) {
                    Some(Ok(output)) => Inferior::parse_typed_locals(&output),
                    Some(Err(_)) => {
                        self.respond_jet_error(
                            out,
                            request_seq,
                            command,
                            22034,
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
                let entries: Vec<String> = locals
                    .iter()
                    .filter_map(|(ty, name, value)| {
                        if raw_scope {
                            return Some(format!(
                                "{{\"name\":\"{}\",\"value\":\"{}\",\"type\":\"{}\",\"variablesReference\":0}}",
                                json_escape(name),
                                json_escape(value),
                                json_escape(ty)
                            ));
                        }
                        if !Inferior::rust_local_is_jet_visible(name) {
                            return None;
                        }
                        let jet_name = Inferior::rust_local_to_jet(name)?;
                        let safe_value = Inferior::safe_value(ty, value);
                        let type_json = Inferior::jet_type_name(ty)
                            .map(|ty| format!(",\"type\":\"{}\"", json_escape(ty)))
                            .unwrap_or_default();
                        Some(format!(
                            "{{\"name\":\"{}\",\"value\":\"{}\"{},\"variablesReference\":0}}",
                            json_escape(&jet_name),
                            json_escape(&safe_value),
                            type_json
                        ))
                    })
                    .collect();
                let body = format!("{{\"variables\":[{}]}}", entries.join(","));
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
                    let Some(position) = self.references.frame_position(frame_id) else {
                        self.respond_stale_reference(out, request_seq, command);
                        return Some(());
                    };
                    if let Some(inf) = self.inf.as_mut() {
                        if inf.select_frame(position).is_err() {
                            self.respond_stale_reference(out, request_seq, command);
                            return Some(());
                        }
                    }
                }
                let rust_expression = format!("{}{}", Inferior::jet_local_to_rust(&root), suffix);
                let output = match self
                    .inf
                    .as_mut()
                    .map(|inf| inf.frame_variable(&rust_expression))
                {
                    Some(Ok(output)) => output,
                    Some(Err(_)) | None => {
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
                self.invalidate_references();
                let resume_cmd = match command {
                    "continue" => "continue",
                    "next" => "thread step-over",
                    "stepIn" => "thread step-in",
                    _ => "thread step-out",
                };
                self.state = State::Running;
                let result = self
                    .inf
                    .as_mut()
                    .map(|inf| inf.resume_and_locate(resume_cmd));
                match result {
                    Some(Ok(result)) => {
                        self.respond(
                            out,
                            request_seq,
                            command,
                            true,
                            "{\"allThreadsContinued\":true}",
                        );
                        self.emit_resume(out, result);
                    }
                    Some(Err(_error)) => {
                        self.state = State::Terminated;
                        self.respond_err(
                            out,
                            request_seq,
                            command,
                            "the native debugger lost the running session",
                        );
                        let body = format!(
                            "{{\"category\":\"stderr\",\"output\":\"{}\\n\"}}",
                            "the native debugger lost the running session"
                        );
                        self.event(out, "output", &body);
                        self.event(out, "terminated", "{}");
                    }
                    None => {
                        self.state = State::Terminated;
                        self.respond_err(out, request_seq, command, "Jet debugger has no target");
                        self.event(out, "terminated", "{}");
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
                self.invalidate_references();
                let result = self.inf.as_mut().map(Inferior::interrupt);
                match result {
                    Some(Ok(bt_text)) => {
                        self.respond(out, request_seq, command, true, "{}");
                        self.emit_resume(out, ResumeResult::Stopped(bt_text));
                    }
                    Some(Err(_error)) => {
                        self.state = State::Terminated;
                        self.respond_err(
                            out,
                            request_seq,
                            command,
                            "the native debugger could not pause the program",
                        );
                        self.event(out, "terminated", "{}");
                    }
                    None => {
                        self.state = State::Terminated;
                        self.respond_err(out, request_seq, command, "Jet debugger has no target");
                        self.event(out, "terminated", "{}");
                    }
                }
                Some(())
            }
            "setExceptionBreakpoints" => {
                let valid = args
                    .and_then(|args| json_get(args, "filters"))
                    .map(|filters| match filters {
                        JSONValue::Array(values) => values.iter().all(|value| {
                            matches!(json_str(value), Some("all" | "error" | "panic" | "signal"))
                        }),
                        _ => false,
                    })
                    .unwrap_or(true);
                if valid {
                    self.respond(out, request_seq, command, true, "{}");
                } else {
                    self.respond_jet_error(
                        out,
                        request_seq,
                        command,
                        22032,
                        "exception filters must be Jet panic, error, signal, or all",
                    );
                }
                Some(())
            }
            "exceptionInfo" => {
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
                self.respond(
                    out,
                    request_seq,
                    command,
                    true,
                    &format!(
                        "{{\"sources\":[{{\"name\":\"{}\",\"path\":\"{}\"}}]}}",
                        json_escape(&self.jet_file),
                        json_escape(&self.jet_file)
                    ),
                );
                Some(())
            }
            "cancel" => {
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
                    Ok(()) => self.respond(out, request_seq, command, true, "{}"),
                    Err(_error) => self.respond_jet_error(
                        out,
                        request_seq,
                        command,
                        22034,
                        "the debugger could not cleanly close the target",
                    ),
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
            self.state = State::Terminated;
            self.event(
                out,
                "output",
                "{\"category\":\"stderr\",\"output\":\"Jet debugger has no target.\\n\"}",
            );
            self.event(out, "terminated", "{}");
            return;
        };
        for id in old {
            if let Err(error) = inf.delete_breakpoint(id) {
                // The DAP body stays a fixed string: this file has no JSON
                // escaper, and an arbitrary error text would break the frame.
                eprintln!("error: native debugger could not remove a source breakpoint: {error}");
                self.state = State::Terminated;
                let body = format!(
                    "{{\"category\":\"stderr\",\"output\":\"{}\\n\"}}",
                    "the native debugger could not remove a source breakpoint"
                );
                self.event(out, "output", &body);
                self.event(out, "terminated", "{}");
                return;
            }
        }
        let (ids, _) = match Self::install_breakpoints(map, &rust_file, &lines, inf) {
            Ok(result) => result,
            Err(_error) => {
                self.state = State::Terminated;
                let body = format!(
                    "{{\"category\":\"stderr\",\"output\":\"{}\\n\"}}",
                    "the native debugger could not install source breakpoints"
                );
                self.event(out, "output", &body);
                self.event(out, "terminated", "{}");
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
            Some(Ok(result)) => self.emit_resume(out, result),
            Some(Err(_error)) => {
                self.state = State::Terminated;
                let body = format!(
                    "{{\"category\":\"stderr\",\"output\":\"{}\\n\"}}",
                    "the native debugger lost the running session"
                );
                self.event(out, "output", &body);
                self.event(out, "terminated", "{}");
            }
            None => {
                self.state = State::Terminated;
                self.event(out, "terminated", "{}");
            }
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
                json_escape(&stdout)
            );
            self.event(out, "output", &body);
        }
        if !stderr.is_empty() {
            let body = format!(
                "{{\"category\":\"stderr\",\"output\":\"{}\"}}",
                json_escape(&stderr)
            );
            self.event(out, "output", &body);
        }
    }

    fn emit_resume(&mut self, out: &mut impl Write, result: ResumeResult) {
        self.emit_resume_inner(out, result, 0);
    }

    fn emit_resume_inner(&mut self, out: &mut impl Write, result: ResumeResult, depth: usize) {
        if depth >= 200 {
            self.state = State::Terminated;
            self.invalidate_references();
            self.event(out, "terminated", "{}");
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
                    (None, Some(signal)) => format!("{{\"signal\":\"{}\"}}", json_escape(&signal)),
                    (None, None) => "{}".to_string(),
                };
                self.event(out, "exited", &body);
                self.event(out, "terminated", "{}");
                return;
            }
            ResumeResult::Stopped(text) => text,
        };
        self.state = State::Stopped;
        let reason = if bt_text.contains("signal") || bt_text.contains("exception") {
            "exception"
        } else {
            "breakpoint"
        };
        if let Some(frame) = Inferior::parse_top_frame(&bt_text) {
            match self.map.jet_line_for(frame.rust_line) {
                None if !self.show_raw_frames => {
                    if self.unmapped_steps >= 200 {
                        self.unmapped_steps = 0;
                        self.state = State::Stopped;
                        self.event(
                            out,
                            "stopped",
                            "{\"reason\":\"step\",\"threadId\":1,\"allThreadsStopped\":true}",
                        );
                        return;
                    }
                    self.unmapped_steps += 1;
                    self.state = State::Running;
                    let next = self
                        .inf
                        .as_mut()
                        .and_then(|inf| inf.resume_and_locate("thread step-over").ok());
                    if let Some(next) = next {
                        self.emit_resume_inner(out, next, depth + 1);
                    } else {
                        self.state = State::Terminated;
                        self.event(out, "terminated", "{}");
                    }
                    return;
                }
                Some(jline) => {
                    self.unmapped_steps = 0;
                    match self.breakpoint_action(jline) {
                        BreakpointAction::Stop => {}
                        BreakpointAction::Continue => {
                            self.state = State::Running;
                            let next = self
                                .inf
                                .as_mut()
                                .and_then(|inf| inf.resume_and_locate("continue").ok());
                            if let Some(next) = next {
                                self.emit_resume_inner(out, next, depth + 1);
                            } else {
                                self.state = State::Terminated;
                                self.event(out, "terminated", "{}");
                            }
                            return;
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
                            self.state = State::Running;
                            let next = self
                                .inf
                                .as_mut()
                                .and_then(|inf| inf.resume_and_locate("continue").ok());
                            if let Some(next) = next {
                                self.emit_resume_inner(out, next, depth + 1);
                            } else {
                                self.state = State::Terminated;
                                self.event(out, "terminated", "{}");
                            }
                            return;
                        }
                    }
                }
                None => {
                    self.unmapped_steps = 0;
                }
            }
        }
        match Inferior::parse_top_frame(&bt_text) {
            Some(frame) => {
                let _ = self.source_line(self.map.jet_line_for(frame.rust_line).unwrap_or(1));
                let body = format!(
                    "{{\"reason\":\"{}\",\"threadId\":1,\"allThreadsStopped\":true}}",
                    reason
                );
                self.event(out, "stopped", &body);
            }
            None => {
                // No parseable frame can still be a signal/exception stop, but
                // never forward the backend transcript as a Jet frame.
                let body = format!(
                    "{{\"reason\":\"{}\",\"threadId\":1,\"allThreadsStopped\":true}}",
                    reason
                );
                self.event(out, "stopped", &body);
            }
        }
    }

    fn breakpoint_action(&mut self, line: usize) -> BreakpointAction {
        if self.pending_specs.is_empty() {
            return BreakpointAction::Stop;
        }
        let mut log = None;
        let mut matched = false;
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
                return BreakpointAction::Stop;
            }
        }
        if let Some(message) = log {
            BreakpointAction::Log(message)
        } else if matched {
            BreakpointAction::Continue
        } else {
            BreakpointAction::Stop
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
            return true;
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
                _ => true,
            };
        }
        match operator {
            "==" => value == right,
            "!=" => value != right,
            _ => true,
        }
    }

    fn read_jet_value(&mut self, expression: &str) -> Option<String> {
        let (root, suffix) = jet_expression(expression)?;
        let rust_expression = format!("{}{}", Inferior::jet_local_to_rust(&root), suffix);
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

    fn next_seq(&mut self) -> i64 {
        let s = self.seq;
        self.seq += 1;
        s
    }
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
    let message = parse_json(body).map_err(|()| "invalid JSON")?;
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
        None => None,
        Some(value) => Some(
            json_str(value)
                .ok_or("launch cwd must be a string")?
                .to_string(),
        ),
    };
    let env = match args.and_then(|args| json_get(args, "env")) {
        None => Vec::new(),
        Some(JSONValue::Object(values)) => values
            .iter()
            .map(|(key, value)| {
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
        _ => left_path.file_name() == right_path.file_name(),
    }
}

fn breakpoint_lines(args: Option<&JSONValue>) -> Result<Vec<usize>, &'static str> {
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
                .filter(|line| *line > 0)
                .ok_or("breakpoint line must be a positive integer")?;
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

fn breakpoint_specs(args: Option<&JSONValue>) -> Result<Vec<BreakpointSpec>, &'static str> {
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
                .filter(|line| *line > 0)
                .and_then(|line| usize::try_from(line).ok())
                .ok_or("breakpoint line must be a positive integer")?;
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
        .collect()
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
    let count = i64::from(count);
    for operator in [">=", "<=", "==", ">", "<"] {
        if let Some(value) = condition.strip_prefix(operator) {
            let Ok(value) = value.trim().parse::<i64>() else {
                return false;
            };
            return match operator {
                ">=" => count >= value,
                "<=" => count <= value,
                "==" => count == value,
                ">" => count > value,
                "<" => count < value,
                _ => false,
            };
        }
    }
    condition
        .parse::<i64>()
        .map(|value| count == value)
        .unwrap_or(false)
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
        if let Some(value) = line.strip_prefix("Content-Length:") {
            if content_length.is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "protocol frame has duplicate Content-Length headers",
                ));
            }
            content_length = Some(value.trim().parse::<usize>().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "protocol Content-Length is invalid",
                )
            })?);
        }
    };
    if len > MAX_PROTOCOL_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "protocol message exceeds the 1048576-byte limit",
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
            entry_breakpoint_id: None,
            stop_on_entry: true,
            target: None,
            launch_args: Vec::new(),
            launch_cwd: None,
            launch_env: Vec::new(),
            current_thread: 1,
            last_signal: None,
            show_raw_frames: false,
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
            r#"{"seq":1,"type":"request","command":"initialize","arguments":{}}"#
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
        let frame = refs.issue_frame(0);
        let scope = refs.issue_scope(frame, ReferenceKind::RawScope);
        assert!(refs.is_live(scope, ReferenceKind::RawScope));
        assert_eq!(refs.scope_position(scope), Some(0));
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
            frame(r#"{"seq":1,"type":"request","command":"initialize","arguments":{}}"#),
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
    fn dap_rejects_oversized_frame_before_reading_a_body() {
        let frame = format!("Content-Length: {}\r\n\r\n", MAX_PROTOCOL_MESSAGE_BYTES + 1);
        let error = read_message(&mut std::io::Cursor::new(frame)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            error.to_string(),
            "protocol message exceeds the 1048576-byte limit"
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

        let stable = refs.issue_frame(0);
        assert_eq!(stable, refs.issue_frame(0));
    }

    #[test]
    fn dap_rejects_stale_scope_after_resume_generation_is_invalidated() {
        let mut server = test_server(State::Stopped);
        let frame_id = server.references.issue_frame(0);
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
}
