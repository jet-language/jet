use std::collections::HashMap;
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

/// Live Canvas debugger ownership. A session is source- and revision-bound;
/// the server replays only its bounded command history through the same Jet
/// debugger boundary on each request. This keeps the HTTP protocol live while
/// avoiding a second interpreter or a second semantic model.
pub struct DebugSessions {
    next_id: AtomicU64,
    sessions: Mutex<HashMap<String, DebugSession>>,
}

struct DebugSession {
    path: PathBuf,
    revision: String,
    breakpoints: Vec<usize>,
    commands: Vec<String>,
    watches: Vec<String>,
}

pub(crate) struct DebugExecution {
    pub(crate) id: String,
    pub(crate) status: jet_debug::SessionStatus,
    pub(crate) transcript: String,
}

impl DebugSessions {
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            sessions: Mutex::new(HashMap::new()),
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
            if session.breakpoints != breakpoints {
                sessions.remove(id);
                return Err(debug_error(
                    "session",
                    "breakpoints changed; start a new debug session",
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
            session.commands.extend(next_commands);
            session.watches = watches.to_vec();
            id.to_string()
        } else {
            if sessions.len() >= MAX_DEBUG_SESSIONS {
                if let Some(oldest) = sessions.keys().next().cloned() {
                    sessions.remove(&oldest);
                }
            }
            let id = format!(
                "canvas-debug-{}",
                self.next_id.fetch_add(1, Ordering::Relaxed)
            );
            let mut history = commands.to_vec();
            if history.is_empty() {
                history.push("s".to_string());
            }
            sessions.insert(
                id.clone(),
                DebugSession {
                    path: path.clone(),
                    revision: revision.to_string(),
                    breakpoints: breakpoints.to_vec(),
                    commands: history,
                    watches: watches.to_vec(),
                },
            );
            id
        };

        let session = sessions
            .get(&id)
            .ok_or_else(|| debug_error("session", "debug session disappeared"))?;
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
        let result = jet_debug::run_session_result_paused(&path.display().to_string(), &refs);
        if result.status != jet_debug::SessionStatus::Running {
            sessions.remove(&id);
        }
        Ok(DebugExecution {
            id,
            status: result.status,
            transcript: result.transcript,
        })
    }

    pub(crate) fn stop(&self, id: &str) -> bool {
        self.sessions
            .lock()
            .map(|mut sessions| sessions.remove(id).is_some())
            .unwrap_or(false)
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
    Ok(())
}

pub(super) fn debug_ok(
    src: &str,
    graph_json: &str,
    transcript: &str,
    status: jet_debug::SessionStatus,
    session_id: &str,
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
    format!(
        "{{\"protocol\":\"jet.canvas.debug\",\"schema_version\":{},\"ok\":true,\"revision\":{},\"session\":{{\"id\":{},\"state\":{},\"tier\":\"jet-dev-interpreter\",\"persistence\":\"local-source-span\"}},\"overlay\":{{\"debug_overlay\":{},\"active_line\":{},\"active_span\":{},\"active_graph_id\":{},\"active_node_id\":{},\"active_wire_id\":{},\"breakpoints\":[{}],\"locals\":[{}],\"watches\":[{}],\"call_stack\":[{}],\"trace\":[{}]}}}}",
        DEBUG_SCHEMA_VERSION,
        json_str(&source_revision(src)),
        json_str(session_id),
        json_str(overlay),
        json_str(overlay),
        active_line
            .map(|line| line.to_string())
            .unwrap_or_else(|| "null".to_string()),
        span_json(active_span),
        json_str(&active_graph),
        json_str(&active_node),
        json_str(&active_wire),
        breakpoint_json(src, breakpoint_lines, stale_breakpoints),
        locals_json(transcript),
        watches_json(transcript, watches),
        call_stack_json(transcript),
        trace_json(transcript)
    )
}

pub(super) fn debug_error(kind: &str, message: &str) -> String {
    format!(
        "{{\"protocol\":\"jet.canvas.debug\",\"schema_version\":{},\"ok\":false,\"kind\":{},\"message\":{}}}",
        DEBUG_SCHEMA_VERSION,
        json_str(kind),
        json_str(message)
    )
}

pub(super) fn debug_stop_ok(src: &str, session_id: &str) -> String {
    format!(
        "{{\"protocol\":\"jet.canvas.debug\",\"schema_version\":{},\"ok\":true,\"revision\":{},\"session\":{{\"id\":{},\"state\":\"stopped\",\"tier\":\"jet-dev-interpreter\",\"persistence\":\"local-source-span\"}},\"overlay\":null}}",
        DEBUG_SCHEMA_VERSION,
        json_str(&source_revision(src)),
        json_str(session_id)
    )
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

fn locals_json(transcript: &str) -> String {
    let Some(line) = transcript
        .lines()
        .rev()
        .find(|line| line.starts_with("locals:"))
    else {
        return String::new();
    };
    parse_assignments(line.trim_start_matches("locals:").trim())
}

fn watches_json(transcript: &str, watches: &[String]) -> String {
    watches
        .iter()
        .filter_map(|watch| {
            let prefix = format!("{watch} = ");
            transcript
                .lines()
                .rev()
                .find_map(|line| line.strip_prefix(&prefix))
                .map(|value| {
                    format!(
                        "{{\"name\":{},\"value\":{},\"state\":\"ok\"}}",
                        json_str(watch),
                        json_str(value)
                    )
                })
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn parse_assignments(text: &str) -> String {
    if text == "(none)" || text.is_empty() {
        return String::new();
    }
    text.split("   ")
        .take(64)
        .filter_map(|part| {
            let (name, value) = part.split_once(" = ")?;
            Some(format!(
                "{{\"name\":{},\"value\":{}}}",
                json_str(name.trim()),
                json_str(value.trim())
            ))
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn call_stack_json(transcript: &str) -> String {
    transcript
        .lines()
        .filter(|line| line.starts_with('#') && line.contains(" at "))
        .rev()
        .take(32)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(json_str)
        .collect::<Vec<_>>()
        .join(",")
}

fn trace_json(transcript: &str) -> String {
    transcript
        .lines()
        .rev()
        .take(128)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(json_str)
        .collect::<Vec<_>>()
        .join(",")
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
}
