//! One shared REPL session observed by first-party, Canvas lens, and Jupyter.

use super::document::{CellKind, JetNotebook};
use super::eval::{evaluate_step_with_items, EvalResult};
use super::trust::{
    decide_render, grant_active, ActiveRequest, MimeBundle, RenderDecision, TrustStore,
    POLICY_VERSION,
};
use crate::{
    is_item_input, ReplFlags, ReplPolicy, RerunPlan, Session, ReplTurn, ReplTurnStatus,
};
use jet_foundation::SHA256;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientKind {
    FirstParty,
    CanvasLens,
    JupyterAdapter,
}

impl ClientKind {
    pub fn renderer(self) -> &'static str {
        match self {
            Self::FirstParty => "jet-notebook",
            Self::CanvasLens => "canvas-lens",
            Self::JupyterAdapter => "jupyter-adapter",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RerunDecision {
    Confirm,
    SkipStale,
}

pub struct Kernel {
    pub session: Session,
    policy: ReplPolicy,
    base_dir: PathBuf,
    pub notebook: JetNotebook,
    pub trust: TrustStore,
    pub document_path: Option<PathBuf>,
    pub execution_count: u32,
    pub stdin_queue: Vec<String>,
    interrupt_requested: bool,
    debug_attached: bool,
    perf_attached: bool,
}

#[derive(Clone, Debug)]
pub struct KernelView {
    pub client: ClientKind,
    pub turns: Vec<ReplTurn>,
    pub stale_ids: Vec<usize>,
    pub environment_hash: String,
}

impl Kernel {
    pub fn open(
        path: Option<&Path>,
        environment_hash: impl Into<String>,
    ) -> Result<Self, String> {
        jet_driver::boot_tir_eval();
        let environment_hash = environment_hash.into();
        let mut kernel = Self::blank(path, environment_hash);
        if let Some(path) = path {
            if path.exists() {
                kernel.notebook = super::document::load_jetnb(path)?;
            }
        }
        kernel.trust = TrustStore::load(&super::trust::trust_store_path());
        Ok(kernel)
    }

    fn blank(path: Option<&Path>, environment_hash: impl Into<String>) -> Self {
        let base_dir = path
            .and_then(|p| p.parent().filter(|d| !d.as_os_str().is_empty()).map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."));
        let flags = notebook_flags();
        Self {
            session: Session::new(),
            policy: ReplPolicy::for_notebook(flags, &base_dir),
            base_dir,
            notebook: JetNotebook::new(environment_hash),
            trust: TrustStore::default(),
            document_path: path.map(Path::to_path_buf),
            execution_count: 0,
            stdin_queue: Vec::new(),
            interrupt_requested: false,
            debug_attached: false,
            perf_attached: false,
        }
    }

    fn reset_runtime(&mut self) {
        self.session.reset();
        self.policy = ReplPolicy::for_notebook(notebook_flags(), &self.base_dir);
        self.execution_count = 0;
        self.stdin_queue.clear();
        self.interrupt_requested = false;
    }

    pub fn open_document(&mut self, path: &Path) -> Result<(), String> {
        let notebook = super::document::load_jetnb(path)?;
        self.notebook = notebook;
        self.document_path = Some(path.to_path_buf());
        self.base_dir = path
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        self.reset_runtime();
        Ok(())
    }

    pub fn reopen_document(&mut self) -> Result<(), String> {
        let path = self
            .document_path
            .clone()
            .ok_or_else(|| "no notebook path is open".to_string())?;
        self.open_document(&path)
    }

    pub fn save_document(&mut self, path: Option<&Path>) -> Result<PathBuf, String> {
        let target = path
            .map(Path::to_path_buf)
            .or_else(|| self.document_path.clone())
            .ok_or_else(|| "save needs a `.jetnb` path".to_string())?;
        super::document::save_jetnb(&self.notebook, &target)?;
        self.document_path = Some(target.clone());
        self.base_dir = target
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        self.policy = ReplPolicy::for_notebook(notebook_flags(), &self.base_dir);
        Ok(target)
    }

    pub fn replace_notebook(&mut self, notebook: JetNotebook) {
        self.notebook = notebook;
        self.document_path = None;
        self.reset_runtime();
    }

    pub fn merge_notebook(&mut self, theirs: JetNotebook) {
        self.notebook = super::document::merge_by_id(&self.notebook, &theirs);
        self.reset_runtime();
    }

    pub fn edit_cell(&mut self, cell_id: &str, source: impl Into<String>) -> Result<(), String> {
        self.notebook.edit_cell(cell_id, source)?;
        self.session.reset();
        Ok(())
    }

    pub fn environment_hash(project_root: &Path) -> String {
        let marker = format!("{}|notebook-env-1", project_root.display());
        SHA256::sha256_hex(marker.as_bytes())
    }

    pub fn view(&self, client: ClientKind) -> KernelView {
        KernelView {
            client,
            turns: self.session.turns.clone(),
            stale_ids: self
                .session
                .turns
                .iter()
                .filter(|t| t.stale)
                .map(|t| t.id)
                .collect(),
            environment_hash: self.notebook.environment_hash.clone(),
        }
    }

    pub fn replay_plan(
        &self,
        from_id: usize,
        edited: Option<&str>,
    ) -> Result<RerunPlan::ReplayPlan, String> {
        RerunPlan::build_replay_plan(&self.session.turns, from_id, edited)
    }

    pub fn request_interrupt(&mut self) {
        self.interrupt_requested = true;
        crate::Comptime::note_repl_interrupt();
    }

    pub fn push_stdin(&mut self, line: impl Into<String>) {
        let line = line.into();
        self.stdin_queue.push(line.clone());
        self.policy.push_input(line);
    }

    pub fn attach_debug(&mut self) {
        self.debug_attached = true;
    }

    pub fn attach_perf(&mut self) {
        self.perf_attached = true;
    }

    pub fn debug_attached(&self) -> bool {
        self.debug_attached
    }

    pub fn perf_attached(&self) -> bool {
        self.perf_attached
    }

    pub fn execute_cell(
        &mut self,
        client: ClientKind,
        cell_id: &str,
    ) -> Result<CellExecResult, String> {
        if self.interrupt_requested {
            self.interrupt_requested = false;
            return Err("interrupted before execute".into());
        }
        let source = {
            let cell = self
                .notebook
                .cells
                .iter()
                .find(|c| c.id == cell_id)
                .ok_or_else(|| format!("unknown cell `{cell_id}`"))?;
            if cell.kind != CellKind::Jet {
                return Err("markdown cells are not executed".into());
            }
            cell.source.clone()
        };

        let item_srcs = self
            .notebook
            .cells
            .iter()
            .filter(|cell| cell.kind == CellKind::Jet && is_item_input(&cell.source))
            .map(|cell| cell.source.clone())
            .collect();
        self.session.replace_notebook_items(item_srcs);

        let queued_input = self.policy.pending_input();
        let started = Instant::now();
        let mut authorizer = self.policy.authorizer(None);
        crate::Comptime::begin_repl_interruptible_turn();
        let eval = evaluate_step_with_items(
            &mut self.session,
            &source,
            &self.base_dir,
            &mut authorizer,
            true,
        );
        crate::Comptime::end_repl_interruptible_turn();
        if self.policy.pending_input() < queued_input {
            self.stdin_queue.remove(0);
        }
        self.execution_count = self.execution_count.saturating_add(1);
        let bundle = bundle_for_eval(&eval);
        let turn_id = self.session.turns.last().map(|t| t.id);
        self.notebook
            .store_output(cell_id, bundle.clone(), self.execution_count, turn_id)?;
        let src_hash = SHA256::sha256_hex(source.as_bytes());
        let render = decide_render(
            &self.trust,
            &src_hash,
            &self.notebook.environment_hash,
            client.renderer(),
            &bundle,
        );
        let display = display_bundle(&render, &bundle);
        Ok(CellExecResult {
            client,
            eval,
            bundle: display,
            render,
            execution_count: self.execution_count,
            turn_id,
            elapsed_ms: started.elapsed().as_millis(),
        })
    }

    pub fn apply_rerun(
        &mut self,
        client: ClientKind,
        plan: &RerunPlan::ReplayPlan,
        decisions: &[RerunDecision],
    ) -> Result<Vec<usize>, String> {
        let _ = client;
        let mut decision_iter = decisions.iter().copied();
        let mut stale_from = None;
        for step in &plan.steps {
            if step.kind == RerunPlan::StepKind::ConfirmEffect {
                match decision_iter.next().unwrap_or(RerunDecision::SkipStale) {
                    RerunDecision::Confirm => {}
                    RerunDecision::SkipStale => {
                        stale_from = Some(step.turn_id);
                        break;
                    }
                }
            }
        }
        {
            let mut auth = self.policy.authorizer(None);
            crate::apply_replay_plan_with_stale(
                &mut self.session,
                plan,
                stale_from,
                &self.base_dir,
                false,
                &mut auth,
            );
        }
        Ok(self
            .session
            .turns
            .iter()
            .filter(|t| t.stale)
            .map(|t| t.id)
            .collect())
    }

    pub fn grant_capability(&mut self, cell_id: &str, renderer: &str) -> Result<(), String> {
        let cell = self
            .notebook
            .cells
            .iter()
            .find(|c| c.id == cell_id)
            .ok_or_else(|| format!("unknown cell `{cell_id}`"))?;
        let out = cell
            .output
            .as_ref()
            .ok_or_else(|| "cell has no output to grant".to_string())?;
        let payload_hash = SHA256::sha256_hex(
            format!("{:?}\0{:?}", out.bundle.mime, out.bundle.widget_id).as_bytes(),
        );
        let req = ActiveRequest {
            notebook_source_hash: SHA256::sha256_hex(cell.source.as_bytes()),
            payload_hash,
            renderer_hash: renderer.to_string(),
            environment_hash: self.notebook.environment_hash.clone(),
            policy_version: POLICY_VERSION.to_string(),
            widget_id: out.bundle.widget_id.clone().unwrap_or_default(),
            origins: out.bundle.requested_origins.clone(),
            messages: out.bundle.requested_messages.clone(),
        };
        grant_active(&mut self.trust, &req);
        self.trust
            .save(&super::trust::trust_store_path())
            .map_err(|error| format!("trust grant was not saved: {error}"))?;
        Ok(())
    }

    pub fn jupyter_visible_output(&self, cell_id: &str) -> Option<MimeBundle> {
        self.visible_for(cell_id, ClientKind::JupyterAdapter)
    }

    pub fn canvas_visible_output(&self, cell_id: &str) -> Option<MimeBundle> {
        self.visible_for(cell_id, ClientKind::CanvasLens)
    }

    pub fn first_party_visible_output(&self, cell_id: &str) -> Option<MimeBundle> {
        self.visible_for(cell_id, ClientKind::FirstParty)
    }

    fn visible_for(&self, cell_id: &str, client: ClientKind) -> Option<MimeBundle> {
        // Enforce identical stale-turn display law for every client projection:
        // if the cell's last turn is stale, never return a success payload.
        let cell = self.notebook.cells.iter().find(|c| c.id == cell_id)?;
        if let Some(turn_id) = cell.output.as_ref().and_then(|output| output.turn_id) {
            if self
                .session
                .turns
                .iter()
                .find(|turn| turn.id == turn_id)
                .is_some_and(|turn| turn.stale)
            {
                return None;
            }
        } else if let Some(turn) = self
            .session
            .turns
            .iter()
            .rev()
            .find(|t| t.input.trim() == cell.source.trim())
        {
            if turn.stale {
                return None;
            }
        }
        let out = self.notebook.visible_output(cell_id)?;
        let src_hash = SHA256::sha256_hex(cell.source.as_bytes());
        let render = decide_render(
            &self.trust,
            &src_hash,
            &self.notebook.environment_hash,
            client.renderer(),
            &out.bundle,
        );
        Some(display_bundle(&render, &out.bundle))
    }

    pub fn state_json(&self) -> String {
        let cells = self
            .notebook
            .cells
            .iter()
            .map(|cell| {
                let output = cell.output.as_ref().map(|out| {
                    let live = self.cell_output_live(cell);
                    format!(
                        "{{\"text\":{},\"quarantined\":{},\"live\":{},\"mime\":{}}}",
                        json_str(&bounded_text(&out.bundle.text_plain)),
                        out.bundle.quarantined,
                        live,
                        json_mime(&out.bundle.mime)
                    )
                });
                format!(
                    "{{\"id\":{},\"kind\":{},\"source\":{},\"output\":{}}}",
                    json_str(&cell.id),
                    json_str(match cell.kind {
                        CellKind::Jet => "jet",
                        CellKind::Markdown => "markdown",
                    }),
                    json_str(&cell.source),
                    output.unwrap_or_else(|| "null".into())
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let turns = self
            .session
            .turns
            .iter()
            .map(|turn| {
                format!(
                    "{{\"id\":{},\"input\":{},\"summary\":{},\"status\":{},\"stale\":{},\"had_effect\":{}}}",
                    turn.id,
                    json_str(&turn.input),
                    json_str(&bounded_text(&turn.summary)),
                    json_str(match turn.status {
                        ReplTurnStatus::Ok => "ok",
                        ReplTurnStatus::Error => "error",
                        ReplTurnStatus::Interrupted => "interrupted",
                    }),
                    turn.stale,
                    turn.had_effect
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"environment_hash\":{},\"path\":{},\"execution_count\":{},\"debug\":{},\"perf\":{},\"pending_stdin\":{},\"cells\":[{}],\"turns\":[{}]}}",
            json_str(&self.notebook.environment_hash),
            self.document_path
                .as_ref()
                .map(|path| json_str(&path.display().to_string()))
                .unwrap_or_else(|| "null".into()),
            self.execution_count,
            self.debug_attached,
            self.perf_attached,
            self.policy.pending_input(),
            cells,
            turns
        )
    }

    fn cell_output_live(&self, cell: &super::document::NotebookCell) -> bool {
        if self.notebook.visible_output(&cell.id).is_none() {
            return false;
        }
        cell.output
            .as_ref()
            .and_then(|output| output.turn_id)
            .and_then(|turn_id| self.session.turns.iter().find(|turn| turn.id == turn_id))
            .is_none_or(|turn| !turn.stale)
    }
}

#[derive(Clone, Debug)]
pub struct CellExecResult {
    pub client: ClientKind,
    pub eval: EvalResult,
    pub bundle: MimeBundle,
    pub render: RenderDecision,
    pub execution_count: u32,
    pub turn_id: Option<usize>,
    pub elapsed_ms: u128,
}

impl CellExecResult {
    pub fn ok(&self) -> bool {
        self.eval.status == ReplTurnStatus::Ok
    }
}

fn bundle_for_eval(eval: &EvalResult) -> MimeBundle {
    let text = eval
        .value
        .as_ref()
        .filter(|value| !matches!(value, crate::Comptime::CtValue::Unit))
        .map(crate::display_value)
        .unwrap_or_else(|| eval.text.trim_end().to_string());
    let mut mime = vec![("text/plain".to_string(), text.clone())];
    if text.trim_start().starts_with("<svg") {
        mime.push(("image/svg+xml".to_string(), text.clone()));
    }
    if let Some(table) = eval.value.as_ref().and_then(table_mime) {
        mime.push(("text/html".to_string(), table));
    }
    MimeBundle {
        text_plain: text,
        mime,
        quarantined: false,
        widget_id: None,
        requested_origins: Vec::new(),
        requested_messages: Vec::new(),
    }
}

fn table_mime(value: &crate::Comptime::CtValue) -> Option<String> {
    let crate::Comptime::CtValue::List(rows) = value else {
        return None;
    };
    let first = rows.first()?;
    let crate::Comptime::CtValue::Struct { fields, .. } = first else {
        return None;
    };
    let mut html = String::from("<table><thead><tr>");
    for (name, _) in fields {
        html.push_str("<th>");
        html.push_str(&html_escape(name));
        html.push_str("</th>");
    }
    html.push_str("</tr></thead><tbody>");
    for row in rows {
        let crate::Comptime::CtValue::Struct { fields, .. } = row else {
            continue;
        };
        html.push_str("<tr>");
        for (_, value) in fields {
            html.push_str("<td>");
            html.push_str(&html_escape(&crate::display_value(value)));
            html.push_str("</td>");
        }
        html.push_str("</tr>");
    }
    html.push_str("</tbody></table>");
    Some(html)
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn notebook_flags() -> ReplFlags {
    ReplFlags::new(&["IO".into(), "FS".into()], &[])
}

fn display_bundle(render: &RenderDecision, source: &MimeBundle) -> MimeBundle {
    match render {
        RenderDecision::AllowPassive { text_plain, mime }
        | RenderDecision::AllowActive { text_plain, mime } => MimeBundle {
            text_plain: text_plain.clone(),
            mime: mime.clone(),
            quarantined: false,
            widget_id: source.widget_id.clone(),
            requested_origins: source.requested_origins.clone(),
            requested_messages: source.requested_messages.clone(),
        },
        RenderDecision::FallbackPlain { text_plain, .. } => MimeBundle {
            text_plain: text_plain.clone(),
            mime: Vec::new(),
            quarantined: true,
            widget_id: None,
            requested_origins: Vec::new(),
            requested_messages: Vec::new(),
        },
    }
}

fn bounded_text(text: &str) -> String {
    const LIMIT: usize = 64 * 1024;
    if text.len() <= LIMIT {
        return text.to_string();
    }
    let mut end = LIMIT;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}\n… output truncated at 64 KiB; save/export retains the full value",
        &text[..end]
    )
}

fn json_mime(mime: &[(String, String)]) -> String {
    let items = mime
        .iter()
        .map(|(kind, data)| {
            format!(
                "{{\"type\":{},\"data\":{}}}",
                json_str(kind),
                json_str(&bounded_text(data))
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("[{items}]")
}

fn json_str(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}
