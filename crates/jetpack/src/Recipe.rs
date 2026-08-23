//! Build recipe substrate + sandbox (D-JPK-ADAPTER1=A safety contract).
//!
//! A `BuildRecipe` turns a staged source tree into a realized package under a
//! confined, auditable build. This is the internal substrate for the ad-hoc
//! adapter surface (`Recipe.*`, D-JPK-ADAPTNAME1 and D-JPK-BUILDRECIPE1);
//! callers build `BuildStep`s directly after the public finite recipe is
//! lowered.
//!
//! The safety contract (D-JPK-ADAPTER1=A), enforced structurally:
//! - **network** is denied except a locked, credential-free `fetch(url,
//!   sha256:)` — an unlocked or credentialed fetch is refused (`E1236`);
//! - **outputs** install only under the package output root — a step targeting
//!   a path outside it escapes confinement (`E1237`);
//! - **build tools** are realized `Pkg` deps, never host `/usr/bin` — an `exec`
//!   naming a tool that is not a realized dep is refused (`E1238`);
//! - **executable actions** run through the shared native child sandbox used by
//!   hermetic BuildPlan actions. Linux uses Bubblewrap, macOS uses Seatbelt,
//!   and Windows uses AppContainer; each gets a private source/output boundary,
//!   cleared environment, denied ambient network, and backend-owned policy
//!   receipt; unavailable enforcement is `E1275`;
//! - every `fetch`/`exec` records an **effect entry** so the build's provenance
//!   is a diff in `.jet/lock`;
//! - a **locked fetch** caches by content hash and is offline-satisfiable on a
//!   re-build (D-JPK-OFFLINE1).
//!
//! std-only (I6): the default transport reads `file://` sources; a remote
//! transport is injected by a caller that already holds network capability, so
//! the compiler seam stays zero-external-crate.

use crate::Diagnostics::Diagnostic;
use crate::SHA256;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

// Card #367 slice 4: the `BuildStep`/`BuildRecipe` *data* shape sunk into
// `jet-pkg-model` (data-down / engine-up) — re-exported under the historical
// `crate::Recipe::{BuildStep,BuildRecipe}` path so every call site here and
// in `Provider.rs`/`ModuleEval::Types` is unchanged.
pub use jet_pkg_model::Recipe::{
    BuildPlanFragment, BuildRecipe, BuildStep, PlanAuthority, PlanFragmentAction, PlanInput,
    StagedPlanAction, StagedPlanError, StagedPlanLock,
};

static STAGED_PLAN_ARTIFACT_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A locked source fetch recorded for `.jet/lock` provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchRecord {
    pub url: String,
    pub sha256: String,
}

/// What a recipe run needs, and where its confinement boundaries are.
pub struct BuildContext<'a> {
    /// The staged source tree (read-only by contract).
    pub source_dir: &'a Path,
    /// The single writable install target. Every `install` dest resolves here.
    pub output_root: &'a Path,
    /// Realized `Pkg` build-tool deps: tool name → executable path. An `exec`
    /// step may only name a tool present here (`E1238`).
    pub tools: HashMap<String, PathBuf>,
    /// Where locked fetches cache their bytes (keyed by sha256), so a re-build
    /// is offline-satisfiable.
    pub fetch_cache: &'a Path,
    /// `--offline`: no would-be network fetch may touch the wire.
    pub offline: bool,
}

/// A remote transport a caller injects when it holds network capability. Given a
/// URL, returns the bytes. The compiler seam never supplies one for remote
/// schemes (I6); `file://` is handled by the default reader before this is
/// consulted.
pub type Transport<'a> = &'a dyn Fn(&str) -> Result<Vec<u8>, String>;

/// The provenance a recipe run produced: locked fetches + the effect vocabulary
/// the build exercised. Both flow into `.jet/lock` (D-JPK-ADAPTER1 / D-EFFBUDGET1).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunReport {
    pub fetches: Vec<FetchRecord>,
    /// Sorted, de-duplicated effect names (`net.fetch`, `exec:<tool>`, `write`).
    pub effects: Vec<String>,
    /// Fingerprint of the canonical finite action graph that admitted this
    /// run. A successful report never comes from the raw recipe loop alone.
    pub plan_fingerprint: String,
    /// Actual child backend used by executable recipe steps, or
    /// `non-executing` when the recipe needed no child process.
    pub sandbox_class: String,
    /// Backend-owned filesystem/process/network/environment/device/resource
    /// policy receipt.
    pub sandbox_policy: String,
}

impl RunReport {
    fn add_effect(&mut self, e: &str) {
        if !self.effects.iter().any(|x| x == e) {
            self.effects.push(e.to_string());
            self.effects.sort();
        }
    }

    fn record_sandbox(&mut self, class: &str, policy: &str) {
        if self.sandbox_class == "non-executing" {
            self.sandbox_class = class.to_string();
            self.sandbox_policy = policy.to_string();
        }
        debug_assert_eq!(self.sandbox_class, class);
        debug_assert_eq!(self.sandbox_policy, policy);
    }
}

fn run_report() -> RunReport {
    RunReport {
        sandbox_class: "non-executing".to_string(),
        sandbox_policy: "no child launched".to_string(),
        ..RunReport::default()
    }
}

/// The only host state exposed while a staged plan action emits its fragment.
/// The callback has no filesystem handle, store handle, resolver, or ambient
/// environment; it can read only the exact locked inputs through `read_input`.
pub struct StagedPlanContext<'a> {
    pub source_dir: &'a Path,
    pub artifact_root: &'a Path,
}

/// A successful staged plan publication. The artifact directory is committed
/// only after the fragment passes model validation and the complete BuildPlan
/// graph is admitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedPlanReport {
    pub action_identity: String,
    pub fragment_digest: String,
    pub plan_fingerprint: String,
    pub artifact_dir: PathBuf,
    pub lock: StagedPlanLock,
}

/// A plan callback can fail or be cancelled without publishing a partial
/// fragment. Access outside the declared input authority is a separate error
/// so diagnostics can name the violated sandbox rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StagedPlanActionError {
    Cancelled,
    Failed(String),
    UndeclaredAccess(String),
}

impl std::fmt::Display for StagedPlanActionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => f.write_str("staged plan action was cancelled"),
            Self::Failed(detail) => f.write_str(detail),
            Self::UndeclaredAccess(detail) => write!(f, "undeclared staged-plan access: {detail}"),
        }
    }
}

impl std::error::Error for StagedPlanActionError {}

/// Capability-limited input view for the plan callback.
pub struct PlanSandbox<'a> {
    source_dir: &'a Path,
    declared_inputs: BTreeMap<String, String>,
    observed_inputs: BTreeSet<String>,
}

impl<'a> PlanSandbox<'a> {
    fn new(source_dir: &'a Path, inputs: &[PlanInput]) -> Self {
        Self {
            source_dir,
            declared_inputs: inputs
                .iter()
                .map(|input| (input.path.clone(), input.digest.clone()))
                .collect(),
            observed_inputs: BTreeSet::new(),
        }
    }

    /// Read one exact, declared input and verify its locked digest before
    /// exposing bytes to the callback.
    pub fn read_input(&mut self, path: &str) -> Result<Vec<u8>, StagedPlanActionError> {
        let Some(declared_digest) = self.declared_inputs.get(path) else {
            return Err(StagedPlanActionError::UndeclaredAccess(format!(
                "input `{path}`"
            )));
        };
        let source = confined_source(self.source_dir, path, false).map_err(|error| {
            StagedPlanActionError::Failed(format!("{}: {}", error.code, error.what))
        })?;
        let bytes = std::fs::read(&source).map_err(|error| {
            StagedPlanActionError::Failed(format!(
                "could not read declared input `{path}`: {error}"
            ))
        })?;
        let digest = SHA256::sha256_hex(&bytes);
        if !digest_matches(declared_digest, &digest) {
            return Err(StagedPlanActionError::Failed(format!(
                "declared input `{path}` digest mismatch: expected `{declared_digest}`, got `sha256-{digest}`"
            )));
        }
        self.observed_inputs.insert(path.to_string());
        Ok(bytes)
    }

    /// Store reads are not part of D-JPK-DYNAMICPLAN1's sandbox authority.
    pub fn read_store(&self, path: &str) -> Result<Vec<u8>, StagedPlanActionError> {
        Err(StagedPlanActionError::UndeclaredAccess(format!(
            "store path `{path}`"
        )))
    }

    /// Package resolution is deliberately unavailable after the finite stage
    /// boundary; all resolution facts must be declared by the caller.
    pub fn resolve_package(&self, package: &str) -> Result<(), StagedPlanActionError> {
        Err(StagedPlanActionError::UndeclaredAccess(format!(
            "package resolution `{package}`"
        )))
    }

    pub fn observed_inputs(&self) -> impl Iterator<Item = &str> {
        self.observed_inputs.iter().map(String::as_str)
    }

    fn verify_declared_inputs(&mut self) -> Result<(), StagedPlanActionError> {
        let paths = self.declared_inputs.keys().cloned().collect::<Vec<_>>();
        for path in paths {
            self.read_input(&path)?;
        }
        Ok(())
    }
}

/// Validate a recipe against the safety contract without running it — the read
/// path `jet inspect audit` uses (D-BUILDSCOPE1: audit never executes). Returns the
/// first violation as a diagnostic.
pub fn validate(recipe: &BuildRecipe, ctx: &BuildContext) -> Result<(), Diagnostic> {
    validate_finite_recipe(recipe, "pkg")?;
    for step in &recipe.steps {
        match step {
            BuildStep::Fetch { url, sha256 } => {
                if credentialed_fetch_url(url) {
                    return Err(e1236_credentialed_url());
                }
                if !valid_sha256(sha256) {
                    return Err(e1236_invalid_hash(url, sha256));
                }
            }
            BuildStep::Exec { tool, .. } => {
                realized_tool(&ctx.tools, tool)?;
            }
            BuildStep::Install { src, dest } => {
                confined_source(ctx.source_dir, src, false)?;
                confined_dest(ctx.output_root, dest)?;
            }
            BuildStep::InstallTree { src, dest } => {
                confined_source(ctx.source_dir, src, true)?;
                confined_dest(ctx.output_root, dest)?;
            }
        }
    }
    Ok(())
}

