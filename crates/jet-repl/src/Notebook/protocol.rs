//! Headless notebook protocol — proves Jupyter adapter + first-party parity.

use super::document::{
    export_ipynb, export_jet, import_ipynb, CellKind, JetNotebook,
};
use super::kernel::{ClientKind, Kernel, RerunDecision};
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum ProtocolMessage {
    Execute { client: ClientKind, cell_id: String },
    Rerun {
        client: ClientKind,
        from_id: usize,
        edited: Option<String>,
        decisions: Vec<RerunDecision>,
    },
    Interrupt,
    Stdin { line: String },
    DebugAttach,
    PerfAttach,
    Inspect { cell_id: String },
    Complete { prefix: String },
    ImportIpynb { text: String },
    Open { path: PathBuf },
    Reopen,
    Edit { cell_id: String, source: String },
    MergePath { path: PathBuf },
    Profile { client: ClientKind, cell_id: String },
    Debug { cell_id: String },
    State,
    ExportIpynb,
    ExportJet,
    Merge { theirs_json: String },
    Save { path: PathBuf },
    AddCell { kind: CellKind, source: String },
    Grant { cell_id: String, renderer: String },
    VisibleOutput { client: ClientKind, cell_id: String },
}

#[derive(Clone, Debug)]
pub enum ProtocolReply {
    Ok { body: String },
    Err { message: String },
}

impl ProtocolReply {
    pub fn ok(body: impl Into<String>) -> Self {
        Self::Ok { body: body.into() }
    }
    pub fn err(message: impl Into<String>) -> Self {
        Self::Err {
            message: message.into(),
        }
    }

    pub fn to_json_line(&self) -> String {
        let (status, payload) = match self {
            Self::Ok { body } => ("ok", body.as_str()),
            Self::Err { message } => ("err", message.as_str()),
        };
        format!(
            "{{\"status\":{},\"body\":{}}}\n",
            json_str(status),
            json_str(payload)
        )
    }
}

