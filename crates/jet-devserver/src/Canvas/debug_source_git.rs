use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use jet_driver::Diagnostics::Diagnostic;
use jet_semindex::SourceSpan;

use super::schema_api::{source_revision, DEBUG_SCHEMA_VERSION};
use super::validation_json::{
    json_str, json_string_field, json_usize_field, parse_json_string, span_json,
};

const MAX_DEBUG_SESSIONS: usize = 32;
const MAX_DEBUG_COMMANDS: usize = 64;
const MAX_DEBUG_WATCHES: usize = 32;
const MAX_DEBUG_BREAKPOINTS: usize = 128;
const MAX_DEBUG_COMMAND_BYTES: usize = 256;
const MAX_DEBUG_TRACE_LINES: usize = 128;
const MAX_DEBUG_CALL_STACK: usize = 32;
const MAX_DEBUG_TRACE_BYTES: usize = 32 * 1024;
const MAX_DEBUG_LOCAL_VALUES: usize = 64;
const MAX_DEBUG_VALUE_BYTES: usize = 4 * 1024;
const MAX_DEBUG_CALL_STACK_BYTES: usize = 16 * 1024;

/// The only debugger engines a Canvas session may claim. The wire names are
/// deliberately explicit: a client must never mistake a native compiled run
/// for the `jet dev` interpreter session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DebugTier {
    Interpreter,
    NativeLldb,
}

impl DebugTier {
    pub(super) fn parse(value: Option<&str>) -> Result<Self, String> {
        match value.unwrap_or("jet-dev-interpreter") {
            "jet-dev-interpreter" => Ok(Self::Interpreter),
            "native-lldb" => Ok(Self::NativeLldb),
            _ => Err(debug_error(
                "bad_request",
                "Canvas debug tier must be `jet-dev-interpreter` or `native-lldb`",
            )),
        }
    }

    fn wire_name(self) -> &'static str {
        match self {
            Self::Interpreter => "jet-dev-interpreter",
            Self::NativeLldb => "native-lldb",
        }
    }
}

/// Live Canvas debugger ownership. A session is source- and revision-bound;
/// the server replays only its bounded command history through the same Jet
/// debugger boundary on each request. This keeps the HTTP protocol live while
/// avoiding a second interpreter or a second semantic model.
pub struct DebugSessions {
    next_id: AtomicU64,
    sessions: Mutex<HashMap<String, DebugSession>>,
    pause_on_input_end: bool,
}

struct DebugSession {
    path: PathBuf,
    revision: String,
    tier: DebugTier,
    created: u64,
    breakpoints: Vec<usize>,
    commands: Vec<String>,
    watches: Vec<String>,
    native: Option<NativeDebugArtifact>,
}

/// Compiled debugger material owned by one live session. It is deliberately
/// session-scoped: a new source revision gets a new compile, and dropping a
/// finished/stopped session removes only this private scratch directory.
struct NativeDebugArtifact {
    dir: PathBuf,
    binary: PathBuf,
    rust_file: String,
    rust_source: String,
    jet_file: String,
    jet_source: String,
}