/// Lower a `BuildRecipe` into the one executable BuildPlan IR (E4-JP2 / #419).
///
/// Fetch / exec / install / install-tree steps become typed actions under one
/// package target. Toolchain, capability, and env allowlists are declared on
/// each action so `ActionKey` is the complete CAS identity. Callers still run
/// parser/sema/policy/diagnostics before any cache lookup.
pub fn lower_to_plan(
    recipe: &BuildRecipe,
    package: &str,
    tools: &HashMap<String, PathBuf>,
) -> Result<crate::Comptime::Build::BuildPlan, Diagnostic> {
    use crate::Comptime::Build::{
        ActionKind, ActionSpec, BuildCapability, BuildContext as PlanContext, TargetSpec,
    };

    validate_finite_recipe(recipe, package)?;

    let mut plan_ctx = PlanContext::new();
    let mut action_handles = Vec::new();
    for (idx, step) in recipe.steps.iter().enumerate() {
        let name = format!("recipe-{package}-{idx}");
        let authority_effect = match step {
            BuildStep::Fetch { .. } => "net.fetch".to_string(),
            BuildStep::Exec { tool, .. } => format!("exec:{tool}"),
            BuildStep::Install { .. } | BuildStep::InstallTree { .. } => "fs.write".to_string(),
        };
        let spec = match step {
            BuildStep::Fetch { url, sha256 } => {
                if credentialed_fetch_url(url) {
                    return Err(e1236_credentialed_url());
                }
                ActionSpec::cached(["jet-fetch", url.as_str(), sha256.as_str()])
                    .with_kind(ActionKind::Generic)
                    .with_inputs(["."])
                    .with_cap(BuildCapability::Net)
                    .with_env("SOURCE_DATE_EPOCH", "0")
                    .with_env_allowlist(["SOURCE_DATE_EPOCH"])
                    .with_helper_version("jet-fetch", env!("CARGO_PKG_VERSION"))
                    .with_label("recipe.step", "fetch")
                    .with_label("fetch.url", url.clone())
                    .with_label("fetch.sha256", sha256.clone())
            }
            BuildStep::Exec { tool, args } => {
                let tool_path = realized_tool(tools, tool)?;
                let mut argv = vec![tool_path.to_string_lossy().into_owned()];
                argv.extend(args.iter().cloned());
                ActionSpec::cached(argv)
                    .with_kind(ActionKind::Compile)
                    .with_inputs(["."])
                    .with_cap(BuildCapability::Exec)
                    .with_env("SOURCE_DATE_EPOCH", "0")
                    .with_env("JET_PROFILE", "default")
                    .with_env_allowlist(["SOURCE_DATE_EPOCH", "JET_PROFILE"])
                    .with_helper_version(tool, "declared")
                    .with_label("recipe.step", "exec")
                    .with_label("recipe.tool", tool.clone())
            }
            BuildStep::Install { src, dest } => {
                ActionSpec::cached(["jet-install", src.as_str(), dest.as_str()])
                    .with_kind(ActionKind::SourceArchive)
                    .with_inputs([src.clone()])
                    .with_cap(BuildCapability::FS)
                    .with_env("SOURCE_DATE_EPOCH", "0")
                    .with_env_allowlist(["SOURCE_DATE_EPOCH"])
                    .with_helper_version("jet-install", env!("CARGO_PKG_VERSION"))
                    .with_label("recipe.step", "install")
                    .with_label("install.dest", dest.clone())
            }
            BuildStep::InstallTree { src, dest } => {
                ActionSpec::cached(["jet-install-tree", src.as_str(), dest.as_str()])
                    .with_kind(ActionKind::SourceArchive)
                    .with_inputs([src.clone()])
                    .with_cap(BuildCapability::FS)
                    .with_env("SOURCE_DATE_EPOCH", "0")
                    .with_env_allowlist(["SOURCE_DATE_EPOCH"])
                    .with_helper_version("jet-install-tree", env!("CARGO_PKG_VERSION"))
                    .with_label("recipe.step", "install-tree")
                    .with_label("install.dest", dest.clone())
            }
        };
        let mut outputs = vec![step_marker(package, idx)];
        if let BuildStep::Install { dest, .. } | BuildStep::InstallTree { dest, .. } = step {
            outputs.push(declared_output_path(package, dest)?);
        }
        let spec = if idx == 0 {
            spec.with_outputs(outputs)
        } else {
            spec.with_outputs(outputs)
                .with_inputs([step_marker(package, idx - 1)])
        }
        .with_label("stage.index", idx.to_string())
        .with_label("stage.bound", recipe.steps.len().to_string())
        .with_label("authority.effect", authority_effect)
        .with_label("platform.os", std::env::consts::OS)
        .with_label("platform.arch", std::env::consts::ARCH);
        let handle = plan_ctx.action(name, spec).map_err(|err| {
            Diagnostic::error(
                "E1238",
                "recipe step could not lower into BuildPlan".to_string(),
                format!("{err:?}"),
                "fix the recipe step so it declares a valid action.".to_string(),
                None,
            )
        })?;
        action_handles.push(handle);
    }

    let mut package_spec = TargetSpec::new().with_metadata("profile", "default");
    for handle in action_handles {
        package_spec = package_spec.with_action(handle);
    }
    let package_target = plan_ctx.add_package(package, package_spec).map_err(|err| {
        Diagnostic::error(
            "E1238",
            format!("recipe package `{package}` could not lower into BuildPlan"),
            format!("{err:?}"),
            "choose a unique package name for the recipe target.".to_string(),
            None,
        )
    })?;
    plan_ctx.plan_with_default(package_target).map_err(|err| {
        Diagnostic::error(
            "E1238",
            "recipe BuildPlan failed validation".to_string(),
            format!("{err:?}"),
            "resolve duplicate outputs or empty steps before caching.".to_string(),
            None,
        )
    })
}

