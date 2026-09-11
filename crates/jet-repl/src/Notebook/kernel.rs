//! One shared REPL session observed by first-party, Canvas lens, and Jupyter.

use super::document::{CellKind, JetNotebook};
use super::eval::{evaluate_step_with_items, EvalResult};
use super::trust::{
    decide_render, grant_active, ActiveRequest, MimeBundle, RenderDecision, TrustStore,
    POLICY_VERSION,
};
use crate::{is_item_input, ReplFlags, ReplPolicy, ReplTurn, ReplTurnStatus, RerunPlan, Session};
use jet_foundation::SHA256;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KernelIdentity {
    pub source: String,
    pub build: String,
    pub session: String,
    pub authority: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotebookAppInput {
    pub name: String,
    pub label: String,
    pub type_name: String,
    pub required: bool,
    pub variadic: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotebookAppSchema {
    pub function: String,
    pub inputs: Vec<NotebookAppInput>,
    pub return_projection: String,
    pub effects: Vec<String>,
    pub authority: String,
    pub source_identity: String,
    pub compatibility_digest: String,
}


#[derive(Clone, Debug)]
struct ControlSpec {
    function: String,
    name: String,
    label: String,
    type_name: String,
    required: bool,
    variadic: bool,
}

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
    document_notice: Option<String>,
    interrupt_requested: bool,
    debug_attached: bool,
    perf_attached: bool,
    cell_effects: BTreeMap<String, bool>,
    cell_lazy: BTreeSet<String>,
    pending_effects: BTreeSet<String>,
    cell_errors: BTreeMap<String, String>,
    executed_cells: BTreeSet<String>,
    reactive_root: Option<String>,
    session_id: String,
    control_values: BTreeMap<String, String>,
    control_errors: BTreeMap<String, String>,
    authority_id: String,
}

#[derive(Clone, Debug)]
pub struct KernelView {
    pub client: ClientKind,
    pub turns: Vec<ReplTurn>,
    pub stale_ids: Vec<usize>,
    pub environment_hash: String,
}

impl Kernel {
    pub fn open(path: Option<&Path>, environment_hash: impl Into<String>) -> Result<Self, String> {
        jet_driver::boot_mir_eval();
        let environment_hash = environment_hash.into();
        let mut kernel = Self::blank(path, environment_hash);
        if let Some(path) = path {
            if path.exists() {
                let mut notebook = super::document::load_jetnb(path)?;
                if notebook.environment_hash != kernel.notebook.environment_hash {
                    kernel.document_notice = Some(
                        "notebook environment changed; cached output is stale until local re-run"
                            .into(),
                    );
                    notebook.environment_hash = kernel.notebook.environment_hash.clone();
                }
                kernel.notebook = notebook;
                kernel.notebook.refresh_dependencies()?;
                kernel.clear_document_runtime();
            } else {
                kernel.notebook.refresh_dependencies()?;
            }
        } else {
            kernel.notebook.refresh_dependencies()?;
        }
        kernel.trust = TrustStore::load(&super::trust::trust_store_path());
        Ok(kernel)
    }

    fn blank(path: Option<&Path>, environment_hash: impl Into<String>) -> Self {
        let base_dir = path
            .and_then(|p| {
                p.parent()
                    .filter(|d| !d.as_os_str().is_empty())
                    .map(Path::to_path_buf)
            })
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
            document_notice: None,
            interrupt_requested: false,
            debug_attached: false,
            perf_attached: false,
            cell_effects: BTreeMap::new(),
            cell_lazy: BTreeSet::new(),
            pending_effects: BTreeSet::new(),
            cell_errors: BTreeMap::new(),
            executed_cells: BTreeSet::new(),
            reactive_root: None,
            session_id: JetNotebook::mint_cell_id(),
            control_values: BTreeMap::new(),
            control_errors: BTreeMap::new(),
            authority_id: SHA256::sha256_hex(b"headless"),
        }
    }

    fn clear_document_runtime(&mut self) {
        self.cell_effects.clear();
        self.cell_lazy.clear();
        self.pending_effects.clear();
        self.cell_errors.clear();
        self.executed_cells.clear();
        self.control_values.clear();
        self.control_errors.clear();
        self.reactive_root = None;
    }

    fn reset_runtime(&mut self) {
        self.session.reset();
        self.policy = ReplPolicy::for_notebook(notebook_flags(), &self.base_dir);
        self.execution_count = 0;
        self.stdin_queue.clear();
        for cell in &mut self.notebook.cells {
            if let Some(output) = &mut cell.output {
                // Turn ids belong to one in-memory session. They must not be
                // compared with newly numbered turns after reopen/edit/merge.
                output.turn_id = None;
            }
        }
        self.debug_attached = false;
        self.perf_attached = false;
        self.interrupt_requested = false;
        self.executed_cells.clear();
        self.pending_effects.clear();
        self.cell_errors.clear();
        self.reactive_root = None;

    }

    pub fn open_document(&mut self, path: &Path) -> Result<(), String> {
        let mut notebook = super::document::load_jetnb(path)?;
        let base_dir = path
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let environment_hash = Self::environment_hash(&base_dir);
        self.document_notice = (notebook.environment_hash != environment_hash).then(|| {
            "notebook environment changed; cached output is stale until local re-run".into()
        });
        notebook.environment_hash = environment_hash;
        notebook.refresh_dependencies()?;
        self.notebook = notebook;
        self.document_path = Some(path.to_path_buf());
        self.base_dir = base_dir;
        self.clear_document_runtime();
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
        let target_base_dir = target
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        self.notebook.environment_hash = Self::environment_hash(&target_base_dir);
        self.notebook.refresh_dependencies()?;
        super::document::save_jetnb(&self.notebook, &target)?;
        self.document_notice = None;
        self.document_path = Some(target.clone());
        self.base_dir = target_base_dir;
        self.policy = ReplPolicy::for_notebook(notebook_flags(), &self.base_dir);
        Ok(target)
    }
    pub fn replace_notebook(&mut self, mut notebook: JetNotebook) {
        notebook.environment_hash = Self::environment_hash(&self.base_dir);
        let _ = notebook.refresh_dependencies();
        self.notebook = notebook;
        self.document_path = None;
        self.document_notice = Some(
            "imported notebook output is quarantined; run cells locally to create trusted cache"
                .into(),
        );
        self.clear_document_runtime();
        self.reset_runtime();
    }

    pub fn merge_notebook(&mut self, theirs: JetNotebook) {
        self.notebook = super::document::merge_by_id(&self.notebook, &theirs);
        let _ = self.notebook.refresh_dependencies();
        self.document_notice = (!self.notebook.merge_conflicts.is_empty())
            .then(|| "merge conflicts are present; edit the marked cells before execution".into());
        self.clear_document_runtime();
        self.reset_runtime();
    }

    pub fn edit_cell(&mut self, cell_id: &str, source: impl Into<String>) -> Result<(), String> {
        let previous_source = self
            .notebook
            .cells
            .iter()
            .find(|cell| cell.id == cell_id)
            .map(|cell| cell.source.clone())
            .ok_or_else(|| format!("unknown cell `{cell_id}`"))?;
        self.notebook.edit_cell(cell_id, source)?;
        if self
            .notebook
            .cells
            .iter()
            .find(|cell| cell.id == cell_id)
            .is_some_and(|cell| cell.source != previous_source)
        {
            self.reset_runtime();
            self.reactive_root = Some(cell_id.to_string());
            let descendants = self.notebook.descendants_in_order(cell_id)?;
            self.pending_effects.extend(descendants.into_iter().filter(|id| {
                self.cell_effects.get(id).copied().unwrap_or(false)
            }));
        }
        if self.notebook.merge_conflicts.is_empty() {
            self.document_notice = None;
        }
        Ok(())
    }

    pub fn add_cell(&mut self, kind: CellKind, source: impl Into<String>) -> Result<String, String> {
        let id = self.notebook.add_cell(kind, source).id.clone();
        self.notebook.refresh_dependencies()?;
        Ok(id)
    }

    pub fn delete_cell(&mut self, cell_id: &str) -> Result<Vec<String>, String> {
        let removed = self.notebook.remove_cell(cell_id)?;
        for id in &removed {
            self.cell_effects.remove(id);
            self.cell_lazy.remove(id);
            self.pending_effects.remove(id);
            self.cell_errors.remove(id);
            self.executed_cells.remove(id);
        }
        self.reset_runtime();
        self.reactive_root = None;
        Ok(removed)
    }

    pub fn set_cell_lazy(&mut self, cell_id: &str, lazy: bool) -> Result<(), String> {
        if self.notebook.cell_index(cell_id).is_none() {
            return Err(diagnostic_error(
                "E2104",
                "notebook cell does not exist",
                format!("the requested lazy-cell control named `{cell_id}`"),
                "refresh notebook state and choose an existing cell",
            ));
        }
        if lazy {
            self.cell_lazy.insert(cell_id.to_string());
        } else {
            self.cell_lazy.remove(cell_id);
            self.pending_effects.remove(cell_id);
        }
        Ok(())
    }
    pub fn set_authority(&mut self, authority: &str) {
        self.authority_id = SHA256::sha256_hex(authority.as_bytes());
    }

    pub fn identity(&self) -> KernelIdentity {
        KernelIdentity {
            source: self.notebook.source_hash(),
            build: build_identity(&self.notebook.environment_hash),
            session: self.session_id.clone(),
            authority: self.authority_id.clone(),
        }
    }

    pub fn reconnect(&self, identity: &KernelIdentity) -> Result<(), String> {
        let expected = self.identity();
        let mismatches = [
            ("source", expected.source.as_str(), identity.source.as_str()),
            ("build", expected.build.as_str(), identity.build.as_str()),
            ("session", expected.session.as_str(), identity.session.as_str()),
            (
                "authority",
                expected.authority.as_str(),
                identity.authority.as_str(),
            ),
        ]
        .into_iter()
        .filter_map(|(name, expected, actual)| (expected != actual).then_some(name))
        .collect::<Vec<_>>();
        if mismatches.is_empty() {
            return Ok(());
        }
        Err(diagnostic_error(
            "E2104",
            "notebook reconnect identity does not match",
            format!("the existing bounded kernel rejected {} identity fields", mismatches.join(", ")),
            "refresh the notebook state and reconnect to the reported source, build, session, and authority",
        ))
    }

    /// Return the checked contract for one notebook function.  The app host
    /// consumes this projection instead of parsing or interpreting the AST.
    pub fn app_schema(&self, function_name: &str) -> Result<NotebookAppSchema, String> {
        let source = self.checked_source();
        if source.trim().is_empty() {
            return Err(diagnostic_error(
                "E2104",
                "notebook app has no checked functions",
                "the notebook contains no parser-plane function declarations".to_string(),
                "add one checked function cell and refresh the notebook".to_string(),
            ));
        }
        let bundle = crate::checked_program(&source).map_err(|_| {
            diagnostic_error(
                "E2104",
                "notebook app function is not checked",
                format!("the selected function `{function_name}` could not be checked"),
                "fix the notebook diagnostics before publishing a local app",
            )
        })?;
        let Some(function) = bundle.modules[bundle.entry].items.iter().find_map(|item| {
            let crate::AST::Item::Func(function) = item else {
                return None;
            };
            (function.name == function_name).then_some(function)
        }) else {
            return Err(diagnostic_error(
                "E2104",
                "notebook app function does not exist",
                format!("no checked function named `{function_name}` is available"),
                "select one of the checked top-level functions in the notebook",
            ));
        };
        if function.is_unsafe {
            return Err(diagnostic_error(
                "E2104",
                "notebook app function is unsafe",
                format!("checked function `{function_name}` carries an unsafe contract"),
                "select a memory-safe function for local-app publication",
            ));
        }
        if let Some((parameter, _)) = &function.effect_via {
            return Err(diagnostic_error(
                "E2104",
                "notebook app function has dynamic effects",
                format!(
                    "checked function `{function_name}` forwards effects through `{parameter}`"
                ),
                "publish a function with a static IO or FS effect row",
            ));
        }
        if let Some(declared) = &function.declared_effects {
            if let Some(effect) = declared
                .iter()
                .map(|(effect, _)| effect)
                .find(|effect| !app_effect_supported(effect))
            {
                return Err(diagnostic_error(
                    "E2104",
                    "notebook app effect is unsupported",
                    format!(
                        "checked function `{function_name}` declares `{effect}`, which the notebook app authority cannot grant"
                    ),
                    "use only project-confined IO or FS effects for local-app publication",
                ));
            }
        }
        if let Some(param) = function
            .params
            .iter()
            .find(|param| !app_control_type_supported(&param.ty))
        {
            return Err(diagnostic_error(
                "E2104",
                "notebook app control type is unsupported",
                format!(
                    "checked function `{function_name}` cannot expose `{}` as a browser control",
                    param.ty.name()
                ),
                "use a scalar, optional, list, map, or tuple control type",
            ));
        }
        let inputs = function
            .params
            .iter()
            .map(|param| NotebookAppInput {
                name: param.name.clone(),
                label: param.call_label().to_string(),
                type_name: param.ty.name(),
                required: param.default.is_none() && !param.variadic,
                variadic: param.variadic,
            })
            .collect::<Vec<_>>();
        let return_projection = function.effective_return_type().name();
        let effects: Vec<String> = function
            .declared_effects
            .as_ref()
            .map(|declared| declared.iter().map(|(name, _)| name.clone()).collect())
            .unwrap_or_default();
        let source_identity = self.notebook.source_hash();
        let authority = self.authority_id.clone();
        let mut material = String::from("jet-notebook-app-compat-v1\0");
        material.push_str(&function.name);
        material.push('\0');
        for input in &inputs {
            material.push_str(&input.name);
            material.push('\0');
            material.push_str(&input.label);
            material.push('\0');
            material.push_str(&input.type_name);
            material.push('\0');
            material.push_str(if input.required { "required" } else { "optional" });
            material.push('\0');
            material.push_str(if input.variadic { "variadic" } else { "single" });
            material.push('\0');
        }
        material.push_str(&return_projection);
        material.push('\0');
        for effect in &effects {
            material.push_str(effect.as_str());
            material.push('\0');
        }
        material.push_str(if function.is_unsafe { "authority-required" } else { "authority-free" });
        let compatibility_digest = SHA256::sha256_hex(material.as_bytes());
        Ok(NotebookAppSchema {
            function: function.name.clone(),
            inputs,
            return_projection,
            effects,
            authority,
            source_identity,
            compatibility_digest,
        })
    }

    pub fn controls_json(&self) -> String {
        self.function_controls()
            .into_iter()
            .map(|control| {
                let key = control_key(&control.function, &control.name);
                let value = self
                    .control_values
                    .get(&key)
                    .cloned()
                    .unwrap_or_default();
                let error = self
                    .control_errors
                    .get(&key)
                    .map(|message| json_str(message))
                    .unwrap_or_else(|| "null".into());
                format!(
                    "{{\"function\":{},\"name\":{},\"label\":{},\"type\":{},\"required\":{},\"variadic\":{},\"value\":{},\"error\":{}}}",
                    json_str(&control.function),
                    json_str(&control.name),
                    json_str(&control.label),
                    json_str(&control.type_name),
                    control.required,
                    control.variadic,
                    json_str(&value),
                    error
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    }

    pub fn set_control(
        &mut self,
        function: &str,
        name: &str,
        value: impl Into<String>,
    ) -> Result<(), String> {
        let value = value.into();
        let Some(control) = self
            .function_controls()
            .into_iter()
            .find(|control| control.function == function && control.name == name)
        else {
            return Err(diagnostic_error(
                "E2104",
                "notebook function argument does not exist",
                format!("no checked function argument `{function}.{name}` is available"),
                "refresh state and choose one of the typed controls reported by the notebook",
            ));
        };
        let key = control_key(function, name);
        if !control_value_matches(&value, &control.type_name) {
            let message = format!(
                "value for `{}` must be a {}",
                control.label, control.type_name
            );
            self.control_errors.insert(key, message.clone());
            return Err(diagnostic_error(
                "E2104",
                "notebook control value has the wrong type",
                message.clone(),
                format!("enter a value accepted by the checked `{}` type", control.type_name),
            ));
        }
        self.control_errors.remove(&key);
        self.control_values.insert(key, value);
        Ok(())
    }


    fn checked_source(&self) -> String {
        self.notebook
            .cells
            .iter()
            .filter(|cell| cell.kind == CellKind::Jet && is_item_input(&cell.source))
            .map(|cell| cell.source.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn function_controls(&self) -> Vec<ControlSpec> {
        let source = self.checked_source();
        if source.trim().is_empty() {
            return Vec::new();
        }
        let Ok(bundle) = crate::checked_program(&source) else {
            return Vec::new();
        };
        bundle.modules[bundle.entry]
            .items
            .iter()
            .filter_map(|item| {
                let crate::AST::Item::Func(function) = item else {
                    return None;
                };
                Some(
                    function
                        .params
                        .iter()
                        .map(|param| ControlSpec {
                            function: function.name.clone(),
                            name: param.name.clone(),
                            label: param.call_label().to_string(),
                            type_name: param.ty.name(),
                            required: param.default.is_none() && !param.variadic,
                            variadic: param.variadic,
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .flatten()
            .collect()
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
        self.notebook.refresh_dependencies()?;
        let order = self.notebook.dependency_order(cell_id)?;
        let reactive = self.reactive_root.as_deref() == Some(cell_id);
        let mut target = None;
        for id in order {
            let is_target = id == cell_id;
            if !is_target && self.executed_cells.contains(&id) {
                continue;
            }
            let result = self.execute_cell_once(client, &id)?;
            if is_target {
                target = Some(result);
            }
        }
        let target = target.ok_or_else(|| format!("unknown cell `{cell_id}`"))?;
        if reactive && target.ok() {
            let _ = self.run_reactive_descendants(client, cell_id, false)?;
            if self.pending_effects.is_empty() {
                self.reactive_root = None;
            }
        }
        Ok(target)
    }

    fn execute_cell_once(
        &mut self,
        client: ClientKind,
        cell_id: &str,
    ) -> Result<CellExecResult, String> {
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
            if let Some(conflict) = self
                .notebook
                .merge_conflicts
                .iter()
                .find(|conflict| conflict.cell_id == cell_id)
            {
                return Err(format!(
                    "cell `{cell_id}` has an unresolved merge conflict; edit it before execution (ours: {:?}, theirs: {:?})",
                    conflict.ours_source, conflict.theirs_source
                ));
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
        self.cell_effects.insert(cell_id.to_string(), eval.had_effect);
        self.pending_effects.remove(cell_id);
        if eval.status == ReplTurnStatus::Ok {
            self.cell_errors.remove(cell_id);
        } else {
            self.cell_errors
                .insert(cell_id.to_string(), eval.text.clone());
        }
        let bundle = bundle_for_eval(&eval);
        let turn_id = self.session.turns.last().map(|t| t.id);
        self.notebook
            .store_output(cell_id, bundle.clone(), self.execution_count, turn_id)?;
        self.executed_cells.insert(cell_id.to_string());
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

    fn run_reactive_descendants(
        &mut self,
        client: ClientKind,
        root_id: &str,
        confirm_effects: bool,
    ) -> Result<Vec<String>, String> {
        let descendants = self.notebook.descendants_in_order(root_id)?;
        let mut ran = Vec::new();
        for cell_id in descendants {
            let Some(cell) = self.notebook.cells.iter().find(|cell| cell.id == cell_id) else {
                continue;
            };
            if cell.kind != CellKind::Jet {
                continue;
            }
            let known_effect = self.cell_effects.get(&cell_id).copied().unwrap_or(false);
            let needs_confirmation = self.cell_lazy.contains(&cell_id)
                || (!confirm_effects
                    && (known_effect || source_may_have_effect(&cell.source)));
            if needs_confirmation {
                self.pending_effects.insert(cell_id);
                continue;
            }
            match self.execute_cell_once(client, &cell_id) {
                Ok(_) => {
                    ran.push(cell_id);
                }
                Err(error) => {
                    self.cell_errors.insert(cell_id, error);
                }
            }
        }
        Ok(ran)
    }

    pub fn confirm_reactive(
        &mut self,
        client: ClientKind,
        cell_id: &str,
    ) -> Result<Vec<String>, String> {
        self.notebook.refresh_dependencies()?;
        let root = self
            .reactive_root
            .clone()
            .unwrap_or_else(|| cell_id.to_string());
        if self.pending_effects.is_empty() && self.reactive_root.is_none() {
            return Err(diagnostic_error(
                "E2104",
                "notebook has no effectful descendants awaiting confirmation",
                format!("cell `{cell_id}` has no pending reactive rerun"),
                "edit and run a source cell before confirming its effectful descendants",
            ));
        }
        let ran = self.run_reactive_descendants(client, &root, true)?;
        if self.pending_effects.is_empty() {
            self.reactive_root = None;
        }
        Ok(ran)
    }

    pub fn apply_rerun(
        &mut self,
        client: ClientKind,
        plan: &RerunPlan::ReplayPlan,
        decisions: &[RerunDecision],
    ) -> Result<Vec<usize>, String> {
        let _ = client;
        // Replay rebuilds the shared session, but it does not know which
        // notebook cell produced each historical turn. Clear cached cell
        // outputs before rebuilding so an old turn can never be relabeled as
        // live by a newly numbered session turn.
        self.notebook.invalidate_all_outputs();
        self.execution_count = 0;
        self.executed_cells.clear();
        self.pending_effects.clear();
        self.reactive_root = None;
        let mut stale_from = None;
        let mut decision_iter = decisions.iter().copied();
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
            crate::apply_replay_plan_with_stale_quiet(
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
        let out = self
            .notebook
            .visible_output(cell_id)
            .ok_or_else(|| "cell has no live output to grant".to_string())?;
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
        }
        let out = self.notebook.visible_output(cell_id)?;
        let render = self.render_for(cell, out, client);
        Some(display_bundle(&render, &out.bundle))
    }

    fn render_for(
        &self,
        cell: &super::document::NotebookCell,
        out: &super::document::CellOutput,
        client: ClientKind,
    ) -> RenderDecision {
        let src_hash = SHA256::sha256_hex(cell.source.as_bytes());
        let first_party = decide_render(
            &self.trust,
            &src_hash,
            &self.notebook.environment_hash,
            ClientKind::FirstParty.renderer(),
            &out.bundle,
        );
        if client == ClientKind::FirstParty {
            return first_party;
        }
        if let RenderDecision::FallbackPlain { .. } = &first_party {
            // Client projections are never trust bypasses: they may further
            // restrict first-party output, but cannot reveal MIME the first-
            // party client would quarantine.
            return first_party;
        }
        decide_render(
            &self.trust,
            &src_hash,
            &self.notebook.environment_hash,
            client.renderer(),
            &out.bundle,
        )
    }

    pub fn state_json(&self) -> String {
        self.state_json_for(ClientKind::FirstParty)
    }

    pub fn state_json_for(&self, client: ClientKind) -> String {
        let cells = self
            .notebook
            .cells
            .iter()
            .map(|cell| {
                let conflict = self
                    .notebook
                    .merge_conflicts
                    .iter()
                    .any(|entry| entry.cell_id == cell.id);
                let status = self.cell_status(cell);
                let effectful = self.cell_effects.get(&cell.id).copied().unwrap_or(false);
                let lazy = self.cell_lazy.contains(&cell.id);
                let pending = self.pending_effects.contains(&cell.id);
                let error = self
                    .cell_errors
                    .get(&cell.id)
                    .map(|message| json_str(&bounded_text(message)))
                    .unwrap_or_else(|| "null".into());
                let output = cell.output.as_ref().map(|out| {
                    let live = self.cell_output_live(cell);
                    let projected = if live {
                        let render = self.render_for(cell, out, client);
                        display_bundle(&render, &out.bundle)
                    } else {
                        // Imported output remains available as safe text for
                        // recovery, but stale cache content never crosses the
                        // state boundary as renderable MIME.
                        MimeBundle {
                            text_plain: out.bundle.text_plain.clone(),
                            mime: Vec::new(),
                            quarantined: out.bundle.quarantined,
                            widget_id: None,
                            requested_origins: Vec::new(),
                            requested_messages: Vec::new(),
                        }
                    };
                    format!(
                        "{{\"text\":{},\"quarantined\":{},\"live\":{},\"cache_key\":{},\"origins\":{},\"messages\":{},\"mime\":{}}}",
                        json_str(&bounded_text(&projected.text_plain)),
                        projected.quarantined,
                        live,
                        out.cache_key
                            .as_ref()
                            .map(|key| json_str(key))
                            .unwrap_or_else(|| "null".into()),
                        json_strings(&out.bundle.requested_origins),
                        json_strings(&out.bundle.requested_messages),
                        json_mime(&projected.mime)
                    )
                });
                format!(
                    "{{\"id\":{},\"kind\":{},\"source\":{},\"depends_on\":{},\"status\":{},\"effectful\":{},\"lazy\":{},\"pending_confirmation\":{},\"error\":{},\"conflict\":{},\"output\":{}}}",
                    json_str(&cell.id),
                    json_str(match cell.kind {
                        CellKind::Jet => "jet",
                        CellKind::Markdown => "markdown",
                    }),
                    json_str(&cell.source),
                    json_strings(&cell.depends_on),
                    json_str(status),
                    effectful,
                    lazy,
                    pending,
                    error,
                    conflict,
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
        let identity = self.identity();
        format!(
            "{{\"environment_hash\":{},\"path\":{},\"notice\":{},\"cache_entries\":{},\"merge_conflicts\":{},\"execution_count\":{},\"debug\":{},\"perf\":{},\"pending_stdin\":{},\"identity\":{{\"source\":{},\"build\":{},\"session\":{},\"authority\":{}}},\"controls\":[{}],\"cells\":[{}],\"turns\":[{}]}}",
            json_str(&self.notebook.environment_hash),
            self.document_path
                .as_ref()
                .map(|path| json_str(&path.display().to_string()))
                .unwrap_or_else(|| "null".into()),
            self.document_notice
                .as_ref()
                .map(|notice| json_str(notice))
                .unwrap_or_else(|| "null".into()),
            self.notebook.output_cache.len(),
            self.notebook.merge_conflicts.len(),
            self.execution_count,
            self.debug_attached,
            self.perf_attached,
            self.policy.pending_input(),
            json_str(&identity.source),
            json_str(&identity.build),
            json_str(&identity.session),
            json_str(&identity.authority),
            self.controls_json(),
            cells,
            turns
        )
    }

    fn cell_status(&self, cell: &super::document::NotebookCell) -> &'static str {
        if self.pending_effects.contains(&cell.id) {
            return "awaiting_confirmation";
        }
        if self
            .cell_errors
            .get(&cell.id)
            .is_some_and(|error| !error.is_empty())
        {
            return "error";
        }
        if self.cell_output_live(cell) {
            return "live";
        }
        if self.cell_lazy.contains(&cell.id) {
            return "stale_lazy";
        }
        if cell.output.is_some() {
            "stale"

        } else {
            "idle"
        }

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
fn app_control_type_supported(ty: &crate::AST::Type) -> bool {
    match ty {
        crate::AST::Type::Int
        | crate::AST::Type::Float
        | crate::AST::Type::Bool
        | crate::AST::Type::String
        | crate::AST::Type::Char
        | crate::AST::Type::IntN { .. }
        | crate::AST::Type::Float32 => true,
        crate::AST::Type::List(inner)
        | crate::AST::Type::Option(inner)
        | crate::AST::Type::FixedList { elem: inner, .. }
        | crate::AST::Type::Tagged { inner, .. }
        | crate::AST::Type::InlineRange { base: inner, .. } => {
            app_control_type_supported(inner)
        }
        crate::AST::Type::Map { key, value, .. } => {
            app_control_type_supported(key) && app_control_type_supported(value)
        }
        crate::AST::Type::Tuple(fields) => fields
            .iter()
            .all(|(_, field)| app_control_type_supported(field)),
        crate::AST::Type::Shared(_)
        | crate::AST::Type::Result { .. }
        | crate::AST::Type::Fn { .. }
        | crate::AST::Type::Named(_)
        | crate::AST::Type::Apply { .. }
        | crate::AST::Type::TraitObject(_)
        | crate::AST::Type::Union(_)
        | crate::AST::Type::Quantity { .. }
        | crate::AST::Type::Measure(_) => false,
    }
}

fn app_effect_supported(effect: &str) -> bool {
    let root = effect
        .strip_prefix('!')
        .unwrap_or(effect)
        .split('.')
        .next()
        .unwrap_or_default();
    notebook_flags().allow.contains(root)
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

fn build_identity(environment_hash: &str) -> String {
    SHA256::sha256_hex(
        format!(
            "jet-notebook-build-v1|{}|{}",
            env!("CARGO_PKG_VERSION"),
            environment_hash
        )
        .as_bytes(),
    )
}

fn control_key(function: &str, name: &str) -> String {
    format!("{function}\0{name}")
}

fn control_value_matches(value: &str, type_name: &str) -> bool {
    let value = value.trim();
    let (tokens, diagnostics) = crate::Lexer::lex(value);
    if diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.severity,
            crate::Diagnostics::Severity::Error
        )
    }) {
        return false;
    }
    let first = tokens
        .iter()
        .find(|token| {
            !matches!(
                &token.kind,
                crate::Lexer::TokKind::Semi
                    | crate::Lexer::TokKind::Eof
                    | crate::Lexer::TokKind::LineComment(_)
                    | crate::Lexer::TokKind::BlockComment(_)
            )
        })
        .map(|token| &token.kind);
    match type_name {
        "Bool" => first.is_some_and(|kind| {
            matches!(
                kind,
                crate::Lexer::TokKind::KwTrue | crate::Lexer::TokKind::KwFalse
            )
        }),
        "String" => first.is_some_and(|kind| {
            matches!(
                kind,
                crate::Lexer::TokKind::Str(_) | crate::Lexer::TokKind::RawStr(_)
            )
        }),
        "Char" => first.is_some_and(|kind| matches!(kind, crate::Lexer::TokKind::Char(_))),
        "Float" | "F32" => first.is_some_and(|kind| {
            matches!(
                kind,
                crate::Lexer::TokKind::Float(..) | crate::Lexer::TokKind::Int(..)
            )
        }),
        "Int" | "I8" | "I16" | "I32" | "I64" | "I128" | "U8" | "U16" | "U32"
        | "U64" | "U128" => first.is_some_and(|kind| {
            matches!(kind, crate::Lexer::TokKind::Int(..))
        }),
        name if name.starts_with('?') => {
            value == "null" || control_value_matches(value, name.trim_start_matches('?'))
        }
        name if name.starts_with('[') || name.starts_with('(') => first.is_some_and(|kind| {
            matches!(
                (name.as_bytes().first(), kind),
                (Some(b'['), crate::Lexer::TokKind::LBracket)
                    | (Some(b'('), crate::Lexer::TokKind::LParen)
            )
        }),
        _ => first.is_some(),
    }
}

fn source_may_have_effect(source: &str) -> bool {
    [
        "print(",
        "println(",
        "eprint(",
        "io.",
        "fs.",
        "net.",
        "exec.",
        "write(",
        "save(",
        "stdin",
        "stdout",
        "stderr",
        "sleep(",
        "random(",
        "clock(",
    ]
    .iter()
    .any(|marker| source.contains(marker))
}

fn diagnostic_error(code: &str, what: &str, why: impl Into<String>, fix: impl Into<String>) -> String {
    format!(
        "{{\"code\":{},\"what\":{},\"why\":{},\"fix\":{}}}",
        json_str(code),
        json_str(what),
        json_str(&why.into()),
        json_str(&fix.into())
    )
}

fn bundle_for_eval(eval: &EvalResult) -> MimeBundle {
    // `EvalResult.text` is the canonical turn/event stream: it contains sink
    // output plus any echoed value or diagnostic. Keep it intact so a client
    // cannot lose a print event when the turn also carries a value.
    let text = match eval.text.trim_end() {
        "" => eval
            .value
            .as_ref()
            .filter(|value| !matches!(value, crate::Comptime::CtValue::Unit))
            .map(crate::display_value)
            .unwrap_or_default(),
        text => text.to_string(),
    };
    let mut mime = vec![("text/plain".to_string(), text.clone())];
    let trimmed = text.trim_start();
    if trimmed.starts_with("<svg") {
        mime.push(("image/svg+xml".to_string(), text.clone()));
    }
    let html_tag = trimmed
        .strip_prefix('<')
        .and_then(|rest| rest.split_once([' ', '>']).map(|(tag, _)| tag));
    if matches!(
        html_tag,
        Some("html" | "table" | "div" | "span" | "pre" | "section")
    ) {
        mime.push(("text/html".to_string(), text.clone()));
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

fn json_strings(values: &[String]) -> String {
    let items = values
        .iter()
        .map(|value| json_str(value))
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
