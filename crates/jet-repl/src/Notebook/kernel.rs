//! One shared REPL session observed by first-party, Canvas lens, and Jupyter.

use super::document::{CellKind, JetNotebook};
use super::eval::{evaluate_step, EvalResult};
use super::trust::{
    decide_render, grant_active, ActiveRequest, MimeBundle, RenderDecision, TrustStore,
    POLICY_VERSION,
};
use crate::{ReplFlags, ReplPolicy, RerunPlan, Session, ReplTurn, ReplTurnStatus};
use jet_foundation::SHA256;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientKind {
    FirstParty,
    CanvasLens,
    JupyterAdapter,
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
    pub fn open(path: Option<&Path>, environment_hash: impl Into<String>) -> Self {
        jet_driver::boot_tir_eval();
        let base_dir = path
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        let flags = ReplFlags::new(&[], &[]);
        let mut kernel = Self {
            session: Session::new(),
            policy: ReplPolicy::new(flags, &base_dir),
            base_dir,
            notebook: JetNotebook::new(environment_hash),
            trust: TrustStore::default(),
            execution_count: 0,
            stdin_queue: Vec::new(),
            interrupt_requested: false,
            debug_attached: false,
            perf_attached: false,
        };
        if let Some(path) = path {
            if path.exists() {
                if let Ok(nb) = super::document::load_jetnb(path) {
                    kernel.notebook = nb;
                }
            }
        }
        kernel
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
        self.stdin_queue.push(line.into());
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
        let _ = client;
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

        let mut authorizer = self.policy.authorizer(None);
        let eval = evaluate_step(
            &mut self.session,
            &source,
            &self.base_dir,
            &mut authorizer,
        );
        self.execution_count = self.execution_count.saturating_add(1);
        let bundle = MimeBundle {
            text_plain: eval.text.trim_end().to_string(),
            mime: Vec::new(),
            quarantined: false,
            widget_id: None,
            requested_origins: Vec::new(),
            requested_messages: Vec::new(),
        };
        self.notebook
            .store_output(cell_id, bundle.clone(), self.execution_count)?;
        let src_hash = SHA256::sha256_hex(source.as_bytes());
        let render = decide_render(
            &self.trust,
            &src_hash,
            &self.notebook.environment_hash,
            "jet-notebook",
            &bundle,
        );
        let display = match &render {
            RenderDecision::AllowPassive { text_plain, mime }
            | RenderDecision::AllowActive { text_plain, mime } => MimeBundle {
                text_plain: text_plain.clone(),
                mime: mime.clone(),
                quarantined: false,
                widget_id: bundle.widget_id.clone(),
                requested_origins: Vec::new(),
                requested_messages: Vec::new(),
            },
            RenderDecision::FallbackPlain { text_plain, .. } => MimeBundle {
                text_plain: text_plain.clone(),
                mime: Vec::new(),
                quarantined: true,
                widget_id: None,
                requested_origins: Vec::new(),
                requested_messages: Vec::new(),
            },
        };
        Ok(CellExecResult {
            client,
            eval,
            bundle: display,
            render,
            execution_count: self.execution_count,
            turn_id: self.session.turns.last().map(|t| t.id),
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
        Ok(())
    }

    pub fn jupyter_visible_output(&self, cell_id: &str) -> Option<MimeBundle> {
        self.visible_for(cell_id, "jupyter-adapter")
    }

    pub fn canvas_visible_output(&self, cell_id: &str) -> Option<MimeBundle> {
        self.visible_for(cell_id, "canvas-lens")
    }

    pub fn first_party_visible_output(&self, cell_id: &str) -> Option<MimeBundle> {
        self.visible_for(cell_id, "jet-notebook")
    }

    fn visible_for(&self, cell_id: &str, renderer: &str) -> Option<MimeBundle> {
        // Enforce identical stale-turn display law for every client projection:
        // if the cell's last turn is stale, never return a success payload.
        if let Some(turn) = self.session.turns.iter().rev().find(|t| t.input.trim()
            == self
                .notebook
                .cells
                .iter()
                .find(|c| c.id == cell_id)
                .map(|c| c.source.trim())
                .unwrap_or(""))
        {
            if turn.stale {
                return None;
            }
        }
        let cell = self.notebook.cells.iter().find(|c| c.id == cell_id)?;
        let out = self.notebook.visible_output(cell_id)?;
        let src_hash = SHA256::sha256_hex(cell.source.as_bytes());
        let render = decide_render(
            &self.trust,
            &src_hash,
            &self.notebook.environment_hash,
            renderer,
            &out.bundle,
        );
        Some(match render {
            RenderDecision::AllowPassive { text_plain, mime }
            | RenderDecision::AllowActive { text_plain, mime } => MimeBundle {
                text_plain,
                mime,
                quarantined: false,
                widget_id: out.bundle.widget_id.clone(),
                requested_origins: Vec::new(),
                requested_messages: Vec::new(),
            },
            RenderDecision::FallbackPlain { text_plain, .. } => MimeBundle {
                text_plain,
                mime: Vec::new(),
                quarantined: true,
                widget_id: None,
                requested_origins: Vec::new(),
                requested_messages: Vec::new(),
            },
        })
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
}

impl CellExecResult {
    pub fn ok(&self) -> bool {
        self.eval.status == ReplTurnStatus::Ok
    }
}