/// Run one finite sandboxed plan action, lower its typed fragment into the
/// ordinary BuildPlan graph, and atomically publish the resulting stage.
///
/// The callback is the only dynamic part. It can inspect declared inputs but
/// cannot resolve packages, read the store, inherit ambient state, or publish
/// an artifact. Validation and graph admission happen before publication.
pub fn run_staged_plan_action<F>(
    action: &StagedPlanAction,
    ctx: &StagedPlanContext<'_>,
    tools: &HashMap<String, PathBuf>,
    emit: F,
) -> Result<StagedPlanReport, Diagnostic>
where
    F: FnOnce(&mut PlanSandbox<'_>) -> Result<BuildPlanFragment, StagedPlanActionError>,
{
    action
        .validate_declaration()
        .map_err(staged_plan_model_error)?;
    let mut sandbox = PlanSandbox::new(ctx.source_dir, &action.inputs);
    let fragment = emit(&mut sandbox).map_err(staged_plan_action_error)?;
    sandbox
        .verify_declared_inputs()
        .map_err(staged_plan_action_error)?;
    action
        .validate(&fragment)
        .map_err(staged_plan_model_error)?;
    let identity = action
        .identity(&fragment)
        .map_err(staged_plan_model_error)?;
    let fragment_digest = action
        .fragment_digest(&fragment)
        .map_err(staged_plan_model_error)?;
    let plan = lower_staged_plan_action(action, &fragment, tools)?;
    let plan_fingerprint = plan_recipe_fingerprint(&plan)?;
    let lock = action.lock(&fragment).map_err(staged_plan_model_error)?;
    let artifact_dir = publish_staged_plan_artifact(
        ctx.artifact_root,
        action,
        &fragment,
        &lock,
        &plan_fingerprint,
    )?;
    Ok(StagedPlanReport {
        action_identity: identity,
        fragment_digest,
        plan_fingerprint,
        artifact_dir,
        lock,
    })
}

/// Lower a validated staged fragment into the same finite action graph used by
/// ordinary package recipes. The planner node owns the canonical fragment;
/// every emitted action depends on it and on its declared predecessor markers.
pub fn lower_staged_plan_action(
    action: &StagedPlanAction,
    fragment: &BuildPlanFragment,
    tools: &HashMap<String, PathBuf>,
) -> Result<crate::Comptime::Build::BuildPlan, Diagnostic> {
    use crate::Comptime::Build::{ActionKind, ActionSpec, BuildContext as PlanContext, TargetSpec};

    action.validate(fragment).map_err(staged_plan_model_error)?;
    let identity = action.identity(fragment).map_err(staged_plan_model_error)?;
    let fragment_digest = action
        .fragment_digest(fragment)
        .map_err(staged_plan_model_error)?;
    let namespace = staged_plan_namespace(&action.name, &identity);
    let plan_output = format!(".jet/staged-plan/{namespace}/fragment.plan");

    let mut ordered = fragment.actions.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| left.name.cmp(&right.name));
    let marker_paths = ordered
        .iter()
        .enumerate()
        .map(|(index, emitted)| {
            (
                emitted.name.clone(),
                format!(".jet/staged-plan/{namespace}/actions/{index}.stamp"),
            )
        })
        .collect::<BTreeMap<_, _>>();
    for emitted in &ordered {
        for output in &emitted.outputs {
            if output_paths_overlap(output, &plan_output)
                || marker_paths
                    .values()
                    .any(|marker| output_paths_overlap(output, marker))
            {
                return Err(e1238_plan(&format!(
                    "staged action `{}` output `{output}` overlaps the reserved staged-plan graph",
                    emitted.name
                )));
            }
        }
    }

    let mut plan_ctx = PlanContext::new();
    let mut planner_spec =
        ActionSpec::cached(["jet-staged-plan", action.name.as_str(), identity.as_str()])
            .with_kind(ActionKind::Generic)
            .with_outputs([plan_output.clone()])
            .with_env("STAGED_PLAN_STAGE", action.stage.to_string())
            .with_env("STAGED_PLAN_BOUND", action.stage_bound.to_string())
            .with_env("STAGED_PLAN_PLATFORM", action.authority.platform.clone())
            .with_env_allowlist([
                "STAGED_PLAN_STAGE",
                "STAGED_PLAN_BOUND",
                "STAGED_PLAN_PLATFORM",
            ])
            .with_helper_version("jet-staged-plan", jet_pkg_model::Recipe::STAGED_PLAN_FORMAT)
            .with_label("staged.action", action.name.clone())
            .with_label("staged.identity", identity.clone())
            .with_label("staged.fragment-digest", fragment_digest.clone())
            .with_label("staged.stage", action.stage.to_string())
            .with_label("staged.stage-bound", action.stage_bound.to_string())
            .with_label("staged.platform", action.authority.platform.clone());
    let mut planner_inputs = action.inputs.clone();
    planner_inputs.sort();
    for (index, input) in planner_inputs.iter().enumerate() {
        planner_spec = planner_spec
            .with_inputs([input.path.clone()])
            .with_label(format!("staged.input.{index}.path"), input.path.clone())
            .with_label(format!("staged.input.{index}.digest"), input.digest.clone());
    }
    let mut authority_tools = action.authority.tools.clone();
    authority_tools.sort();
    for (index, tool) in authority_tools.iter().enumerate() {
        let path = realized_tool(tools, tool)?;
        planner_spec = planner_spec.with_label(
            format!("authority.tool.{index}.path"),
            path.to_string_lossy().into_owned(),
        );
    }
    planner_spec = add_authority_labels(
        planner_spec,
        &action.authority.tools,
        &action.authority.effects,
    );
    for effect in &action.authority.effects {
        planner_spec = planner_spec.with_cap(*effect);
    }
    let planner_handle = plan_ctx
        .action(format!("staged-plan-{namespace}-planner"), planner_spec)
        .map_err(|error| e1238_plan(&format!("staged planner action is invalid: {error:?}")))?;

    let mut action_handles = vec![planner_handle];
    for (index, emitted) in ordered.iter().enumerate() {
        let tool_path = realized_tool(tools, &emitted.tool)?;
        let mut argv = vec![tool_path.to_string_lossy().into_owned()];
        argv.extend(emitted.args.iter().cloned());
        let marker = marker_paths
            .get(&emitted.name)
            .expect("marker was created for every validated staged action");
        let mut inputs = BTreeSet::new();
        inputs.insert(plan_output.clone());
        for input in &emitted.inputs {
            inputs.insert(input.clone());
        }
        for dependency in &emitted.dependencies {
            inputs.insert(
                marker_paths
                    .get(dependency)
                    .expect("dependencies were validated before lowering")
                    .clone(),
            );
        }
        let mut outputs = emitted.outputs.clone();
        outputs.sort();
        outputs.push(marker.clone());
        let mut spec = ActionSpec::cached(argv)
            .with_kind(ActionKind::Compile)
            .with_inputs(inputs)
            .with_outputs(outputs)
            .with_helper_version(&emitted.tool, "declared")
            .with_label("staged.action", action.name.clone())
            .with_label("staged.identity", identity.clone())
            .with_label("staged.fragment-digest", fragment_digest.clone())
            .with_label("staged.fragment-name", emitted.name.clone())
            .with_label("staged.fragment-index", index.to_string())
            .with_label("staged.tool", emitted.tool.clone())
            .with_label("staged.platform", emitted.platform.clone())
            .with_label("staged.stage", action.stage.to_string())
            .with_label("staged.stage-bound", action.stage_bound.to_string());
        for (key, value) in &emitted.env {
            spec = spec.with_env(key.clone(), value.clone());
        }
        spec = spec.with_env_allowlist(emitted.env.keys().cloned());
        for effect in &emitted.effects {
            spec = spec.with_cap(*effect);
        }
        let mut emitted_input_facts = action
            .inputs
            .iter()
            .filter(|input| emitted.inputs.iter().any(|path| path == &input.path))
            .collect::<Vec<_>>();
        emitted_input_facts.sort_by(|left, right| left.path.cmp(&right.path));
        for (input_index, input) in emitted_input_facts.iter().enumerate() {
            spec = spec
                .with_label(
                    format!("staged.input.{input_index}.path"),
                    input.path.clone(),
                )
                .with_label(
                    format!("staged.input.{input_index}.digest"),
                    input.digest.clone(),
                );
        }
        let mut authority_tools = action.authority.tools.clone();
        authority_tools.sort();
        for (tool_index, tool) in authority_tools.iter().enumerate() {
            let path = realized_tool(tools, tool)?;
            spec = spec.with_label(
                format!("authority.tool.{tool_index}.path"),
                path.to_string_lossy().into_owned(),
            );
        }
        spec = add_authority_labels(spec, &action.authority.tools, &action.authority.effects);
        let handle = plan_ctx
            .action(format!("staged-plan-{namespace}-action-{index}"), spec)
            .map_err(|error| {
                e1238_plan(&format!(
                    "staged emitted action `{}` is invalid: {error:?}",
                    emitted.name
                ))
            })?;
        action_handles.push(handle);
    }

    let mut target_spec = TargetSpec::new()
        .with_metadata("staged.action", action.name.clone())
        .with_metadata("staged.identity", identity)
        .with_metadata("staged.fragment-digest", fragment_digest)
        .with_metadata("staged.platform", action.authority.platform.clone())
        .with_metadata("staged.stage", action.stage.to_string())
        .with_metadata("staged.stage-bound", action.stage_bound.to_string());
    for handle in action_handles {
        target_spec = target_spec.with_action(handle);
    }
    let target = plan_ctx
        .add_package(format!("staged-plan-{namespace}"), target_spec)
        .map_err(|error| e1238_plan(&format!("staged plan target is invalid: {error:?}")))?;
    plan_ctx
        .plan_with_default(target)
        .map_err(|error| e1238_plan(&format!("staged plan graph failed validation: {error:?}")))
}

fn add_authority_labels(
    mut spec: crate::Comptime::Build::ActionSpec,
    tools: &[String],
    effects: &[jet_foundation::BuildEffect],
) -> crate::Comptime::Build::ActionSpec {
    let mut tools = tools.to_vec();
    tools.sort();
    for (index, tool) in tools.iter().enumerate() {
        spec = spec.with_label(format!("authority.tool.{index}"), tool.clone());
    }
    let mut effects = effects.to_vec();
    effects.sort();
    for (index, effect) in effects.iter().enumerate() {
        spec = spec.with_label(format!("authority.effect.{index}"), effect.flag());
    }
    spec
}

fn staged_plan_model_error(error: StagedPlanError) -> Diagnostic {
    e1238_plan(&format!("{error}"))
}

fn staged_plan_action_error(error: StagedPlanActionError) -> Diagnostic {
    match error {
        StagedPlanActionError::Cancelled => Diagnostic::error(
            "E1238",
            "staged plan action was cancelled".to_string(),
            "the finite stage produced no publishable fragment".to_string(),
            "retry the build; cancellation leaves the previous artifact unchanged.".to_string(),
            None,
        ),
        StagedPlanActionError::Failed(detail) => e1238_plan(&format!(
            "staged plan action failed before publication: {detail}"
        )),
        StagedPlanActionError::UndeclaredAccess(detail) => {
            e1238_plan(&format!("staged plan action denied {detail}"))
        }
    }
}

fn staged_plan_namespace(name: &str, identity: &str) -> String {
    let digest = identity
        .rsplit(':')
        .next()
        .unwrap_or(identity)
        .chars()
        .take(16)
        .collect::<String>();
    format!("{}-{digest}", safe_plan_component(name))
}

fn safe_plan_component(value: &str) -> String {
    let mut output = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if output.is_empty() {
        output.push_str("action");
    }
    output
}

fn publish_staged_plan_artifact(
    artifact_root: &Path,
    action: &StagedPlanAction,
    fragment: &BuildPlanFragment,
    lock: &StagedPlanLock,
    plan_fingerprint: &str,
) -> Result<PathBuf, Diagnostic> {
    std::fs::create_dir_all(artifact_root)
        .map_err(|error| recipe_io_error("could not create staged-plan artifact root", error))?;
    let namespace = staged_plan_namespace(&action.name, &lock.action_identity);
    let artifact = artifact_root.join(namespace);
    let fragment_bytes = fragment.canonical_bytes();
    let lock_bytes = lock.encode();
    if artifact.exists() {
        let matches = std::fs::read(artifact.join("fragment.plan"))
            .map(|bytes| bytes == fragment_bytes)
            .unwrap_or(false)
            && std::fs::read_to_string(artifact.join("lock"))
                .map(|contents| contents == lock_bytes)
                .unwrap_or(false)
            && std::fs::read_to_string(artifact.join("plan.fingerprint"))
                .map(|contents| contents == plan_fingerprint)
                .unwrap_or(false);
        if matches {
            return Ok(artifact);
        }
        return Err(e1238_plan(
            "staged-plan artifact identity collides with a different published result",
        ));
    }

    let counter = STAGED_PLAN_ARTIFACT_COUNTER.fetch_add(1, Ordering::Relaxed);
    let staging = artifact_root.join(format!(".staged-plan-{}-{counter}", std::process::id()));
    let mut guard = StagedPlanArtifactGuard {
        path: staging.clone(),
        published: false,
    };
    std::fs::create_dir(&staging)
        .map_err(|error| recipe_io_error("could not create staged-plan scratch", error))?;
    std::fs::write(staging.join("fragment.plan"), fragment_bytes)
        .map_err(|error| recipe_io_error("could not write staged-plan fragment", error))?;
    std::fs::write(staging.join("lock"), lock_bytes)
        .map_err(|error| recipe_io_error("could not write staged-plan lock", error))?;
    std::fs::write(staging.join("plan.fingerprint"), plan_fingerprint)
        .map_err(|error| recipe_io_error("could not write staged-plan fingerprint", error))?;
    if let Err(error) = std::fs::rename(&staging, &artifact) {
        return Err(recipe_io_error(
            "could not publish staged-plan artifact",
            error,
        ));
    }
    guard.published = true;
    Ok(artifact)
}

