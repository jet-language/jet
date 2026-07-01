//! D-DBG3 step 2 (dap-debugger): the Debug Adapter Protocol server — the
//! "editor wiring" half of the native backend. Same [`super::Inferior`]/
//! [`super::LineMap`] the terminal `(jet)` session (`Native.rs`) uses; this
//! module only adds the DAP wire format (Content-Length framed JSON on stdio,
//! the same convention `Source/LSP/Server.rs` already speaks) so VS Code/Zed
//! can launch `jet debug --dap <file>` as a debug adapter.
//!
//! I6: reuses the hand-rolled JSON codec in `Source/LSP/JSON.rs` (no serde, no
//! DAP crate). I2: every `stackTrace`/`variables` response is translated to
//! Jet terms through `LineMap` before it reaches the editor — the raw Rust
//! frame never crosses the wire (DAP has no `--raw-frames` equivalent; that's
//! a terminal-only expert opt-in).
//!
//! Caveat (honest, not a stub): this speaks the documented DAP message shapes,
//! but this sandbox has no editor to drive it against live — verify against a
//! real VS Code/Zed session before wiring it into `editors/` launch configs.

use std::io::{BufRead, Write};
use std::path::Path;

use super::Inferior::{Inferior, ResumeResult};
use super::LineMap::LineMap;
use crate::LSP::JSON::{json_escape, json_get, json_int, json_str, parse_json, JsonValue};

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
        let Some(body) = read_message(&mut reader) else {
            break;
        };
        let Ok(msg) = parse_json(&body) else { continue };
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
    fn handle(&mut self, msg: &JsonValue, out: &mut impl Write) -> Option<()> {
        let command = json_get(msg, "command").and_then(json_str)?;
        let request_seq = json_get(msg, "seq").and_then(json_int).unwrap_or(0);
        let args = json_get(msg, "arguments");
        match command {
            "initialize" => {
                self.respond(out, request_seq, "initialize", true, "{\"supportsConfigurationDoneRequest\":true}");
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
                let lines: Vec<usize> = args
                    .and_then(|a| json_get(a, "breakpoints"))
                    .and_then(|v| match v {
                        JsonValue::Array(items) => Some(
                            items
                                .iter()
                                .filter_map(|it| json_get(it, "line").and_then(json_int))
                                .map(|n| n as usize)
                                .collect(),
                        ),
                        _ => None,
                    })
                    .unwrap_or_default();
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
                    .map(|l| format!("{{\"verified\":{},\"line\":{}}}", self.map.rust_line_for(*l).is_some(), l))
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
                let body = format!("{{\"stackFrames\":[{}],\"totalFrames\":{}}}", entries.join(","), entries.len());
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
                // I2: only `user_`-mangled bindings are Jet locals; a
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
                self.respond(out, request_seq, command, true, "{\"allThreadsContinued\":true}");
                let resume_cmd = match command {
                    "continue" => "continue",
                    "next" => "thread step-over",
                    "stepIn" => "thread step-in",
                    _ => "thread step-out",
                };
                let result = self.inf.as_mut().and_then(|inf| inf.resume_and_locate(resume_cmd).ok());
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
        let result = self.inf.as_mut().and_then(|inf| inf.resume_and_locate("run").ok());
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

    fn respond(&mut self, out: &mut impl Write, request_seq: i64, command: &str, success: bool, body: &str) {
        let seq = self.next_seq();
        let json = format!(
            "{{\"seq\":{},\"type\":\"response\",\"request_seq\":{},\"success\":{},\"command\":\"{}\",\"body\":{}}}",
            seq, request_seq, success, command, body
        );
        let _ = write_message(out, &json);
    }

    fn respond_err(&mut self, out: &mut impl Write, request_seq: i64, command: &str, message: &str) {
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

/// Content-Length framed read, the same convention `Source/LSP/Server.rs` uses
/// for LSP (DAP shares the exact same header-framing rule).
fn read_message(reader: &mut impl BufRead) -> Option<String> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).ok()?;
        if n == 0 {
            return None;
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some(rest) = line.strip_prefix("Content-Length:") {
            content_length = rest.trim().parse().ok();
        }
    }
    let len = content_length?;
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).ok()?;
    Some(String::from_utf8_lossy(&body).into_owned())
}

fn write_message<W: Write>(w: &mut W, json: &str) -> std::io::Result<()> {
    write!(w, "Content-Length: {}\r\n\r\n{}", json.len(), json)?;
    w.flush()
}