impl Drop for NativeDebugArtifact {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

pub(crate) struct DebugExecution {
    pub(crate) id: String,
    pub(crate) status: jet_debug::SessionStatus,
    pub(crate) transcript: String,
    pub(crate) tier: DebugTier,
}

impl DebugSessions {
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            sessions: Mutex::new(HashMap::new()),
            pause_on_input_end: true,
        }
    }

    pub(crate) fn one_shot() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            sessions: Mutex::new(HashMap::new()),
            pause_on_input_end: false,
        }
    }

    pub(crate) fn execute(
        &self,
        path: &Path,
        revision: &str,
        requested_id: Option<&str>,
        commands: &[String],
        breakpoints: &[usize],
        watches: &[String],
        tier: DebugTier,
    ) -> Result<DebugExecution, String> {
        validate_debug_limits(commands, breakpoints, watches)?;
        let path = canonical_path(path);
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| debug_error("session", "debug session store is unavailable"))?;

        let id = if let Some(id) = requested_id {
            let Some(session) = sessions.get_mut(id) else {
                return Err(debug_error(
                    "session",
                    "debug session is no longer live; start a new session",
                ));
            };
            if session.path != path || session.revision != revision {
                sessions.remove(id);
                return Err(debug_error(
                    "conflict",
                    "debug session is stale for the current source revision",
                ));
            }
            if session.tier != tier {
                return Err(debug_error(
                    "conflict",
                    "debug session tier does not match the requested execution tier",
                ));
            }
            let mut next_commands = commands.to_vec();
            if next_commands.is_empty() {
                next_commands.push("s".to_string());
            }
            if session.commands.len() + next_commands.len() > MAX_DEBUG_COMMANDS {
                return Err(debug_error(
                    "limit",
                    "debug session command history is full; start a new session",
                ));
            }
            // Breakpoints and watches are editor state, not session identity.
            // Updating them keeps one live session usable while preserving the
            // same source revision and bounded replay history.
            session.breakpoints = breakpoints.to_vec();
            session.commands.extend(next_commands);
            session.watches = watches.to_vec();
            id.to_string()
        } else {
            if sessions.len() >= MAX_DEBUG_SESSIONS {
                if let Some(oldest) = sessions
                    .iter()
                    .min_by_key(|(_, session)| session.created)
                    .map(|(id, _)| id.clone())
                {
                    sessions.remove(&oldest);
                }
            }
            let serial = self.next_id.fetch_add(1, Ordering::Relaxed);
            let id = format!("canvas-debug-{serial}");
            let mut history = commands.to_vec();
            if history.is_empty() {
                history.push("s".to_string());
            }
            let native = match tier {
                DebugTier::Interpreter => None,
                DebugTier::NativeLldb => Some(NativeDebugArtifact::build(&path, &id)?),
            };
            sessions.insert(
                id.clone(),
                DebugSession {
                    path: path.clone(),
                    revision: revision.to_string(),
                    tier,
                    created: serial,
                    breakpoints: breakpoints.to_vec(),
                    commands: history,
                    watches: watches.to_vec(),
                    native,
                },
            );
            id
        };

        let session = sessions
            .get(&id)
            .ok_or_else(|| debug_error("session", "debug session disappeared"))?;
        let session_tier = session.tier;
        let mut inputs = session
            .breakpoints
            .iter()
            .map(|line| format!("break {line}"))
            .collect::<Vec<_>>();
        inputs.extend(session.commands.iter().cloned());
        inputs.push("locals".to_string());
        inputs.extend(session.watches.iter().map(|watch| format!("p {watch}")));
        inputs.push("bt".to_string());
        let refs = inputs.iter().map(String::as_str).collect::<Vec<_>>();
        let result = match session_tier {
            DebugTier::Interpreter => {
                if self.pause_on_input_end {
                    jet_debug::run_session_result_paused(&path.display().to_string(), &refs)
                } else {
                    jet_debug::run_session_result(&path.display().to_string(), &refs)
                }
            }
            DebugTier::NativeLldb => {
                let Some(native) = session.native.as_ref() else {
                    return Err(debug_error(
                        "session",
                        "native debug session lost its compiled artifact",
                    ));
                };
                if self.pause_on_input_end {
                    jet_debug::run_native_session_result_paused(
                        &native.binary,
                        &native.rust_file,
                        &native.rust_source,
                        &native.jet_file,
                        &native.jet_source,
                        false,
                        &refs,
                    )
                } else {
                    jet_debug::run_native_session_result(
                        &native.binary,
                        &native.rust_file,
                        &native.rust_source,
                        &native.jet_file,
                        &native.jet_source,
                        false,
                        &refs,
                    )
                }
            }
        };
        if result.status != jet_debug::SessionStatus::Running {
            sessions.remove(&id);
        }
        Ok(DebugExecution {
            id,
            status: result.status,
            transcript: result.transcript,
            tier: session_tier,
        })
    }

    pub(crate) fn discard(&self, id: &str) -> Option<DebugTier> {
        self.sessions
            .lock()
            .ok()
            .and_then(|mut sessions| sessions.remove(id).map(|session| session.tier))
    }

    pub(crate) fn stop(
        &self,
        path: &Path,
        revision: &str,
        current_revision: &str,
        id: &str,
        tier: DebugTier,
    ) -> Result<DebugTier, String> {
        let path = canonical_path(path);
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| debug_error("session", "debug session store is unavailable"))?;
        let Some(session) = sessions.get(id) else {
            return Err(debug_error(
                "session",
                "debug session is no longer live; nothing to stop",
            ));
        };
        if session.path != path || session.tier != tier {
            return Err(debug_error(
                "conflict",
                "debug stop does not match the live session source or tier",
            ));
        }
        if revision != current_revision {
            if session.revision != revision {
                return Err(debug_error(
                    "conflict",
                    "debug stop request is stale; the live session was kept",
                ));
            }
            sessions.remove(id);
            return Err(debug_error(
                "conflict",
                "debug session is stale for the current source revision",
            ));
        }
        if session.revision != revision {
            sessions.remove(id);
            return Err(debug_error(
                "conflict",
                "debug session is stale for the current source revision",
            ));
        }
        sessions
            .remove(id)
            .map(|session| session.tier)
            .ok_or_else(|| debug_error("session", "debug session disappeared"))
    }
}