struct StagedPlanArtifactGuard {
    path: PathBuf,
    published: bool,
}

impl Drop for StagedPlanArtifactGuard {
    fn drop(&mut self) {
        if !self.published {
            remove_path(&self.path);
        }
    }
}

fn step_marker(package: &str, index: usize) -> String {
    format!(".jet/recipe/{package}/step-{index}.stamp")
}

fn validate_finite_recipe(recipe: &BuildRecipe, package: &str) -> Result<(), Diagnostic> {
    if package.is_empty()
        || package == "."
        || package == ".."
        || package.chars().any(char::is_control)
        || package.contains('/')
        || package.contains('\\')
    {
        return Err(e1238_plan(
            "recipe package name is not one safe path segment",
        ));
    }
    if recipe.steps.is_empty() {
        return Err(e1238_plan(
            "recipe must declare at least one finite staged action",
        ));
    }

    let mut declared_outputs = Vec::<(String, String)>::new();
    for (index, step) in recipe.steps.iter().enumerate() {
        let (dest, is_tree) = match step {
            BuildStep::Install { dest, .. } => (dest.as_str(), false),
            BuildStep::InstallTree { dest, .. } => (dest.as_str(), true),
            BuildStep::Fetch { .. } | BuildStep::Exec { .. } => continue,
        };
        let output = declared_output_path(package, dest)?;
        let root = normalize(&PathBuf::from(format!(".jet/recipe/{package}/outputs")));
        if !is_tree && Path::new(&output) == root {
            return Err(e1238_plan(&format!(
                "file install at recipe step {index} must declare a file below the output root"
            )));
        }
        let step_name = format!("recipe-{package}-{index}");
        if let Some((previous, previous_step)) = declared_outputs
            .iter()
            .find(|(previous, _)| output_paths_overlap(previous, &output))
        {
            return Err(e1238_output_conflict(
                previous,
                previous_step,
                &output,
                &step_name,
            ));
        }
        declared_outputs.push((output, step_name));
    }
    Ok(())
}

fn declared_output_path(package: &str, dest: &str) -> Result<String, Diagnostic> {
    let relative = Path::new(dest);
    if dest.is_empty()
        || dest.chars().any(char::is_control)
        || relative.is_absolute()
        || relative
            .components()
            .any(|component| component == std::path::Component::ParentDir)
    {
        return Err(e1237(dest));
    }
    let root = PathBuf::from(format!(".jet/recipe/{package}/outputs"));
    let normalized = normalize(&root.join(relative));
    if !normalized.starts_with(&normalize(&root)) {
        return Err(e1237(dest));
    }
    normalized
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| e1238_plan("recipe output path is not valid UTF-8"))
}

fn output_paths_overlap(left: &str, right: &str) -> bool {
    left == right
        || left
            .strip_prefix(right)
            .is_some_and(|suffix| suffix.starts_with(std::path::MAIN_SEPARATOR))
        || right
            .strip_prefix(left)
            .is_some_and(|suffix| suffix.starts_with(std::path::MAIN_SEPARATOR))
}

fn e1238_plan(detail: &str) -> Diagnostic {
    Diagnostic::error(
        "E1238",
        "build recipe or staged plan action is not a valid declared action graph".to_string(),
        format!(
            "{detail}; staged plans use only declared inputs, realized tools, effects, platform facts, and finite acyclic outputs"
        ),
        "declare every authority fact, fix the graph, and retry the stage.".to_string(),
        None,
    )
}

fn e1238_output_conflict(
    previous: &str,
    previous_step: &str,
    output: &str,
    step: &str,
) -> Diagnostic {
    e1238_plan(&format!(
        "recipe output `{output}` overlaps output `{previous}` owned by `{previous_step}`; `{step}` cannot claim the same staged path"
    ))
}

/// Hangar cache-identity hook: recipe fingerprint is the complete plan key
/// (E4-JP2). Does not redesign Store ingest — only produces the fingerprint
/// string consumers already write into `CacheIdentity.recipe_fingerprint`.
pub fn plan_recipe_fingerprint(
    plan: &crate::Comptime::Build::BuildPlan,
) -> Result<String, Diagnostic> {
    plan.execution_model().map_err(|err| {
        Diagnostic::error(
            "E1238",
            "could not admit lowered BuildPlan execution graph".to_string(),
            format!("{err:?}"),
            "remove cyclic or incomplete stage dependencies before caching.".to_string(),
            None,
        )
    })?;
    plan.complete_recipe_fingerprint().map_err(|err| {
        Diagnostic::error(
            "E1238",
            "could not fingerprint lowered BuildPlan".to_string(),
            format!("{err:?}"),
            "ensure every recipe action is present in the plan.".to_string(),
            None,
        )
    })
}

fn admitted_step_order(
    plan: &crate::Comptime::Build::BuildPlan,
    step_count: usize,
) -> Result<Vec<usize>, Diagnostic> {
    let model = plan.execution_model().map_err(|err| {
        Diagnostic::error(
            "E1238",
            "recipe action graph is not finite and acyclic".to_string(),
            format!("{err:?}"),
            "remove cyclic or incomplete stage dependencies before execution.".to_string(),
            None,
        )
    })?;
    let mut seen = vec![false; step_count];
    let mut order = Vec::with_capacity(step_count);
    for stage in &model.stages {
        if stage.actions.len() != 1 {
            return Err(e1238_plan(
                "recipe stage graph must admit exactly one action per finite stage",
            ));
        }
        let action_id = stage.actions[0];
        let action = plan
            .actions()
            .get(action_id.0)
            .ok_or_else(|| e1238_plan("recipe stage graph refers to an unknown action"))?;
        let index = action
            .labels
            .get("stage.index")
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or_else(|| e1238_plan("recipe action is missing its finite stage index"))?;
        let bound = action
            .labels
            .get("stage.bound")
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or_else(|| e1238_plan("recipe action is missing its finite stage bound"))?;
        if bound != step_count || index != stage.index || index >= step_count {
            return Err(e1238_plan(
                "recipe action stage facts do not match the finite recipe bound",
            ));
        }
        if seen[index] {
            return Err(e1238_plan("recipe stage graph repeats one action identity"));
        }
        seen[index] = true;
        order.push(index);
    }
    if order.len() != step_count || seen.iter().any(|present| !present) {
        return Err(e1238_plan(
            "recipe stage graph does not cover every declared recipe action",
        ));
    }
    Ok(order)
}

/// Run a recipe under the sandbox. Validates first (so a violation never gets to
/// execute), then performs each step. Returns the build provenance on success.
pub fn run(
    recipe: &BuildRecipe,
    ctx: &BuildContext,
    transport: Option<Transport>,
) -> Result<RunReport, Diagnostic> {
    validate(recipe, ctx)?;
    // E4-JP2: every recipe run lowers through the one BuildPlan IR before
    // sandbox steps execute (cache identity consumers use the fingerprint).
    let plan = lower_to_plan(recipe, "pkg", &ctx.tools)?;
    let plan_fingerprint = plan_recipe_fingerprint(&plan)?;
    let order = admitted_step_order(&plan, recipe.steps.len())?;
    let staged = PrivateStage::new(ctx.output_root);
    let staged_ctx = BuildContext {
        source_dir: ctx.source_dir,
        output_root: staged.path(),
        tools: ctx.tools.clone(),
        fetch_cache: ctx.fetch_cache,
        offline: ctx.offline,
    };
    let mut report = run_steps(recipe, &order, &staged_ctx, transport)?;
    staged.publish(ctx.output_root)?;
    report.plan_fingerprint = plan_fingerprint;
    Ok(report)
}

struct PrivateStage {
    path: PathBuf,
    published: bool,
}

impl PrivateStage {
    fn new(output_root: &Path) -> Self {
        Self {
            path: staged_output_path(output_root),
            published: false,
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn publish(mut self, output_root: &Path) -> Result<(), Diagnostic> {
        commit_staged_output(&self.path, output_root)?;
        self.published = true;
        Ok(())
    }
}

impl Drop for PrivateStage {
    fn drop(&mut self) {
        if !self.published {
            remove_path(&self.path);
        }
    }
}

/// Run with U27 step logging. Used by adapter/core build paths that need
/// `jet logs`, `jet explain`, and preserved failed scratch.
pub fn run_logged(
    recipe: &BuildRecipe,
    ctx: &BuildContext,
    transport: Option<Transport>,
    attempt: &mut super::BuildDebug::Attempt,
) -> Result<RunReport, Diagnostic> {
    validate(recipe, ctx)?;
    let plan = lower_to_plan(recipe, "pkg", &ctx.tools)?;
    let plan_fingerprint = plan_recipe_fingerprint(&plan)?;
    let order = admitted_step_order(&plan, recipe.steps.len())?;
    let staged = PrivateStage::new(ctx.output_root);
    let staged_ctx = BuildContext {
        source_dir: ctx.source_dir,
        output_root: staged.path(),
        tools: ctx.tools.clone(),
        fetch_cache: ctx.fetch_cache,
        offline: ctx.offline,
    };
    let mut report = run_report();
    let total = recipe.steps.len();
    let result = (|| {
        std::fs::create_dir_all(staged_ctx.output_root)
            .map_err(|error| recipe_io_error("could not create staged recipe output", error))?;
        for (position, step_index) in order.iter().enumerate() {
            let step = recipe
                .steps
                .get(*step_index)
                .ok_or_else(|| e1238_plan("admitted stage refers to a missing recipe step"))?;
            let index = position + 1;
            let step_result = match step {
                BuildStep::Fetch { url, sha256 } => {
                    do_fetch(url, sha256, &staged_ctx, transport, &mut report)
                }
                BuildStep::Exec { tool, args } => {
                    do_exec_logged(tool, args, &staged_ctx, &mut report)
                }
                BuildStep::Install { src, dest } => {
                    install_file(src, dest, &staged_ctx, &mut report)
                }
                BuildStep::InstallTree { src, dest } => {
                    install_tree(src, dest, &staged_ctx, &mut report)
                }
            };
            match step_result {
                Ok(()) => attempt.push_step(step_log(step, index, total, ctx, "ok", "", "")),
                Err(d) => {
                    attempt.push_step(step_log(
                        step,
                        index,
                        total,
                        ctx,
                        "failed",
                        "",
                        &format!("{}: {}\n{}\n", d.code, d.what, d.why),
                    ));
                    return Err(d);
                }
            }
        }
        Ok(report)
    })();
    match result {
        Ok(mut report) => {
            staged.publish(ctx.output_root)?;
            attempt.mark_ok();
            report.plan_fingerprint = plan_fingerprint;
            Ok(report)
        }
        Err(error) => Err(error),
    }
}

fn run_steps(
    recipe: &BuildRecipe,
    order: &[usize],
    ctx: &BuildContext,
    transport: Option<Transport>,
) -> Result<RunReport, Diagnostic> {
    std::fs::create_dir_all(ctx.output_root)
        .map_err(|error| recipe_io_error("could not create staged recipe output", error))?;
    let mut report = run_report();
    for step_index in order {
        let step = recipe
            .steps
            .get(*step_index)
            .ok_or_else(|| e1238_plan("admitted stage refers to a missing recipe step"))?;
        match step {
            BuildStep::Fetch { url, sha256 } => {
                do_fetch(url, sha256, ctx, transport, &mut report)?;
            }
            BuildStep::Exec { tool, args } => {
                do_exec(tool, args, ctx, &mut report)?;
            }
            BuildStep::Install { src, dest } => install_file(src, dest, ctx, &mut report)?,
            BuildStep::InstallTree { src, dest } => install_tree(src, dest, ctx, &mut report)?,
        }
    }
    Ok(report)
}

fn install_file(
    src: &str,
    dest: &str,
    ctx: &BuildContext,
    report: &mut RunReport,
) -> Result<(), Diagnostic> {
    let target = confined_dest(ctx.output_root, dest)?;
    let from = confined_source(ctx.source_dir, src, false)?;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| recipe_io_error("could not create an install directory", error))?;
    }
    std::fs::copy(&from, &target).map_err(|e| {
        Diagnostic::error(
            "E1237",
            format!("build step could not install `{src}`"),
            format!(
                "copying `{}` into the output root failed: {e}",
                from.display()
            ),
            "make sure the source file exists in the staged tree.".to_string(),
            None,
        )
    })?;
    report.add_effect("write");
    Ok(())
}

