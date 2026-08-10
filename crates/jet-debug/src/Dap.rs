//! D-DBG3 step 2 (dap-debugger): the Debug Adapter Protocol server — the
//! "editor wiring" half of the native backend. Same [`super::Inferior`]/
//! [`super::LineMap`] the terminal `(jet)` session (`Native.rs`) uses; this
//! module only adds the DAP wire format (Content-Length framed JSON on stdio,
//! the same convention `Source/LSP/Server.rs` already speaks) so VS Code/Zed
//! can launch `jet debug --dap <file>` as a debug adapter.
//!
//! I6: reuses the foundation hand-rolled JSON codec (no serde, no
//! DAP crate). I2: every `stackTrace`/`variables` response is translated to
//! Jet terms through `LineMap` before it reaches the editor — the raw Rust
//! frame never crosses the wire (DAP has no `--raw-frames` equivalent; that's
//! a terminal-only expert opt-in).
//!
//! Caveat (honest, not a stub): this speaks the documented DAP message shapes,
//! but this sandbox has no editor to drive it against live — verify against a
//! real VS Code/Zed session before wiring it into `editors/` launch configs.

use std::io::{self, BufRead, Write};
use std::path::Path;

use super::Inferior::{Inferior, ResumeResult};
use super::LineMap::LineMap;
use jet_foundation::JSON::{
    json_escape, json_get, json_str, json_u32, parse_json, read_protocol_content_length, JSONValue,
    MAX_PROTOCOL_MESSAGE_BYTES,
};

pub fn run(binary: &Path, rust_file: &str, rust_src: &str, jet_file: &str, jet_src: &str) -> i32 {
    let map = LineMap::build(rust_src);
    let mut server = DapServer {
        map,
        rust_file: rust_file.to_string(),
        rust_src: rust_src.to_string(),
        jet_file: jet_file.to_string(),
        jet_src: jet_src.to_string(),
        binary: binary.to_path_buf(),
        inf: None,
        pending_breakpoints: Vec::new(),
        seq: 1,
    };
    let stdin = std::io::stdin();
    let mut reader = std::io::BufReader::new(stdin.lock());
    let mut stdout = std::io::stdout();
    loop {
        let body = match read_message(&mut reader) {
            Ok(Some(body)) => body,
            Ok(None) | Err(_) => break,
        };
        let Ok(msg) = parse_dap_request(&body) else {
            continue;
        };
        if server.handle(&msg, &mut stdout).is_none() {
            break;
        }
    }
    if let Some(inf) = server.inf.take() {
        inf.quit();
    }
    crate::ExitCodes::OK
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
    seq: i64,
}

impl DapServer {
    fn source_line(&self, line: usize) -> &str {
        self.jet_src
            .lines()
            .nth(line.saturating_sub(1))
            .unwrap_or("")
    }