fn json_str(s: &str) -> String {
    let mut out = String::from("\"");
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn handle_message(kernel: &mut Kernel, msg: ProtocolMessage) -> ProtocolReply {
    match msg {
        ProtocolMessage::AddCell { kind, source } => {
            let cell = kernel.notebook.add_cell(kind, source);
            ProtocolReply::ok(format!("cell_id={}", cell.id))
        }
        ProtocolMessage::Execute { client, cell_id } => match kernel.execute_cell(client, &cell_id)
        {
            Ok(result) => {
                let status = if result.ok() { "ok" } else { "error" };
                ProtocolReply::ok(format!(
                    "status={status}; turn={:?}; count={}; text={}",
                    result.turn_id,
                    result.execution_count,
                    result.bundle.text_plain.replace('\n', "\\n")
                ))
            }
            Err(e) => ProtocolReply::err(e),
        },
        ProtocolMessage::Rerun {
            client,
            from_id,
            edited,
            decisions,
        } => {
            let plan = match kernel.replay_plan(from_id, edited.as_deref()) {
                Ok(p) => p,
                Err(e) => return ProtocolReply::err(e),
            };
            match kernel.apply_rerun(client, &plan, &decisions) {
                Ok(stale) => ProtocolReply::ok(format!(
                    "stale={}; steps={}",
                    stale
                        .iter()
                        .map(|id| id.to_string())
                        .collect::<Vec<_>>()
                        .join(","),
                    plan.steps.len()
                )),
                Err(e) => ProtocolReply::err(e),
            }
        }
        ProtocolMessage::Interrupt => {
            kernel.request_interrupt();
            ProtocolReply::ok("interrupt_requested")
        }
        ProtocolMessage::Stdin { line } => {
            kernel.push_stdin(line);
            ProtocolReply::ok(format!("stdin_queued={}", kernel.stdin_queue.len()))
        }
        ProtocolMessage::DebugAttach => {
            kernel.attach_debug();
            ProtocolReply::ok("debug_attached")
        }
        ProtocolMessage::PerfAttach => {
            kernel.attach_perf();
            ProtocolReply::ok("perf_attached")
        }
        ProtocolMessage::Open { path } => match kernel.open_document(&path) {
            Ok(()) => ProtocolReply::ok(format!("opened={}", path.display())),
            Err(error) => ProtocolReply::err(error),
        },
        ProtocolMessage::Reopen => match kernel.reopen_document() {
            Ok(()) => ProtocolReply::ok("reopened"),
            Err(error) => ProtocolReply::err(error),
        },
        ProtocolMessage::Edit { cell_id, source } => match kernel.edit_cell(&cell_id, source) {
            Ok(()) => ProtocolReply::ok(format!("edited={cell_id}")),
            Err(error) => ProtocolReply::err(error),
        },
        ProtocolMessage::Profile { client, cell_id } => {
            kernel.attach_perf();
            match kernel.execute_cell(client, &cell_id) {
                Ok(result) => ProtocolReply::ok(format!(
                    "profiled={cell_id}; elapsed_ms={}; count={}",
                    result.elapsed_ms, result.execution_count
                )),
                Err(error) => ProtocolReply::err(error),
            }
        }
        ProtocolMessage::Debug { cell_id } => {
            kernel.attach_debug();
            let Some(cell) = kernel.notebook.cells.iter().find(|c| c.id == cell_id) else {
                return ProtocolReply::err(format!("unknown cell `{cell_id}`"));
            };
            ProtocolReply::ok(format!(
                "debug cell={cell_id}; source={}; bindings={}",
                cell.source.replace('\n', "\\n"),
                kernel.session.scope.keys().cloned().collect::<Vec<_>>().join(",")
            ))
        }
        ProtocolMessage::State => ProtocolReply::ok(kernel.state_json()),
        ProtocolMessage::Inspect { cell_id } => {
            let Some(cell) = kernel.notebook.cells.iter().find(|c| c.id == cell_id) else {
                return ProtocolReply::err(format!("unknown cell `{cell_id}`"));
            };
            let out = kernel
                .notebook
                .visible_output(&cell_id)
                .map(|o| o.bundle.text_plain.clone())
                .unwrap_or_else(|| "(no live output)".into());
            ProtocolReply::ok(format!(
                "id={}; kind={:?}; source_len={}; output={}",
                cell.id,
                cell.kind,
                cell.source.len(),
                out.replace('\n', "\\n")
            ))
        }
        ProtocolMessage::Complete { prefix } => {
            let mut names: Vec<_> = kernel
                .session
                .scope
                .keys()
                .filter(|k| k.starts_with(&prefix))
                .cloned()
                .collect();
            names.sort();
            ProtocolReply::ok(names.join(","))
        }
        ProtocolMessage::ImportIpynb { text } => match import_ipynb(&text) {
            Ok((nb, loss)) => {
                kernel.replace_notebook(nb);
                ProtocolReply::ok(format!(
                    "cells={}; {}",
                    kernel.notebook.cells.len(),
                    loss.render().replace('\n', " | ")
                ))
            }
            Err(e) => ProtocolReply::err(e),
        },
        ProtocolMessage::ExportIpynb => match export_ipynb(&kernel.notebook) {
            Ok((text, loss)) => ProtocolReply::ok(format!(
                "ipynb_bytes={}; {}",
                text.len(),
                loss.render().replace('\n', " | ")
            )),
            Err(e) => ProtocolReply::err(e),
        },
        ProtocolMessage::ExportJet => {
            let (text, loss) = export_jet(&kernel.notebook);
            ProtocolReply::ok(format!(
                "jet_bytes={}; {}",
                text.len(),
                loss.render().replace('\n', " | ")
            ))
        }
        ProtocolMessage::Merge { theirs_json } => {
            match JetNotebook::from_canonical_bytes(theirs_json.as_bytes()) {
                Ok(theirs) => {
                    kernel.merge_notebook(theirs);
                    ProtocolReply::ok(format!("cells={}", kernel.notebook.cells.len()))
                }
                Err(e) => ProtocolReply::err(e),
            }
        }
        ProtocolMessage::MergePath { path } => match super::document::load_jetnb(&path) {
            Ok(theirs) => {
                kernel.merge_notebook(theirs);
                ProtocolReply::ok(format!("cells={}", kernel.notebook.cells.len()))
            }
            Err(error) => ProtocolReply::err(error),
        },
        ProtocolMessage::Save { path } => match kernel.save_document(Some(&path)) {
            Ok(saved) => ProtocolReply::ok(format!("saved={}", saved.display())),
            Err(e) => ProtocolReply::err(e),
        },
        ProtocolMessage::Grant { cell_id, renderer } => {
            match kernel.grant_capability(&cell_id, &renderer) {
                Ok(()) => ProtocolReply::ok("granted"),
                Err(e) => ProtocolReply::err(e),
            }
        }
        ProtocolMessage::VisibleOutput { client, cell_id } => {
            let bundle = match client {
                ClientKind::FirstParty => kernel.first_party_visible_output(&cell_id),
                ClientKind::CanvasLens => kernel.canvas_visible_output(&cell_id),
                ClientKind::JupyterAdapter => kernel.jupyter_visible_output(&cell_id),
            };
            match bundle {
                Some(b) => ProtocolReply::ok(b.text_plain.replace('\n', "\\n")),
                None => ProtocolReply::ok(""),
            }
        }
    }
}

pub fn run_headless_script(kernel: &mut Kernel, lines: &[&str]) -> String {
    let mut out = String::new();
    for line in lines {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let reply = match parse_script_line(kernel, line) {
            Ok(msg) => handle_message(kernel, msg),
            Err(e) => ProtocolReply::err(e),
        };
        out.push_str(&reply.to_json_line());
    }
    out
}

fn parse_script_line(kernel: &Kernel, line: &str) -> Result<ProtocolMessage, String> {
    let mut parts = line.splitn(2, ' ');
    let cmd = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("").trim();
    match cmd {
        "add-jet" => Ok(ProtocolMessage::AddCell {
            kind: CellKind::Jet,
            source: rest.to_string(),
        }),
        "add-md" => Ok(ProtocolMessage::AddCell {
            kind: CellKind::Markdown,
            source: rest.to_string(),
        }),
        "exec" => {
            let mut bits = rest.split_whitespace();
            let client = parse_client(bits.next().unwrap_or("first"))?;
            let cell_id = bits
                .next()
                .map(|s| s.to_string())
                .or_else(|| kernel.notebook.cells.last().map(|c| c.id.clone()))
                .ok_or("exec needs cell id")?;
            Ok(ProtocolMessage::Execute { client, cell_id })
        }
        "interrupt" => Ok(ProtocolMessage::Interrupt),
        "stdin" => Ok(ProtocolMessage::Stdin {
            line: rest.to_string(),
        }),
        "debug" => Ok(ProtocolMessage::DebugAttach),
        "perf" => Ok(ProtocolMessage::PerfAttach),
        "open" => Ok(ProtocolMessage::Open {
            path: PathBuf::from(rest),
        }),
        "reopen" => Ok(ProtocolMessage::Reopen),
        "edit" => {
            let mut bits = rest.splitn(2, ' ');
            Ok(ProtocolMessage::Edit {
                cell_id: bits.next().ok_or("edit needs cell id")?.to_string(),
                source: bits.next().unwrap_or("").to_string(),
            })
        }
        "save" => Ok(ProtocolMessage::Save {
            path: PathBuf::from(rest),
        }),
        "merge" => Ok(ProtocolMessage::MergePath {
            path: PathBuf::from(rest),
        }),
        "import-ipynb" => Ok(ProtocolMessage::ImportIpynb {
            text: rest.to_string(),
        }),
        "state" => Ok(ProtocolMessage::State),
        "profile" => {
            let mut bits = rest.split_whitespace();
            let client = parse_client(bits.next().unwrap_or("first"))?;
            let cell_id = bits
                .next()
                .map(str::to_string)
                .or_else(|| kernel.notebook.cells.last().map(|c| c.id.clone()))
                .ok_or("profile needs cell id")?;
            Ok(ProtocolMessage::Profile { client, cell_id })
        }
        "inspect" => {
            let cell_id = if rest.is_empty() {
                kernel
                    .notebook
                    .cells
                    .last()
                    .map(|c| c.id.clone())
                    .ok_or("inspect needs cell id")?
            } else {
                rest.to_string()
            };
            Ok(ProtocolMessage::Inspect { cell_id })
        }
        "complete" => Ok(ProtocolMessage::Complete {
            prefix: rest.to_string(),
        }),
        "export-ipynb" => Ok(ProtocolMessage::ExportIpynb),
        "export-jet" => Ok(ProtocolMessage::ExportJet),
        "grant" => {
            let mut bits = rest.split_whitespace();
            let cell_id = bits
                .next()
                .map(|s| s.to_string())
                .or_else(|| kernel.notebook.cells.last().map(|c| c.id.clone()))
                .ok_or("grant needs cell id")?;
            Ok(ProtocolMessage::Grant {
                cell_id,
                renderer: bits.next().unwrap_or("jet-notebook").into(),
            })
        }
        "visible" => {
            let mut bits = rest.split_whitespace();
            let client = parse_client(bits.next().unwrap_or("first"))?;
            let cell_id = bits
                .next()
                .map(|s| s.to_string())
                .or_else(|| kernel.notebook.cells.last().map(|c| c.id.clone()))
                .ok_or("visible needs cell id")?;
            Ok(ProtocolMessage::VisibleOutput { client, cell_id })
        }
        "rerun" => {
            let mut bits = rest.split_whitespace();
            let client = parse_client(bits.next().unwrap_or("jupyter"))?;
            let from_id: usize = bits
                .next()
                .ok_or("rerun needs turn id")?
                .parse()
                .map_err(|_| "bad turn id")?;
            let decisions: Vec<_> = bits
                .map(|b| match b {
                    "confirm" | "y" => RerunDecision::Confirm,
                    _ => RerunDecision::SkipStale,
                })
                .collect();
            Ok(ProtocolMessage::Rerun {
                client,
                from_id,
                edited: None,
                decisions,
            })
        }
        other => Err(format!("unknown protocol command `{other}`")),
    }
}

fn parse_client(name: &str) -> Result<ClientKind, String> {
    match name {
        "first" | "first-party" => Ok(ClientKind::FirstParty),
        "canvas" | "lens" => Ok(ClientKind::CanvasLens),
        "jupyter" | "jp" => Ok(ClientKind::JupyterAdapter),
        other => Err(format!("unknown client `{other}`")),
    }
}