fn install_tree(
    src: &str,
    dest: &str,
    ctx: &BuildContext,
    report: &mut RunReport,
) -> Result<(), Diagnostic> {
    let target = confined_dest(ctx.output_root, dest)?;
    let from = confined_source(ctx.source_dir, src, true)?;
    copy_tree(&from, &target).map_err(|e| {
        Diagnostic::error(
            "E1237",
            format!("build step could not install `{src}`"),
            format!(
                "copying `{}` into the output root failed: {e}",
                from.display()
            ),
            "make sure the source directory exists in the staged tree.".to_string(),
            None,
        )
    })?;
    report.add_effect("write");
    Ok(())
}

fn staged_output_path(output_root: &Path) -> PathBuf {
    let parent = output_root.parent().unwrap_or_else(|| Path::new("."));
    let name = output_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("output");
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    parent.join(format!(".{name}.jet-stage-{}-{stamp}", std::process::id()))
}

fn commit_staged_output(staged: &Path, output_root: &Path) -> Result<(), Diagnostic> {
    let backup = staged.with_extension("previous");
    let had_previous = output_root.exists();
    if had_previous {
        std::fs::rename(output_root, &backup).map_err(|error| {
            recipe_io_error("could not preserve the previous recipe output", error)
        })?;
    }
    if let Err(error) = std::fs::rename(staged, output_root) {
        if had_previous {
            let _ = std::fs::rename(&backup, output_root);
        }
        return Err(recipe_io_error(
            "could not publish staged recipe output",
            error,
        ));
    }
    if had_previous {
        remove_path(&backup);
    }
    Ok(())
}

fn remove_path(path: &Path) {
    if let Ok(metadata) = std::fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
            let _ = std::fs::remove_file(path);
        } else if metadata.is_dir() {
            let _ = std::fs::remove_dir_all(path);
        } else {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn is_reparse_point(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x0000_0400 != 0
    }
    #[cfg(not(windows))]
    {
        let _ = metadata;
        false
    }
}

fn recipe_io_error(what: &str, error: std::io::Error) -> Diagnostic {
    Diagnostic::error(
        "E1238",
        what.to_string(),
        error.to_string(),
        "fix the build workspace permissions and retry the recipe.".to_string(),
        None,
    )
}

fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    let source_metadata = std::fs::symlink_metadata(src)?;
    if source_metadata.file_type().is_symlink() || is_reparse_point(&source_metadata) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!(
                "recipe tree root is a symlink or reparse point `{}`",
                src.display()
            ),
        ));
    }
    if !source_metadata.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("recipe tree root is not a directory `{}`", src.display()),
        ));
    }
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let metadata = std::fs::symlink_metadata(&from)?;
        if metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!(
                    "recipe source contains a symlink or reparse point `{}`",
                    from.display()
                ),
            ));
        }
        if metadata.is_dir() {
            copy_tree(&from, &to)?;
        } else if metadata.is_file() {
            std::fs::copy(&from, &to)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(&from)?.permissions().mode();
                std::fs::set_permissions(&to, std::fs::Permissions::from_mode(mode))?;
            }
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("recipe tree contains special file `{}`", from.display()),
            ));
        }
    }
    Ok(())
}

fn confined_source(
    source_root: &Path,
    source: &str,
    directory: bool,
) -> Result<PathBuf, Diagnostic> {
    let relative = Path::new(source);
    if source.is_empty()
        || relative.is_absolute()
        || relative
            .components()
            .any(|component| component == std::path::Component::ParentDir)
    {
        return Err(e1237_source(source));
    }
    let root = source_root
        .canonicalize()
        .map_err(|error| recipe_io_error("could not resolve the recipe source root", error))?;
    let path = source_root.join(relative);
    let canonical = path.canonicalize().map_err(|error| {
        recipe_io_error(
            &format!("could not resolve recipe source `{source}`"),
            error,
        )
    })?;
    if !canonical.starts_with(&root) {
        return Err(e1237_source(source));
    }
    if directory && !canonical.is_dir() {
        return Err(e1237_source(source));
    }
    if !directory && !canonical.is_file() {
        return Err(e1237_source(source));
    }
    Ok(canonical)
}

/// Resolve `dest` under `output_root`, rejecting any escape (`..`, absolute
/// path, or a symlink-free normalized path that leaves the root) with `E1237`.
fn confined_dest(output_root: &Path, dest: &str) -> Result<PathBuf, Diagnostic> {
    let relative = Path::new(dest);
    if dest.is_empty()
        || dest.chars().any(char::is_control)
        || relative.is_absolute()
        || relative
            .components()
            .any(|component| component == std::path::Component::ParentDir)
    {
        return Err(e1237(dest));
    }
    let joined = output_root.join(relative);
    let normalized = normalize(&joined);
    if !normalized.starts_with(&normalize(output_root)) {
        return Err(e1237(dest));
    }
    Ok(normalized)
}

/// Lexically normalize a path (no filesystem access): collapse `.` and `..`.
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in p.components() {
        use std::path::Component::*;
        match comp {
            ParentDir => {
                out.pop();
            }
            CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn do_fetch(
    url: &str,
    sha256: &str,
    ctx: &BuildContext,
    transport: Option<Transport>,
    report: &mut RunReport,
) -> Result<(), Diagnostic> {
    if credentialed_fetch_url(url) {
        return Err(e1236_credentialed_url());
    }
    // A locked fetch must carry a path-safe, canonical SHA-256 key (checked in
    // validate too, defensively). Never let an unchecked lock value select a
    // cache path.
    if !valid_sha256(sha256) {
        return Err(e1236_invalid_hash(url, sha256));
    }
    std::fs::create_dir_all(ctx.fetch_cache).ok();
    let cached = ctx.fetch_cache.join(sha256);
    if cached.is_file() {
        // Offline-satisfiable only after re-verifying the immutable key. A
        // corrupt or stale cache entry must never become trusted input.
        let cached_bytes =
            std::fs::read(&cached).map_err(|e| e1236_fetch(url, &format!("reading cache: {e}")))?;
        if SHA256::sha256_hex(&cached_bytes) == sha256 {
            report.fetches.push(FetchRecord {
                url: url.to_string(),
                sha256: sha256.to_string(),
            });
            report.add_effect("net.fetch");
            return Ok(());
        }
        let _ = std::fs::remove_file(&cached);
    }
    // Not cached: acquire the bytes. `file://` is std-only and offline-safe.
    let bytes = if let Some(path) = url.strip_prefix("file://") {
        std::fs::read(path).map_err(|e| e1236_fetch(url, &e.to_string()))?
    } else if ctx.offline {
        // A network fetch under `--offline` with no cache hit is ungranted.
        return Err(e1236_offline(url));
    } else if let Some(t) = transport {
        t(url).map_err(|e| e1236_fetch(url, &e))?
    } else {
        // No transport injected for a remote scheme: the compiler seam holds no
        // network capability (I6). The caller must vendor or mirror the source.
        return Err(e1236_no_transport(url));
    };
    // Verify the locked hash before the bytes are ever used.
    let got = SHA256::sha256_hex(&bytes);
    if got != sha256 {
        return Err(e1236_mismatch(url, sha256, &got));
    }
    let temporary = ctx
        .fetch_cache
        .join(format!(".{sha256}.tmp-{}", std::process::id()));
    std::fs::write(&temporary, &bytes).map_err(|e| e1236_fetch(url, &e.to_string()))?;
    if let Err(error) = std::fs::rename(&temporary, &cached) {
        let _ = std::fs::remove_file(&temporary);
        return Err(e1236_fetch(url, &format!("publishing cache: {error}")));
    }
    report.fetches.push(FetchRecord {
        url: url.to_string(),
        sha256: sha256.to_string(),
    });
    report.add_effect("net.fetch");
    Ok(())
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn digest_matches(declared: &str, actual_hex: &str) -> bool {
    declared == actual_hex
        || declared
            .strip_prefix("sha256-")
            .is_some_and(|digest| digest == actual_hex)
        || declared
            .strip_prefix("sha256:")
            .is_some_and(|digest| digest == actual_hex)
}

/// Build hooks cannot receive a credential provider. Reject URL userinfo at
/// the recipe boundary so the secret cannot reach transport, cache metadata,
/// diagnostics, or step logs.
fn credentialed_fetch_url(url: &str) -> bool {
    if url.chars().any(char::is_control) {
        return true;
    }
    let Some((_, rest)) = url.split_once("://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.contains('@') {
        return true;
    }
    let Some((_, query_and_fragment)) = rest.split_once('?') else {
        return rest
            .split_once('#')
            .is_some_and(|(_, fragment)| credential_fields(fragment));
    };
    let (query, fragment) = query_and_fragment
        .split_once('#')
        .map_or((query_and_fragment, ""), |(query, fragment)| {
            (query, fragment)
        });
    credential_fields(query) || credential_fields(fragment)
}

fn credential_fields(fields: &str) -> bool {
    fields
        .split(['&', ';'])
        .any(|field| credential_field_key(field.split('=').next().unwrap_or_default()))
}

fn credential_field_key(raw: &str) -> bool {
    let key = percent_decode_ascii(raw.trim())
        .to_ascii_lowercase()
        .replace('-', "_");
    matches!(
        key.as_str(),
        "auth"
            | "authorization"
            | "api_key"
            | "apikey"
            | "credential"
            | "credentials"
            | "password"
            | "passwd"
            | "secret"
            | "sig"
            | "signature"
            | "token"
            | "access_token"
            | "refresh_token"
            | "oauth_token"
            | "client_secret"
            | "private_key"
            | "x_amz_credential"
            | "x_amz_signature"
    ) || key.ends_with("_token")
        || key.ends_with("_secret")
        || key.ends_with("_credential")
        || key.ends_with("_signature")
        || key.ends_with("_password")
        || key.ends_with("_api_key")
}

fn percent_decode_ascii(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = String::with_capacity(value.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
            {
                decoded.push((high << 4 | low) as char);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index] as char);
        index += 1;
    }
    decoded
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Construct the only environment a recipe tool may observe. The native
/// backend clears the inherited environment and maps `JET_BUILD_OUTPUT` to the
/// private output mount; this map carries only declared deterministic values.
fn build_env() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("SOURCE_DATE_EPOCH".to_string(), "0".to_string()),
        ("JET_PROFILE".to_string(), "default".to_string()),
    ])
}