    /// Dispatch one DAP request. `Some(())` to keep the loop going, `None` to
    /// stop (a `disconnect`/`terminate` request, or an unrecoverable error).
    fn handle(&mut self, msg: &JSONValue, out: &mut impl Write) -> Option<()> {
        let command = json_get(msg, "command").and_then(json_str)?;
        let request_seq = i64::from(json_get(msg, "seq").and_then(json_u32)?);
        let args = json_get(msg, "arguments");
        match command {
            "initialize" => {
                self.respond(
                    out,
                    request_seq,
                    "initialize",
                    true,
                    "{\"supportsConfigurationDoneRequest\":true}",
                );
                self.event(out, "initialized", "{}");
                Some(())
            }
            "launch" | "attach" => {
                match Inferior::spawn(&self.binary) {
                    Ok(mut inf) => {
                        if let Some(entry_line) = self.map.main_entry_line(&self.rust_src) {
                            let _ = inf.set_breakpoint(&self.rust_file, entry_line);
                        }
                        self.inf = Some(inf);
                        self.respond(out, request_seq, command, true, "{}");
                    }
                    Err(e) => self.respond_err(out, request_seq, command, &e.to_string()),
                }
                Some(())
            }
            "setBreakpoints" => {
                let lines = match breakpoint_lines(args) {
                    Ok(lines) => lines,
                    Err(message) => {
                        self.respond_err(out, request_seq, command, message);
                        return Some(());
                    }
                };
                self.pending_breakpoints = lines.clone();
                if let Some(inf) = &mut self.inf {
                    for jline in &lines {
                        if let Some(rust_line) = self.map.rust_line_for(*jline) {
                            let _ = inf.set_breakpoint(&self.rust_file, rust_line);
                        }
                    }
                }
                let verified: Vec<String> = lines
                    .iter()
                    .map(|l| {
                        format!(
                            "{{\"verified\":{},\"line\":{}}}",
                            self.map.rust_line_for(*l).is_some(),
                            l
                        )
                    })
                    .collect();
                let body = format!("{{\"breakpoints\":[{}]}}", verified.join(","));
                self.respond(out, request_seq, command, true, &body);
                Some(())
            }
            "configurationDone" => {
                self.respond(out, request_seq, command, true, "{}");
                self.apply_pending_breakpoints_and_run(out);
                Some(())
            }
            "threads" => {
                self.respond(
                    out,
                    request_seq,
                    command,
                    true,
                    "{\"threads\":[{\"id\":1,\"name\":\"main\"}]}",
                );
                Some(())
            }
            "stackTrace" => {
                let frames = self
                    .inf
                    .as_mut()
                    .and_then(|inf| inf.backtrace().ok())
                    .map(|out_text| Inferior::parse_frames(&out_text))
                    .unwrap_or_default();
                let entries: Vec<String> = frames
                    .iter()
                    .enumerate()
                    .filter_map(|(i, f)| {
                        let jline = self.map.jet_line_for(f.rust_line)?;
                        Some(format!(
                            "{{\"id\":{},\"name\":\"{}\",\"source\":{{\"path\":\"{}\"}},\"line\":{},\"column\":1}}",
                            i,
                            json_escape(&Inferior::rust_func_to_jet(&f.func)),
                            json_escape(&self.jet_file),
                            jline
                        ))
                    })
                    .collect();
                let body = format!(
                    "{{\"stackFrames\":[{}],\"totalFrames\":{}}}",
                    entries.join(","),
                    entries.len()
                );
                self.respond(out, request_seq, command, true, &body);
                Some(())
            }
            "scopes" => {
                self.respond(
                    out,
                    request_seq,
                    command,
                    true,
                    "{\"scopes\":[{\"name\":\"Locals\",\"variablesReference\":1,\"expensive\":false}]}",
                );
                Some(())
            }
            "variables" => {
                let pairs = self
                    .inf
                    .as_mut()
                    .and_then(|inf| inf.locals().ok())
                    .map(|out_text| Inferior::parse_locals(&out_text))
                    .unwrap_or_default();
                // I2: only `__jet_`-mangled bindings are Jet locals; a
                // compiler-internal temp (no prefix) is filtered, never shown.
                let entries: Vec<String> = pairs
                    .iter()
                    .filter_map(|(n, v)| {
                        let jn = Inferior::rust_local_to_jet(n)?;
                        Some(format!(
                            "{{\"name\":\"{}\",\"value\":\"{}\",\"variablesReference\":0}}",
                            json_escape(&jn),
                            json_escape(v)
                        ))
                    })
                    .collect();
                let body = format!("{{\"variables\":[{}]}}", entries.join(","));
                self.respond(out, request_seq, command, true, &body);
                Some(())
            }
            "continue" | "next" | "stepIn" | "stepOut" => {
                self.respond(
                    out,
                    request_seq,
                    command,
                    true,
                    "{\"allThreadsContinued\":true}",
                );
                let resume_cmd = match command {
                    "continue" => "continue",
                    "next" => "thread step-over",
                    "stepIn" => "thread step-in",
                    _ => "thread step-out",
                };
                let result = self
                    .inf
                    .as_mut()
                    .and_then(|inf| inf.resume_and_locate(resume_cmd).ok());
                if let Some(result) = result {
                    self.emit_resume(out, result);
                }
                Some(())
            }
            "disconnect" | "terminate" => {
                self.respond(out, request_seq, command, true, "{}");
                None
            }
            _ => {
                // Unknown request: acknowledge so a strict client doesn't hang
                // waiting on a response, but claim nothing was done.
                self.respond(out, request_seq, command, false, "{}");
                Some(())
            }
        }
    }

    fn apply_pending_breakpoints_and_run(&mut self, out: &mut impl Write) {
        let lines = std::mem::take(&mut self.pending_breakpoints);
        if let Some(inf) = &mut self.inf {
            for jline in &lines {
                if let Some(rust_line) = self.map.rust_line_for(*jline) {
                    let _ = inf.set_breakpoint(&self.rust_file, rust_line);
                }
            }
        }
        let result = self
            .inf
            .as_mut()
            .and_then(|inf| inf.resume_and_locate("run").ok());
        if let Some(result) = result {
            self.emit_resume(out, result);
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
        self.emit_program_output(out);
        let bt_text = match result {
            ResumeResult::Exited => {
                self.event(out, "exited", "{\"exitCode\":0}");
                self.event(out, "terminated", "{}");
                return;
            }
            ResumeResult::Stopped(text) => text,
        };
        match Inferior::parse_top_frame(&bt_text) {
            Some(frame) => {
                let _ = self.source_line(self.map.jet_line_for(frame.rust_line).unwrap_or(1));
                self.event(
                    out,
                    "stopped",
                    "{\"reason\":\"breakpoint\",\"threadId\":1,\"allThreadsStopped\":true}",
                );
            }
            None => {
                // No parseable frame — still tell the client execution paused,
                // rather than leaving it waiting on a stop event forever.
                self.event(
                    out,
                    "stopped",
                    "{\"reason\":\"step\",\"threadId\":1,\"allThreadsStopped\":true}",
                );
            }
        }
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
            seq, request_seq, success, command, body
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
        let seq = self.next_seq();
        let json = format!(
            "{{\"seq\":{},\"type\":\"response\",\"request_seq\":{},\"success\":false,\"command\":\"{}\",\"message\":\"{}\"}}",
            seq, request_seq, command, json_escape(message)
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
    if !matches!(json_get(&message, "arguments"), None | Some(JSONValue::Object(_))) {
        return Err("arguments must be an object");
    }
    Ok(message)
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

/// Content-Length framed read, the same convention `Source/LSP/Server.rs` uses
/// for LSP (DAP shares the exact same header-framing rule).
fn read_message(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let Some(len) = read_protocol_content_length(reader)? else {
        return Ok(None);
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
    write!(w, "Content-Length: {}\r\n\r\n{}", json.len(), json)?;
    w.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

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
            assert!(parse_dap_request(raw).is_err(), "accepted hostile DAP: {raw}");
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
            assert!(breakpoint_lines(Some(&args)).is_err(), "accepted hostile line: {raw}");
        }
        let args = parse_json(r#"{"breakpoints":[{"line":2},{"line":9}]}"#).unwrap();
        assert_eq!(breakpoint_lines(Some(&args)).unwrap(), vec![2, 9]);
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
}
