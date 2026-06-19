//! Machine-readable diagnostic serialization (D-DX1, E2-M3).
//!
//! A single, stable, versioned JSON schema shared by every consumer that needs
//! diagnostics as data rather than prose: the `--json` CLI flag (this chunk),
//! and — reusing the very same function — a future `jet fix` engine and the
//! LSP. The schema is documented in docs/spec/diagnostics.md ("Machine-readable
//! diagnostics (`--json`)") and that doc is the single source of truth.
//!
//! Shape: **JSON Lines** — one self-contained JSON object per diagnostic,
//! terminated by `\n`, matching `cargo --message-format=json` so existing tools
//! and habits transfer. A stream of N diagnostics is N lines; a clean run emits
//! zero lines. This is friendlier to pipe-and-filter (`jq`, `grep`) than one
//! giant array and needs no buffering of the whole batch.
//!
//! Determinism: fields are emitted in a fixed order, numbers are integers, and
//! nothing here ever writes ANSI — the human renderer (diag.rs) is a separate
//! path, so `--json` is never colored. Scripts never parse ANSI (E2-M3).
//!
//! Schema is **additive-only**: bump `SCHEMA_VERSION` and document the change;
//! never repurpose or drop a field within a version.

use crate::Diagnostics::{span_line_col, Diagnostic, Severity, TextEdit};

/// Stable schema version. Bump only for additive, documented changes; never
/// reuse a field name for a new meaning.
pub const SCHEMA_VERSION: u32 = 1;

/// Escape a string for inclusion inside a JSON string literal. std-only (I6):
/// quotes, backslash, the named control escapes, and `\u00xx` for the rest.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// `"key":"value"` with the value escaped.
fn kv_str(key: &str, val: &str) -> String {
    format!("\"{}\":\"{}\"", key, esc(val))
}

/// Render the span object: 1-based line/col of both ends plus raw byte offsets,
/// so editors (1-based) and the fix engine (byte slices) both have what they
/// need without recomputing. Returns `null` when the diagnostic has no span
/// (e.g. a whole-file error).
fn span_json(d: &Diagnostic, src: &str) -> String {
    match d.span {
        None => "null".to_string(),
        Some(span) => {
            let (line, col) = span_line_col(src, span.start);
            let (end_line, end_col) = span_line_col(src, span.end);
            format!(
                "{{\"start_byte\":{},\"end_byte\":{},\"start_line\":{},\"start_col\":{},\"end_line\":{},\"end_col\":{}}}",
                span.start, span.end, line, col, end_line, end_col
            )
        }
    }
}

/// One structured replacement inside a suggestion. The future `jet fix` / LSP
/// applies `new_text` over the byte range `[start_byte, end_byte)` in `file`.
fn replacement_json(file: &str, src: &str, edit: &TextEdit) -> String {
    let (line, col) = span_line_col(src, edit.span.start);
    let (end_line, end_col) = span_line_col(src, edit.span.end);
    format!(
        "{{{},\"span\":{{\"start_byte\":{},\"end_byte\":{},\"start_line\":{},\"start_col\":{},\"end_line\":{},\"end_col\":{}}},{}}}",
        kv_str("file", file),
        edit.span.start,
        edit.span.end,
        line,
        col,
        end_line,
        end_col,
        kv_str("new_text", &edit.new_text),
    )
}

/// The `suggestions` array: machine-applicable fixes. Today a diagnostic
/// carries at most one mechanical `edit` (S14 teaching autocorrect); we surface
/// it as a single suggestion whose human label is the `fix` line. Diagnostics
/// with no mechanical edit emit `[]` — the field is always present so consumers
/// need no special-casing, and the future fix engine can grow multi-edit
/// suggestions without a schema break.
fn suggestions_json(d: &Diagnostic, file: &str, src: &str) -> String {
    match &d.edit {
        None => "[]".to_string(),
        Some(edit) => format!(
            "[{{{},\"replacements\":[{}]}}]",
            kv_str("message", &d.fix),
            replacement_json(file, src, edit),
        ),
    }
}

/// Serialize one diagnostic to a single JSON object (no trailing newline).
///
/// This is THE shared serializer (D-DX1): `--json` writes `to_json(...) + "\n"`
/// per diagnostic; a future `jet fix` reads back the `suggestions`; the LSP can
/// build code actions from the same bytes. Field order is fixed for
/// determinism and snapshot stability.
pub fn to_json(d: &Diagnostic, file: &str, src: &str) -> String {
    let severity = match d.severity {
        Severity::Error => "error",
        Severity::Lint => "warning",
    };
    let detail = match &d.detail {
        Some(t) => kv_str("detail", t),
        None => "\"detail\":null".to_string(),
    };
    format!(
        "{{\"schema_version\":{ver},{code},{severity},{message},{why},{fix},{file},\"span\":{span},\"suggestions\":{sugg},{detail}}}",
        ver = SCHEMA_VERSION,
        code = kv_str("code", d.code),
        severity = kv_str("severity", severity),
        message = kv_str("message", &d.what),
        why = kv_str("why", &d.why),
        fix = kv_str("fix", &d.fix),
        file = kv_str("file", file),
        span = span_json(d, src),
        sugg = suggestions_json(d, file, src),
        detail = detail,
    )
}

/// Serialize a batch as JSON Lines: one object per line, each `\n`-terminated.
/// An empty batch yields the empty string (zero lines) — a clean run prints
/// nothing on the JSON stream, matching the `cargo --message-format=json` habit.
pub fn render_all_json(file: &str, src: &str, diags: &[Diagnostic]) -> String {
    let mut out = String::new();
    for d in diags {
        out.push_str(&to_json(d, file, src));
        out.push('\n');
    }
    out
}