/// A realized tool is an exact filesystem artifact. Relative names are
/// rejected so an absent or malformed dependency cannot fall through to the
/// caller's `PATH`.
fn realized_tool<'a>(
    tools: &'a HashMap<String, PathBuf>,
    tool: &str,
) -> Result<&'a Path, Diagnostic> {
    let path = tools.get(tool).ok_or_else(|| e1238(tool))?;
    if !path.is_absolute() {
        return Err(e1238(tool));
    }
    Ok(path.as_path())
}

fn do_exec(
    tool: &str,
    args: &[String],
    ctx: &BuildContext,
    report: &mut RunReport,
) -> Result<(), Diagnostic> {
    let exe = realized_tool(&ctx.tools, tool)?;
    let result = run_recipe_tool(exe, args, ctx)?;
    if !result.output.status.success() {
        return Err(Diagnostic::error(
            "E1238",
            format!("build tool `{tool}` exited with an error"),
            format!("`{tool}` returned a non-zero status; declared arguments are omitted from the diagnostic."),
            "check the build recipe and the tool's arguments.".to_string(),
            None,
        ));
    }
    report.record_sandbox(&result.mechanism, &result.policy);
    report.add_effect(&format!("exec:{tool}"));
    Ok(())
}

fn do_exec_logged(
    tool: &str,
    args: &[String],
    ctx: &BuildContext,
    report: &mut RunReport,
) -> Result<(), Diagnostic> {
    let exe = realized_tool(&ctx.tools, tool)?;
    let result = run_recipe_tool(exe, args, ctx)?;
    if !result.output.status.success() {
        return Err(Diagnostic::error(
            "E1238",
            format!("build tool `{tool}` exited with an error"),
            format!("`{tool}` returned a non-zero status; command arguments and tool output are omitted from the diagnostic."),
            "check the build recipe and the tool's arguments.".to_string(),
            None,
        ));
    }
    report.record_sandbox(&result.mechanism, &result.policy);
    report.add_effect(&format!("exec:{tool}"));
    Ok(())
}

/// Run an untrusted recipe against private copies of source and output. The
/// native backend must not bind the caller's staged output directly: a child
/// can create a symlink there and turn a later output write into a host write.
fn run_recipe_tool(
    exe: &Path,
    args: &[String],
    ctx: &BuildContext,
) -> Result<jet_comptime::Comptime::Build::NativeSandboxOutput, Diagnostic> {
    let parent = ctx.output_root.parent().unwrap_or_else(|| Path::new("."));
    let name = ctx
        .output_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("output");
    let sandbox = parent.join(format!(
        ".{name}.jet-sandbox-{}-{}",
        std::process::id(),
        STAGED_PLAN_ARTIFACT_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    remove_path(&sandbox);
    let source = sandbox.join("source");
    let output = sandbox.join("output");
    std::fs::create_dir_all(&source)
        .map_err(|error| sandbox_copy_error("could not create the private recipe source", error))?;
    std::fs::create_dir_all(&output)
        .map_err(|error| sandbox_copy_error("could not create the private recipe output", error))?;
    if let Err(error) = copy_tree(ctx.source_dir, &source) {
        remove_path(&sandbox);
        return Err(sandbox_copy_error(
            "could not snapshot the recipe source for the sandbox",
            error,
        ));
    }
    if let Err(error) = copy_tree(ctx.output_root, &output) {
        remove_path(&sandbox);
        return Err(sandbox_copy_error(
            "could not snapshot the recipe output for the sandbox",
            error,
        ));
    }

    let result = jet_comptime::Comptime::Build::run_native_sandboxed(
        exe,
        args,
        &source,
        Some(&output),
        &build_env(),
        false,
    )
    .map_err(native_sandbox_diagnostic);
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            remove_path(&sandbox);
            return Err(error);
        }
    };
    if result.output.status.success() {
        if let Err(error) = replace_recipe_output(&output, ctx.output_root) {
            remove_path(&sandbox);
            return Err(sandbox_copy_error(
                "the sandbox produced an unsafe recipe output",
                error,
            ));
        }
    }
    remove_path(&sandbox);
    Ok(result)
}

fn replace_recipe_output(from: &Path, to: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(to)? {
        remove_path(&entry?.path());
    }
    copy_tree(from, to)
}

fn sandbox_copy_error(what: &str, error: std::io::Error) -> Diagnostic {
    Diagnostic::error(
        "E1237",
        what.to_string(),
        format!("the sandbox boundary rejected a source or output tree: {error}"),
        "remove symlinks and special files from the recipe source and output, then retry."
            .to_string(),
        None,
    )
}

fn native_sandbox_diagnostic(
    error: jet_comptime::Comptime::Build::NativeSandboxError,
) -> Diagnostic {
    let detail = match error {
        jet_comptime::Comptime::Build::NativeSandboxError::Unsupported(detail)
        | jet_comptime::Comptime::Build::NativeSandboxError::Io(detail) => detail,
    };
    Diagnostic::error(
        "E1275",
        "build sandboxing is required but unavailable".to_string(),
        detail,
        "provide a trusted substitute or approved remote builder, or enable the native sandbox, then retry.".to_string(),
        None,
    )
}

fn step_log(
    step: &BuildStep,
    index: usize,
    total: usize,
    ctx: &BuildContext,
    status: &str,
    stdout: &str,
    stderr: &str,
) -> super::BuildDebug::StepLog {
    super::BuildDebug::StepLog {
        index,
        total,
        name: step_name(step).to_string(),
        command: step_command(step),
        cwd: ctx.source_dir.to_string_lossy().into_owned(),
        status: status.to_string(),
        stdout: stdout.to_string(),
        stderr: stderr.to_string(),
    }
}

fn step_name(step: &BuildStep) -> &'static str {
    match step {
        BuildStep::Fetch { .. } => "fetch",
        BuildStep::Exec { .. } => "exec",
        BuildStep::Install { .. } => "install",
        BuildStep::InstallTree { .. } => "install-tree",
    }
}

fn step_command(step: &BuildStep) -> String {
    match step {
        BuildStep::Fetch { url, sha256 } => format!("fetch {url} sha256:{sha256}"),
        BuildStep::Exec { tool, args } => {
            format!("{tool} [{} declared args]", args.len())
        }
        BuildStep::Install { src, dest } => format!("install {src} {dest}"),
        BuildStep::InstallTree { src, dest } => format!("install-tree {src} {dest}"),
    }
}

// ── U19 trust gate (internal substrate) ──────────────────────────────────────
// The interactive first-build approval UX is card #176 (U19); here we keep the
// durable marker so a recipe's first build is distinguishable from a re-build.

/// Record trust for a recipe hash in the canonical trust store. Returns `true`
/// when the hash is new and `false` when the store already contains it.
pub fn trust_first_build(recipe_hash: &str, trust_store: &Path) -> bool {
    crate::Trust::grant_hash(trust_store, recipe_hash)
}

// ── Diagnostics ───────────────────────────────────────────────────────────────

/// E1236 — a build fetch is unlocked or carries credentials in its URL.
pub fn e1236(url: &str) -> Diagnostic {
    if credentialed_fetch_url(url) {
        return e1236_credentialed_url();
    }
    Diagnostic::error(
        "E1236",
        "a build fetch is not an allowed locked, credential-free input".to_string(),
        format!(
            "build hooks admit only content-hash-pinned fetches without URL userinfo or credential \
             query fields; `{url}` is not an allowed build input."
        ),
        "add the source hash, remove URL credentials, or vendor the source with `jet registry vendor`."
            .to_string(),
        None,
    )
}