impl Default for DebugSessions {
    fn default() -> Self {
        Self::new()
    }
}

fn validate_debug_limits(
    commands: &[String],
    breakpoints: &[usize],
    watches: &[String],
) -> Result<(), String> {
    if commands.len() > MAX_DEBUG_COMMANDS
        || breakpoints.len() > MAX_DEBUG_BREAKPOINTS
        || watches.len() > MAX_DEBUG_WATCHES
    {
        return Err(debug_error(
            "limit",
            "Canvas debug accepts at most 64 commands, 128 breakpoints, and 32 watches per session",
        ));
    }
    if commands
        .iter()
        .chain(watches.iter())
        .any(|value| value.len() > MAX_DEBUG_COMMAND_BYTES)
    {
        return Err(debug_error(
            "limit",
            "Canvas debug command and watch names are limited to 256 bytes",
        ));
    }
    for command in commands {
        validate_debug_command(command)?;
    }
    for watch in watches {
        if watch.trim().is_empty() || watch.split_whitespace().count() != 1 {
            return Err(debug_error(
                "unsupported",
                "Canvas debug watches must be one source-level name",
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_debug_breakpoint_anchors(anchors: &[String]) -> Result<(), String> {
    if anchors.len() > MAX_DEBUG_BREAKPOINTS
        || anchors
            .iter()
            .any(|anchor| anchor.len() > MAX_DEBUG_COMMAND_BYTES)
    {
        return Err(debug_error(
            "limit",
            "Canvas debug accepts at most 128 breakpoint spans of 256 bytes each",
        ));
    }
    Ok(())
}

fn validate_debug_command(command: &str) -> Result<(), String> {
    let mut parts = command.split_whitespace();
    let Some(verb) = parts.next() else {
        return Err(debug_error(
            "unsupported",
            "Canvas debug commands cannot be empty",
        ));
    };
    let arg = parts.next();
    let extra = parts.next().is_some();
    let valid = match verb {
        "step" | "s" | "next" | "n" | "continue" | "c" | "finish" | "f" | "locals"
        | "backtrace" | "bt" | "help" | "h" | "quit" | "q" | "list" | "l" => {
            arg.is_none() && !extra
        }
        "print" | "p" => arg.is_some() && !extra,
        "break" | "b" => {
            arg.and_then(|value| value.parse::<usize>().ok())
                .is_some_and(|line| line >= 1)
                && !extra
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(debug_error(
            "unsupported",
            "Canvas debug command is not in the source-level debugger vocabulary",
        ))
    }
}

impl NativeDebugArtifact {
    fn build(path: &Path, id: &str) -> Result<Self, String> {
        let jet_source = fs::read_to_string(path)
            .map_err(|error| debug_error("io", &format!("couldn't read debug source: {error}")))?;
        let output = jet_driver::run_compiler_work(|| {
            jet_driver::Driver::compile_bundle_path_opts_dbg(
                &path.display().to_string(),
                jet_driver::Sema::CompileMode::Run,
                false,
                jet_driver::Policy::GateSet::default(),
                false,
                true,
                None,
            )
        })
        .map_err(|diags| debug_diagnostics_error(path, &jet_source, &diags))?;

        let dir = std::env::temp_dir().join(format!("jet-canvas-debug-{id}"));
        fs::create_dir_all(&dir).map_err(|error| {
            debug_error(
                "io",
                &format!("couldn't prepare native Canvas debug storage: {error}"),
            )
        })?;
        let rust_file = "canvas_debug.rs".to_string();
        let rust_path = dir.join(&rust_file);
        let binary = dir.join("canvas_debug");
        if let Err(error) = fs::write(&rust_path, &output.rust) {
            let _ = fs::remove_dir_all(&dir);
            return Err(debug_error(
                "io",
                &format!("couldn't write native Canvas debug artifact: {error}"),
            ));
        }

        let mut rustc = Command::new("rustc");
        rustc
            .args([
                "--edition",
                "2021",
                "-C",
                "debuginfo=2",
                "-C",
                "opt-level=0",
            ])
            .arg("--crate-name")
            .arg("jet_canvas_debug")
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
        let compiled = rustc.output().map_err(|error| {
            debug_error(
                "diagnostic",
                &format!("native Canvas debugger is unavailable: couldn't run the native compiler ({error})"),
            )
        });
        let compiled = match compiled {
            Ok(output) if output.status.success() => output,
            Ok(_) => {
                let _ = fs::remove_dir_all(&dir);
                return Err(debug_error(
                    "diagnostic",
                    "native Canvas debugger could not build the compiled source; the Jet source was kept intact",
                ));
            }
            Err(error) => {
                let _ = fs::remove_dir_all(&dir);
                return Err(error);
            }
        };
        let _ = compiled;
        Ok(Self {
            dir,
            binary,
            rust_file,
            rust_source: output.rust,
            jet_file: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string()),
            jet_source,
        })
    }
}

pub(super) fn debug_ok(
    src: &str,
    graph_json: &str,
    transcript: &str,
    status: jet_debug::SessionStatus,
    session_id: &str,
    tier: DebugTier,
    source_id: &str,
    breakpoint_lines: &[usize],
    stale_breakpoints: &[String],
    watches: &[String],
) -> String {
    let active_line = match status {
        jet_debug::SessionStatus::Running => active_line_from_transcript(transcript),
        jet_debug::SessionStatus::Finished | jet_debug::SessionStatus::Failed => None,
    };
    let active_span = active_line
        .map(|line| line_span(src, line))
        .unwrap_or(SourceSpan { start: 0, end: 0 });
    let active_node = active_line
        .and_then(|_| record_id_for_span(graph_json, "node_id", active_span))
        .unwrap_or_default();
    let active_wire = active_line
        .and_then(|_| record_id_for_span(graph_json, "wire_id", active_span))
        .unwrap_or_default();
    let active_graph = graph_id_from_node_id(&active_node).unwrap_or_default();
    let overlay = match status {
        jet_debug::SessionStatus::Running => "running",
        jet_debug::SessionStatus::Finished | jet_debug::SessionStatus::Failed => "finished",
    };
    let revision = source_revision(src);
    let (locals, locals_truncated) = locals_json(transcript);
    let (watch_values, watches_truncated) = watches_json(transcript, watches);
    let (call_stack, call_stack_truncated) = call_stack_json(transcript);
    let (trace, trace_truncated) = trace_json(transcript);
    format!(
        "{{\"protocol\":\"jet.canvas.debug\",\"schema_version\":{},\"ok\":true,\"source_id\":{},\"revision\":{},\"session\":{{\"id\":{},\"state\":{},\"tier\":{},\"persistence\":\"local-source-span\",\"source_id\":{},\"revision\":{}}},\"overlay\":{{\"debug_overlay\":{},\"source_id\":{},\"revision\":{},\"active_line\":{},\"active_span\":{},\"active_graph_id\":{},\"active_node_id\":{},\"active_wire_id\":{},\"breakpoints\":[{}],\"locals\":[{}],\"watches\":[{}],\"call_stack\":[{}],\"trace\":[{}],\"limits\":{{\"locals_truncated\":{},\"watches_truncated\":{},\"call_stack_truncated\":{},\"trace_truncated\":{}}}}}}}",
        DEBUG_SCHEMA_VERSION,
        json_str(source_id),
        json_str(&revision),
        json_str(session_id),
        json_str(overlay),
        json_str(tier.wire_name()),
        json_str(source_id),
        json_str(&revision),
        json_str(overlay),
        json_str(source_id),
        json_str(&revision),
        active_line
            .map(|line| line.to_string())
            .unwrap_or_else(|| "null".to_string()),
        span_json(active_span),
        json_str(&active_graph),
        json_str(&active_node),
        json_str(&active_wire),
        breakpoint_json(src, breakpoint_lines, stale_breakpoints),
        locals,
        watch_values,
        call_stack,
        trace,
        if locals_truncated { "true" } else { "false" },
        if watches_truncated { "true" } else { "false" },
        if call_stack_truncated { "true" } else { "false" },
        if trace_truncated { "true" } else { "false" }
    )
}

pub(super) fn debug_error(kind: &str, message: &str) -> String {
    let message = bounded_message(message);
    format!(
        "{{\"protocol\":\"jet.canvas.debug\",\"schema_version\":{},\"ok\":false,\"kind\":{},\"message\":{}}}",
        DEBUG_SCHEMA_VERSION,
        json_str(kind),
        json_str(&message)
    )
}

pub(super) fn debug_stop_ok(
    src: &str,
    session_id: &str,
    tier: DebugTier,
    source_id: &str,
) -> String {
    let revision = source_revision(src);
    format!(
        "{{\"protocol\":\"jet.canvas.debug\",\"schema_version\":{},\"ok\":true,\"source_id\":{},\"revision\":{},\"session\":{{\"id\":{},\"state\":\"stopped\",\"tier\":{},\"persistence\":\"local-source-span\",\"source_id\":{},\"revision\":{}}},\"overlay\":null}}",
        DEBUG_SCHEMA_VERSION,
        json_str(source_id),
        json_str(&revision),
        json_str(session_id),
        json_str(tier.wire_name()),
        json_str(source_id),
        json_str(&revision)
    )
}

fn bounded_message(message: &str) -> String {
    bounded_text(message, 16 * 1024).0
}

fn bounded_text(text: &str, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text.to_string(), false);
    }
    let mut end = max_bytes.saturating_sub(" [truncated]".len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (format!("{} [truncated]", &text[..end]), true)
}

pub(super) fn debug_diagnostics_error(path: &Path, src: &str, diags: &[Diagnostic]) -> String {
    debug_error(
        "diagnostic",
        &jet_driver::Diagnostics::render_all(&path.display().to_string(), src, diags),
    )
}

pub(super) fn required_debug_string(text: &str, key: &str) -> Result<String, String> {
    json_string_field(text, key)
        .ok_or_else(|| debug_error("bad_request", &format!("missing `{key}`")))
}

fn active_line_from_transcript(transcript: &str) -> Option<usize> {
    for line in transcript.lines().rev() {
        if !line.contains("<- here") {
            continue;
        }
        let before_pipe = line.split('|').next()?.trim();
        if let Some(n) = before_pipe.split_whitespace().last() {
            if let Ok(line) = n.parse::<usize>() {
                return Some(line);
            }
        }
    }
    for line in transcript.lines().rev() {
        if let Some((_, rest)) = line.split_once("breakpoint hit") {
            if let Some((before_in, _)) = rest.split_once("  in ") {
                if let Some((_, line_no)) = before_in.trim().rsplit_once(':') {
                    if let Ok(line) = line_no.parse::<usize>() {
                        return Some(line);
                    }
                }
            }
        }
    }
    None
}

fn line_span(src: &str, line: usize) -> SourceSpan {
    let mut current = 1usize;
    let mut start = 0usize;
    for (i, ch) in src.char_indices() {
        if current == line {
            start = i;
            break;
        }
        if ch == '\n' {
            current += 1;
        }
    }
    if line > current {
        start = src.len();
    }
    let end = src[start..]
        .find('\n')
        .map(|i| start + i)
        .unwrap_or(src.len());
    SourceSpan { start, end }
}

fn line_of_offset(src: &str, offset: usize) -> usize {
    src[..offset.min(src.len())]
        .bytes()
        .filter(|b| *b == b'\n')
        .count()
        + 1
}

pub(super) fn line_from_anchor(src: &str, anchor: &str) -> Option<usize> {
    let span = anchor_span(src, anchor)?;
    Some(line_of_offset(src, span.start))
}

fn anchor_span(src: &str, anchor: &str) -> Option<SourceSpan> {
    let (start, end) = anchor.split_once(':')?;
    let start = start.parse::<usize>().ok()?;
    let end = end.parse::<usize>().ok()?;
    if start >= src.len()
        || start > end
        || end > src.len()
        || !src.is_char_boundary(start)
        || !src.is_char_boundary(end)
    {
        return None;
    }
    Some(SourceSpan { start, end })
}

fn record_id_for_span(json: &str, id_key: &str, active: SourceSpan) -> Option<String> {
    let needle = format!("\"{id_key}\":");
    let mut best: Option<(usize, String)> = None;
    for chunk in json.split(&needle).skip(1) {
        let Some((id, _)) = parse_json_string(chunk.trim_start()) else {
            continue;
        };
        let Some(pos) = chunk.find("\"source_span\"") else {
            continue;
        };
        let rest = &chunk[pos + "\"source_span\"".len()..];
        let Some(colon) = rest.find(':') else {
            continue;
        };
        let value = rest[colon + 1..].trim_start();
        if value.starts_with("null") {
            continue;
        }
        let Some(start) = json_usize_field(value, "start") else {
            continue;
        };
        let Some(end) = json_usize_field(value, "end") else {
            continue;
        };
        if span_overlaps(SourceSpan { start, end }, active) {
            let width = end.saturating_sub(start);
            if best.as_ref().map(|(w, _)| width < *w).unwrap_or(true) {
                best = Some((width, id));
            }
        }
    }
    best.map(|(_, id)| id)
}

pub(super) fn span_overlaps(a: SourceSpan, b: SourceSpan) -> bool {
    a.start <= b.end && b.start <= a.end
}

fn graph_id_from_node_id(node_id: &str) -> Option<String> {
    for marker in [":entry", ":stmt:", ":expr:"] {
        if let Some(pos) = node_id.find(marker) {
            return Some(node_id[..pos].to_string());
        }
    }
    None
}

fn breakpoint_json(src: &str, lines: &[usize], stale_anchors: &[String]) -> String {
    let mut entries = lines
        .iter()
        .map(|line| {
            let valid = *line > 0 && *line <= src.lines().count();
            let span = valid.then(|| line_span(src, *line));
            format!(
                "{{\"line\":{},\"source_span\":{},\"state\":{}}}",
                line,
                span.map(span_json).unwrap_or_else(|| "null".to_string()),
                json_str(if valid { "valid" } else { "stale" })
            )
        })
        .collect::<Vec<_>>();
    entries.extend(stale_anchors.iter().map(|anchor| {
        let span = anchor_span(src, anchor);
        format!(
            "{{\"line\":null,\"source_span\":{},\"anchor\":{},\"state\":\"stale\"}}",
            span.map(span_json).unwrap_or_else(|| "null".to_string()),
            json_str(anchor)
        )
    }));
    entries.join(",")
}

fn locals_json(transcript: &str) -> (String, bool) {
    let Some(line) = transcript
        .lines()
        .rev()
        .find(|line| line.starts_with("locals:"))
    else {
        return (String::new(), false);
    };
    parse_assignments(line.trim_start_matches("locals:").trim())
}

fn watches_json(transcript: &str, watches: &[String]) -> (String, bool) {
    let mut truncated = false;
    let values = watches
        .iter()
        .filter_map(|watch| {
            let prefix = format!("{watch} = ");
            transcript
                .lines()
                .rev()
                .find_map(|line| line.strip_prefix(&prefix))
                .map(|value| {
                    let (value, value_truncated) = bounded_text(value, MAX_DEBUG_VALUE_BYTES);
                    truncated |= value_truncated;
                    format!(
                        "{{\"name\":{},\"value\":{},\"state\":\"ok\"}}",
                        json_str(watch),
                        json_str(&value)
                    )
                })
        })
        .collect::<Vec<_>>()
        .join(",");
    (values, truncated)
}

fn parse_assignments(text: &str) -> (String, bool) {
    if text == "(none)" || text.is_empty() {
        return (String::new(), false);
    }
    let mut truncated = false;
    let values = text
        .split("   ")
        .enumerate()
        .filter_map(|(index, part)| {
            if index >= MAX_DEBUG_LOCAL_VALUES {
                truncated = true;
                return None;
            }
            let (name, value) = part.split_once(" = ")?;
            let (value, value_truncated) = bounded_text(value.trim(), MAX_DEBUG_VALUE_BYTES);
            truncated |= value_truncated;
            Some(format!(
                "{{\"name\":{},\"value\":{}}}",
                json_str(name.trim()),
                json_str(&value)
            ))
        })
        .collect::<Vec<_>>()
        .join(",");
    (values, truncated)
}

fn call_stack_json(transcript: &str) -> (String, bool) {
    let mut entries = Vec::new();
    let mut bytes = 0;
    let mut truncated = false;
    let mut lines = VecDeque::with_capacity(MAX_DEBUG_CALL_STACK);
    for line in transcript
        .lines()
        .filter(|line| line.starts_with('#') && line.contains(" at "))
    {
        if lines.len() >= MAX_DEBUG_CALL_STACK {
            lines.pop_front();
            truncated = true;
        }
        lines.push_back(line);
    }
    for line in lines.iter().rev() {
        if entries.len() >= MAX_DEBUG_CALL_STACK {
            truncated = true;
            break;
        }
        let entry = json_str(line);
        if bytes + entry.len() > MAX_DEBUG_CALL_STACK_BYTES {
            truncated = true;
            break;
        }
        bytes += entry.len();
        entries.push(entry);
    }
    entries.reverse();
    (entries.join(","), truncated)
}

fn trace_json(transcript: &str) -> (String, bool) {
    let mut entries = Vec::new();
    let mut bytes = 0usize;
    let mut truncated = false;
    for (index, line) in transcript.lines().rev().enumerate() {
        if index >= MAX_DEBUG_TRACE_LINES {
            truncated = true;
            break;
        }
        let entry = json_str(line);
        if bytes + entry.len() > MAX_DEBUG_TRACE_BYTES {
            truncated = true;
            break;
        }
        bytes += entry.len();
        entries.push(entry);
    }
    entries.reverse();
    (entries.join(","), truncated)
}

pub(super) fn canonical_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

pub(super) fn git_root(path: &Path) -> Option<PathBuf> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let out = Command::new("git")
        .args(["-C"])
        .arg(dir)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(PathBuf::from(text))
    }
}

pub(super) fn git_relative_path(root: &Path, path: &Path) -> String {
    let abs = canonical_path(path);
    let root = canonical_path(root);
    abs.strip_prefix(&root)
        .unwrap_or(&abs)
        .to_string_lossy()
        .replace('\\', "/")
}

pub(super) fn git_output(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        None
    }
}

pub(super) fn untracked_diff(rel: &str, src: &str) -> String {
    let mut diff = format!("--- /dev/null\n+++ b/{rel}\n");
    for line in src.lines() {
        diff.push('+');
        diff.push_str(line);
        diff.push('\n');
    }
    diff
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_state_ignores_spoofed_completion_output() {
        let src = "fn run() {\n    print(\"program finished\")\n}\n";
        let active = line_span(src, 2);
        let graph = format!(
            "{{\"node_id\":\"fn:main.jet::run@3-6:expr:1:call:print\",\"source_span\":{},\"wire_id\":\"fn:main.jet::run@3-6:wire:1\",\"source_span\":{}}}",
            span_json(active),
            span_json(active),
        );
        let transcript = "breakpoint hit  main.jet:2  in main()\n   2 |     print(\"program finished\")        <- here\nprogram finished\n";

        let out = debug_ok(
            src,
            &graph,
            transcript,
            jet_debug::SessionStatus::Running,
            "test-session",
            DebugTier::Interpreter,
            "main.jet",
            &[],
            &[],
            &[],
        );

        assert!(out.contains("\"state\":\"running\""), "{out}");
        assert!(out.contains("\"debug_overlay\":\"running\""), "{out}");
        assert!(out.contains("\"active_line\":2"), "{out}");
        assert!(
            out.contains("\"active_graph_id\":\"fn:main.jet::run@3-6\""),
            "{out}"
        );
        assert!(
            out.contains("\"active_node_id\":\"fn:main.jet::run@3-6:expr:1:call:print\""),
            "{out}"
        );
        assert!(
            out.contains("\"active_wire_id\":\"fn:main.jet::run@3-6:wire:1\""),
            "{out}"
        );
    }

    #[test]
    fn overlay_marks_bounded_debug_values() {
        let src = "fn run() {\n    print(\"ok\")\n}\n";
        let huge = "x".repeat(MAX_DEBUG_VALUE_BYTES + 128);
        let out = debug_ok(
            src,
            "{}",
            &format!("locals: value = {huge}\nvalue = {huge}"),
            jet_debug::SessionStatus::Finished,
            "test-session",
            DebugTier::Interpreter,
            "main.jet",
            &[],
            &[],
            &["value".to_string()],
        );
        assert!(out.contains("\"locals_truncated\":true"), "{out}");
        assert!(out.contains("\"watches_truncated\":true"), "{out}");
        assert!(
            out.len() < 20 * 1024,
            "debug overlay escaped its bound: {}",
            out.len()
        );
    }
}
