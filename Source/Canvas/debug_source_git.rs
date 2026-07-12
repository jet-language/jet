use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Diagnostics::Diagnostic;
use jet_semindex::SourceSpan;

use super::schema_api::{DEBUG_SCHEMA_VERSION, source_revision};
use super::validation_json::{json_str, json_string_field, json_usize_field, parse_json_string, span_json};

pub(super) fn debug_ok(
    src: &str,
    graph_json: &str,
    transcript: &str,
    breakpoint_lines: &[usize],
    watches: &[String],
) -> String {
    let active_line = active_line_from_transcript(transcript);
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
    let overlay = if active_line.is_some() {
        "running"
    } else {
        "finished"
    };
    format!(
        "{{\"protocol\":\"jet.canvas.debug\",\"schema_version\":{},\"ok\":true,\"revision\":{},\"session\":{{\"id\":\"local-source-span\",\"state\":{},\"persistence\":\"local-source-span\"}},\"overlay\":{{\"debug_overlay\":{},\"active_line\":{},\"active_span\":{},\"active_graph_id\":{},\"active_node_id\":{},\"active_wire_id\":{},\"breakpoints\":[{}],\"locals\":[{}],\"watches\":[{}],\"call_stack\":[{}],\"trace\":[{}]}}}}",
        DEBUG_SCHEMA_VERSION,
        json_str(&source_revision(src)),
        json_str(overlay),
        json_str(overlay),
        active_line
            .map(|line| line.to_string())
            .unwrap_or_else(|| "null".to_string()),
        span_json(active_span),
        json_str(&active_graph),
        json_str(&active_node),
        json_str(&active_wire),
        breakpoint_json(src, breakpoint_lines),
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

pub(super) fn debug_diagnostics_error(path: &Path, src: &str, diags: &[Diagnostic]) -> String {
    debug_error(
        "diagnostic",
        &crate::render_diagnostics(&path.display().to_string(), src, diags),
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
    let (start, _) = anchor.split_once(':')?;
    let offset = start.parse::<usize>().ok()?;
    Some(line_of_offset(src, offset))
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

fn breakpoint_json(src: &str, lines: &[usize]) -> String {
    lines
        .iter()
        .map(|line| {
            let span = line_span(src, *line);
            format!(
                "{{\"line\":{},\"source_span\":{},\"state\":\"valid\"}}",
                line,
                span_json(span)
            )
        })
        .collect::<Vec<_>>()
        .join(",")
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
        .map(json_str)
        .collect::<Vec<_>>()
        .join(",")
}

fn trace_json(transcript: &str) -> String {
    transcript
        .lines()
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