fn e1236_offline(url: &str) -> Diagnostic {
    Diagnostic::error(
        "E1236",
        "a build step needs the network but the build is offline".to_string(),
        format!("`--offline` forbids any network fetch; `{url}` is not in the local fetch cache."),
        "run once online to populate the cache, or `jet registry vendor` the source and rebuild."
            .to_string(),
        None,
    )
}

fn e1236_no_transport(url: &str) -> Diagnostic {
    Diagnostic::error(
        "E1236",
        "a build step needs a remote source with no transport available".to_string(),
        format!(
            "`{url}` is a remote URL; the build seam holds no network ability by itself \
             (zero-external-crate compiler)."
        ),
        "provide a `file://` mirror, or `jet registry vendor` the source so the build is offline."
            .to_string(),
        None,
    )
}

fn e1236_credentialed_url() -> Diagnostic {
    Diagnostic::error(
        "E1236",
        "a build fetch contains embedded credentials".to_string(),
        "build-hook fetch URLs are action inputs and may be retained in cache metadata and logs; URL userinfo and credential query or fragment fields are therefore not allowed.".to_string(),
        "remove the credentials, use a public or vendored source, and keep authentication outside the build hook.".to_string(),
        None,
    )
}

fn e1236_fetch(url: &str, reason: &str) -> Diagnostic {
    Diagnostic::error(
        "E1236",
        "a locked build fetch failed".to_string(),
        format!("fetching `{url}` failed: {reason}"),
        "check the URL and the locked hash, or vendor the source.".to_string(),
        None,
    )
}

fn e1236_mismatch(url: &str, want: &str, got: &str) -> Diagnostic {
    Diagnostic::error(
        "E1236",
        "a locked build fetch did not match its hash".to_string(),
        format!("`{url}` was fetched but its sha256 was `{got}`, not the locked `{want}`."),
        "the source changed upstream or was tampered with; update the locked hash deliberately."
            .to_string(),
        None,
    )
}

fn e1236_invalid_hash(url: &str, hash: &str) -> Diagnostic {
    Diagnostic::error(
        "E1236",
        "a locked build fetch has an invalid sha256".to_string(),
        format!(
            "the fetch of `{url}` uses `{hash}`; a locked fetch needs exactly 64 hexadecimal SHA-256 characters."
        ),
        "replace it with the source's canonical SHA-256 hash, or vendor the source.".to_string(),
        None,
    )
}

/// E1237 — a build step wrote outside the package output root.
pub fn e1237(dest: &str) -> Diagnostic {
    Diagnostic::error(
        "E1237",
        format!("a build step tried to write outside the output root: `{dest}`"),
        "a build may only install files under its own package output root. Writing elsewhere \
         would let a build mutate the machine or other packages."
            .to_string(),
        "install into a path under the output root (no `..`, no absolute paths).".to_string(),
        None,
    )
}

fn e1237_source(source: &str) -> Diagnostic {
    Diagnostic::error(
        "E1237",
        format!("a build step tried to read an unsafe recipe source: `{source}`"),
        "recipe inputs must be existing files or directories below the staged source root; absolute, parent, and escaping symlink paths are rejected".to_string(),
        "use a project-relative source path inside the staged tree".to_string(),
        None,
    )
}

/// E1238 — a recipe or staged plan violated its declared build graph contract.
pub fn e1238(tool: &str) -> Diagnostic {
    Diagnostic::error(
        "E1238",
        format!("build tool `{tool}` is not a realized dependency"),
        "build tools must be realized `Pkg` dependencies of the package, so the build is \
         reproducible. A build never falls through to host `/usr/bin`."
            .to_string(),
        format!(
            "add `{tool}` to the adapter's `deps: […]` list so Jetpack realizes it into the hangar."
        ),
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "recipe-{tag}-{}-{:?}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn ctx_at<'a>(
        base: &'a Path,
        src: &'a Path,
        out: &'a Path,
        cache: &'a Path,
    ) -> BuildContext<'a> {
        let _ = base;
        BuildContext {
            source_dir: src,
            output_root: out,
            tools: HashMap::new(),
            fetch_cache: cache,
            offline: false,
        }
    }

    #[test]
    fn build_denies_ambient_network() {
        // A fetch with no locked sha256 is ungranted ambient network → E1236.
        let base = scratch("net");
        let src = base.join("src");
        let out = base.join("out");
        let cache = base.join("cache");
        std::fs::create_dir_all(&src).unwrap();
        let ctx = ctx_at(&base, &src, &out, &cache);
        let recipe = BuildRecipe {
            steps: vec![BuildStep::Fetch {
                url: "https://example.invalid/src.tar".to_string(),
                sha256: String::new(),
            }],
        };
        let err = run(&recipe, &ctx, None).unwrap_err();
        assert_eq!(err.code, "E1236");
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn build_rejects_fetch_url_credentials_without_echoing_them() {
        let base = scratch("fetch-credentials");
        let src = base.join("src");
        let out = base.join("out");
        let cache = base.join("cache");
        std::fs::create_dir_all(&src).unwrap();
        let secret = "do-not-log-this-token";
        let recipe = BuildRecipe {
            steps: vec![BuildStep::Fetch {
                url: format!("https://builder:{secret}@example.invalid/source.tar"),
                sha256: "0".repeat(64),
            }],
        };
        let error = run(
            &recipe,
            &BuildContext {
                source_dir: &src,
                output_root: &out,
                tools: HashMap::new(),
                fetch_cache: &cache,
                offline: false,
            },
            None,
        )
        .expect_err("credentialed fetch must fail before transport");
        assert_eq!(error.code, "E1236");
        assert!(!format!("{error:?}").contains(secret));
        assert!(!out.exists(), "rejected hooks must not publish an output");
        assert!(credentialed_fetch_url(
            "https://example.invalid/source.tar?token=hidden"
        ));
        assert!(credentialed_fetch_url(
            "https://example.invalid/source.tar?download=1#token=hidden"
        ));
        assert!(credentialed_fetch_url(
            "https://example.invalid/source.tar?%74oken=hidden"
        ));
        assert!(credentialed_fetch_url(
            "https://example.invalid/source.tar?X-Amz-Credential=hidden"
        ));
        assert!(!credentialed_fetch_url(
            "https://example.invalid/source.tar?download=1"
        ));
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn build_output_confined() {
        // An install targeting a path outside the output root → E1237.
        let base = scratch("confine");
        let src = base.join("src");
        let out = base.join("out");
        let cache = base.join("cache");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("f"), "hi").unwrap();
        let ctx = ctx_at(&base, &src, &out, &cache);
        let recipe = BuildRecipe {
            steps: vec![BuildStep::Install {
                src: "f".to_string(),
                dest: "../escape".to_string(),
            }],
        };
        let err = run(&recipe, &ctx, None).unwrap_err();
        assert_eq!(err.code, "E1237");
        // A confined install succeeds and lands under the output root.
        let ok = BuildRecipe {
            steps: vec![BuildStep::Install {
                src: "f".to_string(),
                dest: "bin/f".to_string(),
            }],
        };
        run(&ok, &ctx, None).unwrap();
        assert!(out.join("bin/f").is_file());
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn build_tool_not_a_dep() {
        // An exec naming a tool that is not a realized dep → E1238.
        let base = scratch("tool");
        let src = base.join("src");
        let out = base.join("out");
        let cache = base.join("cache");
        std::fs::create_dir_all(&src).unwrap();
        let ctx = ctx_at(&base, &src, &out, &cache);
        let recipe = BuildRecipe {
            steps: vec![BuildStep::Exec {
                tool: "gcc".to_string(),
                args: vec![],
            }],
        };
        let err = run(&recipe, &ctx, None).unwrap_err();
        assert_eq!(err.code, "E1238");
        std::fs::remove_dir_all(&base).ok();
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn native_recipe_path_succeeds_or_refuses_before_launch() {
        let base = scratch("native-sandbox");
        let src = base.join("src");
        let out = base.join("out");
        let cache = base.join("cache");
        let marker = base.join("host-marker");
        std::fs::create_dir_all(&src).unwrap();
        let shell = std::env::split_paths(
            &std::env::var_os("PATH").expect("PATH should be available to sandbox tests"),
        )
        .map(|directory| directory.join("sh"))
        .find(|candidate| candidate.is_file())
        .expect("a shell is required for the native recipe path");
        let marker = marker.to_string_lossy().replace('\'', "'\\''");
        let mut tools = HashMap::new();
        tools.insert("sh".to_string(), shell);
        let ctx = BuildContext {
            source_dir: &src,
            output_root: &out,
            tools,
            fetch_cache: &cache,
            offline: false,
        };
        let recipe = BuildRecipe {
            steps: vec![BuildStep::Exec {
                tool: "sh".to_string(),
                args: vec![
                    "-c".to_string(),
                    format!(
                        "printf ok > \"$JET_BUILD_OUTPUT/ok\"; printf escaped > '{marker}' || :"
                    ),
                ],
            }],
        };
        let result = run(&recipe, &ctx, None);
        let status = jet_comptime::Comptime::Build::native_sandbox_status();
        if status.available {
            let report = result.expect("native recipe sandbox should run");
            assert_eq!(
                report.sandbox_class,
                if cfg!(target_os = "linux") {
                    "linux-bwrap"
                } else {
                    "macos-seatbelt"
                }
            );
            assert_eq!(std::fs::read_to_string(out.join("ok")).unwrap(), "ok");
        } else {
            assert_eq!(result.unwrap_err().code, "E1275");
        }
        assert!(!base.join("host-marker").exists());
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn locked_fetch_roundtrips() {
        // A locked file:// fetch caches by hash, records provenance, and is
        // offline-satisfiable on a second run even after the source vanishes.
        let base = scratch("fetch");
        let src = base.join("src");
        let out = base.join("out");
        let cache = base.join("cache");
        std::fs::create_dir_all(&src).unwrap();
        let source_file = base.join("upstream.tar");
        let payload = b"the source bytes";
        std::fs::write(&source_file, payload).unwrap();
        let sha = SHA256::sha256_hex(payload);
        let url = format!("file://{}", source_file.to_string_lossy());

        let recipe = BuildRecipe {
            steps: vec![BuildStep::Fetch {
                url: url.clone(),
                sha256: sha.clone(),
            }],
        };

        // First build: online, populates the cache and records the fetch.
        let ctx = ctx_at(&base, &src, &out, &cache);
        let report = run(&recipe, &ctx, None).unwrap();
        assert_eq!(report.fetches.len(), 1);
        assert_eq!(report.fetches[0].sha256, sha);
        assert!(report.effects.iter().any(|e| e == "net.fetch"));
        assert!(
            cache.join(&sha).is_file(),
            "locked source must be cached by hash"
        );

        // Now the upstream source disappears and we go offline — the re-build is
        // still satisfiable from the cache, no network.
        std::fs::remove_file(&source_file).unwrap();
        let offline_ctx = BuildContext {
            source_dir: &src,
            output_root: &out,
            tools: HashMap::new(),
            fetch_cache: &cache,
            offline: true,
        };
        let report2 = run(&recipe, &offline_ctx, None).unwrap();
        assert_eq!(report2.fetches[0].sha256, sha);
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn offline_uncached_fetch_is_ungranted() {
        // Offline with no cache hit and a remote scheme → E1236.
        let base = scratch("offline");
        let src = base.join("src");
        let out = base.join("out");
        let cache = base.join("cache");
        std::fs::create_dir_all(&src).unwrap();
        let ctx = BuildContext {
            source_dir: &src,
            output_root: &out,
            tools: HashMap::new(),
            fetch_cache: &cache,
            offline: true,
        };
        let recipe = BuildRecipe {
            steps: vec![BuildStep::Fetch {
                url: "https://example.invalid/x.tar".to_string(),
                sha256: "abc123".to_string(),
            }],
        };
        assert_eq!(run(&recipe, &ctx, None).unwrap_err().code, "E1236");
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn recipe_and_trust_consumers_share_one_hash_grant() {
        let base = scratch("trust");
        let trust = base.join("trust");
        let recipe = BuildRecipe {
            steps: vec![BuildStep::Install {
                src: "a".to_string(),
                dest: "a".to_string(),
            }],
        };
        let h = recipe.recipe_hash();
        assert!(
            trust_first_build(&h, &trust),
            "first build is newly trusted"
        );
        assert!(
            crate::Trust::is_trusted(&trust, &base, &h),
            "the trust consumer sees the recipe grant"
        );
        assert_eq!(
            crate::Trust::list_entries(&trust),
            vec![format!("hash:{h}")],
            "one canonical trust record is written"
        );
        assert!(
            !trust_first_build(&h, &trust),
            "second build is already trusted"
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn validate_is_pure_read_no_exec() {
        // `jet inspect audit` uses validate(): it flags violations without running any
        // step. A recipe with an exec of a missing tool validates to E1238 and
        // never spawns a process.
        let base = scratch("audit");
        let src = base.join("src");
        let out = base.join("out");
        let cache = base.join("cache");
        std::fs::create_dir_all(&src).unwrap();
        let ctx = ctx_at(&base, &src, &out, &cache);
        let recipe = BuildRecipe {
            steps: vec![BuildStep::Exec {
                tool: "definitely-not-a-real-tool".to_string(),
                args: vec!["--boom".to_string()],
            }],
        };
        assert_eq!(validate(&recipe, &ctx).unwrap_err().code, "E1238");
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn recipe_lowers_to_one_build_plan_with_complete_fingerprint() {
        let mut tools = HashMap::new();
        tools.insert("cc".to_string(), PathBuf::from("/hangar/bin/cc"));
        let recipe = BuildRecipe {
            steps: vec![
                BuildStep::Fetch {
                    url: "file:///src.tgz".to_string(),
                    sha256: "abc123".to_string(),
                },
                BuildStep::Exec {
                    tool: "cc".to_string(),
                    args: vec!["-c".to_string(), "main.c".to_string()],
                },
                BuildStep::Install {
                    src: "main.o".to_string(),
                    dest: "lib/main.o".to_string(),
                },
            ],
        };
        let plan = lower_to_plan(&recipe, "demo", &tools).expect("lower");
        assert_eq!(plan.actions().len(), 3);
        assert_eq!(plan.targets().len(), 1);
        assert_eq!(
            plan.targets()[0].kind,
            crate::Comptime::Build::TargetKind::Package
        );
        let kinds: Vec<_> = plan.actions().iter().map(|a| a.kind).collect();
        assert_eq!(
            kinds,
            vec![
                crate::Comptime::Build::ActionKind::Generic,
                crate::Comptime::Build::ActionKind::Compile,
                crate::Comptime::Build::ActionKind::SourceArchive,
            ]
        );
        let fp = plan_recipe_fingerprint(&plan).expect("fingerprint");
        assert!(fp.starts_with("plan-sha256:"));
        assert_eq!(fp, plan.complete_recipe_fingerprint().unwrap());
        // Distinct from a different package name / step set.
        let other = lower_to_plan(&recipe, "other", &tools).unwrap();
        assert_ne!(
            plan_recipe_fingerprint(&plan).unwrap(),
            plan_recipe_fingerprint(&other).unwrap()
        );
    }

    #[test]
    fn recipe_plan_binds_outputs_authority_and_finite_stage_facts() {
        let recipe = BuildRecipe {
            steps: vec![BuildStep::Install {
                src: "bin/tool".to_string(),
                dest: "bin/tool".to_string(),
            }],
        };
        let plan = lower_to_plan(&recipe, "demo", &HashMap::new()).expect("lower");
        let action = &plan.actions()[0];
        let outputs = action
            .outputs
            .iter()
            .map(|path| path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            outputs,
            vec![
                ".jet/recipe/demo/step-0.stamp",
                ".jet/recipe/demo/outputs/bin/tool"
            ]
        );
        assert!(action
            .caps
            .contains(&crate::Comptime::Build::BuildCapability::FS));
        assert_eq!(
            action.labels.get("authority.effect"),
            Some(&"fs.write".to_string())
        );
        assert_eq!(action.labels.get("stage.index"), Some(&"0".to_string()));
        assert_eq!(action.labels.get("stage.bound"), Some(&"1".to_string()));
        assert!(action.labels.contains_key("platform.os"));
        assert!(action.labels.contains_key("platform.arch"));
    }

    #[test]
    fn staged_plan_authority_order_does_not_change_plan_fingerprint() {
        let platform = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
        let action = StagedPlanAction::new(
            "discover",
            0,
            1,
            vec![PlanInput::new("manifest", "sha256-input")],
            PlanAuthority {
                tools: vec!["planner".to_string(), "planner-alt".to_string()],
                effects: vec![
                    jet_foundation::BuildEffect::Exec,
                    jet_foundation::BuildEffect::FS,
                ],
                platform: platform.clone(),
            },
        );
        let mut emitted = PlanFragmentAction::new("compile", "planner");
        emitted.inputs = vec!["manifest".to_string()];
        emitted.outputs = vec!["result.bin".to_string()];
        emitted.effects = vec![
            jet_foundation::BuildEffect::Exec,
            jet_foundation::BuildEffect::FS,
        ];
        emitted.platform = platform;
        let fragment = BuildPlanFragment {
            actions: vec![emitted],
        };
        let tools = HashMap::from([
            ("planner".to_string(), PathBuf::from("/hangar/bin/planner")),
            (
                "planner-alt".to_string(),
                PathBuf::from("/hangar/bin/planner-alt"),
            ),
        ]);
        let first = lower_staged_plan_action(&action, &fragment, &tools).unwrap();
        let mut reordered = action;
        reordered.authority.tools.reverse();
        reordered.authority.effects.reverse();
        let second = lower_staged_plan_action(&reordered, &fragment, &tools).unwrap();
        assert_eq!(
            plan_recipe_fingerprint(&first).unwrap(),
            plan_recipe_fingerprint(&second).unwrap()
        );
    }

    #[test]
    fn finite_plan_rejects_empty_and_overlapping_outputs() {
        let empty = lower_to_plan(&BuildRecipe::default(), "demo", &HashMap::new())
            .expect_err("an empty staged recipe has no finite action graph");
        assert_eq!(empty.code, "E1238");

        let overlapping = BuildRecipe {
            steps: vec![
                BuildStep::InstallTree {
                    src: "share".to_string(),
                    dest: "share".to_string(),
                },
                BuildStep::Install {
                    src: "tool".to_string(),
                    dest: "share/tool".to_string(),
                },
            ],
        };
        let error = lower_to_plan(&overlapping, "demo", &HashMap::new())
            .expect_err("two stages cannot own overlapping output paths");
        assert_eq!(error.code, "E1238");
        assert!(error.why.contains("overlaps output"));
    }

    #[test]
    fn failed_stage_removes_partial_output_and_preserves_previous_output() {
        let base = scratch("partial-stage");
        let src = base.join("src");
        let out = base.join("out");
        let cache = base.join("cache");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("tool"), "new").unwrap();
        std::fs::create_dir_all(out.join("bin")).unwrap();
        std::fs::write(out.join("bin/tool"), "old").unwrap();
        let recipe = BuildRecipe {
            steps: vec![
                BuildStep::Install {
                    src: "tool".to_string(),
                    dest: "bin/tool".to_string(),
                },
                BuildStep::Fetch {
                    url: "https://example.invalid/source.tar".to_string(),
                    sha256: "0".repeat(64),
                },
            ],
        };
        let error = run(&recipe, &ctx_at(&base, &src, &out, &cache), None)
            .expect_err("remote fetch without a transport must fail");
        assert_eq!(error.code, "E1236");
        assert_eq!(
            std::fs::read_to_string(out.join("bin/tool")).unwrap(),
            "old"
        );
        assert!(!base
            .read_dir()
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with(".out.jet-stage-")));
        std::fs::remove_dir_all(&base).ok();
    }
}
