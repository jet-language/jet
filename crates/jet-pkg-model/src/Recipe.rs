//! `BuildRecipe` data model (D-JPK-ADAPTER1=A safety contract).
//!
//! A `BuildRecipe` turns a staged source tree into a realized package under a
//! confined, auditable build. This is the pure **data** half of the build
//! substrate — the struct/enum the `Build(BuildRecipe)` plan variant carries.
//! The engine (`validate`/`run`/`run_logged`, sandboxing, fetch/exec/install,
//! trust gate) stays in `jetpack`'s `Recipe.rs`, which imports `BuildRecipe`
//! from here (card #367 slice 4, data-down / engine-up).
//!
//! std-only (I6).

use crate::SHA256;
use jet_foundation::BuildEffect;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Component, Path};

/// One step of a build recipe. Names are internal; the user-facing spellings are
/// the finite `Recipe.build(steps: […])` forms from D-JPK-BUILDRECIPE1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildStep {
    /// A locked network fetch. `sha256` must be present; an empty hash is
    /// ungranted ambient network (`E1236`).
    Fetch { url: String, sha256: String },
    /// Run a build tool. `tool` must be the name of a realized `Pkg` dep in the
    /// `BuildContext.tools` map — never resolved from host PATH (`E1238`).
    Exec { tool: String, args: Vec<String> },
    /// Copy `src` (relative to the source dir) to `dest` under the output root.
    /// `dest` must resolve inside the output root (`E1237`).
    Install { src: String, dest: String },
    /// Copy a whole directory tree relative to the source dir into `dest`
    /// under the output root. Used by `Recipe.copy()`.
    InstallTree { src: String, dest: String },
}

/// A build recipe over a staged source tree.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BuildRecipe {
    pub steps: Vec<BuildStep>,
}

/// One exact input made visible to a finite staged plan action.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PlanInput {
    pub path: String,
    pub digest: String,
}

impl PlanInput {
    pub fn new(path: impl Into<String>, digest: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            digest: digest.into(),
        }
    }
}

/// The only authority a staged plan action may observe.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlanAuthority {
    pub tools: Vec<String>,
    pub effects: Vec<BuildEffect>,
    pub platform: String,
}

/// A typed finite action emitted by a staged plan action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanFragmentAction {
    pub name: String,
    pub tool: String,
    pub args: Vec<String>,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub effects: Vec<BuildEffect>,
    pub dependencies: Vec<String>,
    pub platform: String,
}

impl PlanFragmentAction {
    pub fn new(name: impl Into<String>, tool: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            tool: tool.into(),
            args: Vec::new(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            env: BTreeMap::new(),
            effects: Vec::new(),
            dependencies: Vec::new(),
            platform: String::new(),
        }
    }
}

/// Typed output of a finite staged plan action. It is lowered into the same
/// `BuildPlan` action graph as ordinary recipes; it is not an alternate graph.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BuildPlanFragment {
    pub actions: Vec<PlanFragmentAction>,
}

/// Metadata for one sandboxed plan action. The fragment is supplied only after
/// the action runs, then checked against this declaration before publication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedPlanAction {
    pub name: String,
    pub stage: usize,
    pub stage_bound: usize,
    pub inputs: Vec<PlanInput>,
    pub authority: PlanAuthority,
}

impl StagedPlanAction {
    pub fn new(
        name: impl Into<String>,
        stage: usize,
        stage_bound: usize,
        inputs: Vec<PlanInput>,
        authority: PlanAuthority,
    ) -> Self {
        Self {
            name: name.into(),
            stage,
            stage_bound,
            inputs,
            authority,
        }
    }

    /// Validate the declaration before the sandbox is entered.
    pub fn validate_declaration(&self) -> Result<(), StagedPlanError> {
        if self.name.trim().is_empty() || has_control(&self.name) {
            return Err(StagedPlanError::EmptyActionName);
        }
        if self.stage_bound == 0 {
            return Err(StagedPlanError::InvalidStageBound);
        }
        if self.stage >= self.stage_bound {
            return Err(StagedPlanError::StageOutOfBounds {
                stage: self.stage,
                bound: self.stage_bound,
            });
        }
        if self.authority.platform.trim().is_empty() || has_control(&self.authority.platform) {
            return Err(StagedPlanError::EmptyPlatform);
        }

        let mut declared_inputs = BTreeMap::new();
        for input in &self.inputs {
            validate_relative_path(&input.path)
                .map_err(|reason| StagedPlanError::InvalidInput(input.path.clone(), reason))?;
            if input.digest.trim().is_empty() || has_control(&input.digest) {
                return Err(StagedPlanError::MissingInputDigest(input.path.clone()));
            }
            if declared_inputs
                .insert(input.path.clone(), input.digest.clone())
                .is_some()
            {
                return Err(StagedPlanError::DuplicateInput(input.path.clone()));
            }
        }

        unique_strings(&self.authority.tools, StagedPlanError::DuplicateTool)?;
        unique_effects(&self.authority.effects)?;
        Ok(())
    }

    /// Validate the fragment before it can enter the executable graph.
    pub fn validate(&self, fragment: &BuildPlanFragment) -> Result<(), StagedPlanError> {
        self.validate_declaration()?;
        let declared_inputs = self
            .inputs
            .iter()
            .map(|input| input.path.clone())
            .collect::<BTreeSet<_>>();
        let authority_tools =
            unique_strings(&self.authority.tools, StagedPlanError::DuplicateTool)?;
        let authority_effects = unique_effects(&self.authority.effects)?;
        let mut names = BTreeSet::new();
        let mut output_owners = Vec::<(String, String)>::new();
        let mut dependencies = BTreeMap::<String, Vec<String>>::new();
        if fragment.actions.is_empty() {
            return Err(StagedPlanError::EmptyFragment);
        }

        for action in &fragment.actions {
            if action.name.trim().is_empty() || has_control(&action.name) {
                return Err(StagedPlanError::EmptyFragmentActionName);
            }
            if !names.insert(action.name.clone()) {
                return Err(StagedPlanError::DuplicateFragmentActionName(
                    action.name.clone(),
                ));
            }
            if action.tool.trim().is_empty() || has_control(&action.tool) {
                return Err(StagedPlanError::EmptyFragmentTool(action.name.clone()));
            }
            if !authority_tools.contains(&action.tool) {
                return Err(StagedPlanError::UnauthorizedTool {
                    action: action.name.clone(),
                    tool: action.tool.clone(),
                });
            }
            if action.platform != self.authority.platform {
                return Err(StagedPlanError::PlatformMismatch {
                    action: action.name.clone(),
                    expected: self.authority.platform.clone(),
                    actual: action.platform.clone(),
                });
            }
            let effects = unique_effects(&action.effects)?;
            for effect in effects {
                if !authority_effects.contains(&effect) {
                    return Err(StagedPlanError::UnauthorizedEffect {
                        action: action.name.clone(),
                        effect,
                    });
                }
            }
            for input in &action.inputs {
                validate_relative_path(input)
                    .map_err(|reason| StagedPlanError::InvalidInput(input.clone(), reason))?;
                if !declared_inputs.contains(input) {
                    return Err(StagedPlanError::UndeclaredInput {
                        action: action.name.clone(),
                        path: input.clone(),
                    });
                }
            }
            if action.outputs.is_empty() {
                return Err(StagedPlanError::MissingOutput(action.name.clone()));
            }
            for output in &action.outputs {
                validate_relative_path(output)
                    .map_err(|reason| StagedPlanError::InvalidOutput(output.clone(), reason))?;
                if let Some((first, first_owner)) = output_owners
                    .iter()
                    .find(|(old, _)| paths_overlap(old, output))
                {
                    return Err(StagedPlanError::OutputConflict {
                        first: first.clone(),
                        first_owner: first_owner.clone(),
                        second: output.clone(),
                        second_owner: action.name.clone(),
                    });
                }
                output_owners.push((output.clone(), action.name.clone()));
            }
            for dependency in &action.dependencies {
                if dependency == &action.name || dependency.trim().is_empty() {
                    return Err(StagedPlanError::InvalidDependency {
                        action: action.name.clone(),
                        dependency: dependency.clone(),
                    });
                }
            }
            dependencies.insert(action.name.clone(), action.dependencies.clone());
            for (key, value) in &action.env {
                if key.trim().is_empty() || has_control(key) || has_control(value) {
                    return Err(StagedPlanError::InvalidEnvironment(action.name.clone()));
                }
            }
            if action.args.iter().any(|arg| has_control(arg)) {
                return Err(StagedPlanError::InvalidArguments(action.name.clone()));
            }
        }

        for (action, action_dependencies) in &dependencies {
            for dependency in action_dependencies {
                if !names.contains(dependency) {
                    return Err(StagedPlanError::UnknownDependency {
                        action: action.clone(),
                        dependency: dependency.clone(),
                    });
                }
            }
        }
        detect_dependency_cycle(&dependencies)?;
        Ok(())
    }

    /// Stable identity over the declaration and emitted fragment. Exact input
    /// digests and every authority fact are part of the identity.
    pub fn identity(&self, fragment: &BuildPlanFragment) -> Result<String, StagedPlanError> {
        self.validate(fragment)?;
        let mut writer = CanonicalWriter::new(STAGED_PLAN_FORMAT);
        self.write_canonical(&mut writer);
        fragment.write_canonical(&mut writer);
        Ok(format!(
            "staged-plan-sha256:{}",
            SHA256::sha256_hex(&writer.bytes)
        ))
    }

    pub fn fragment_digest(&self, fragment: &BuildPlanFragment) -> Result<String, StagedPlanError> {
        self.validate(fragment)?;
        Ok(format!(
            "sha256-{}",
            SHA256::sha256_hex(&fragment.canonical_bytes())
        ))
    }

    pub fn lock(&self, fragment: &BuildPlanFragment) -> Result<StagedPlanLock, StagedPlanError> {
        let identity = self.identity(fragment)?;
        let fragment_digest = self.fragment_digest(fragment)?;
        Ok(StagedPlanLock {
            action_identity: identity,
            fragment_digest,
            stage: self.stage,
            stage_bound: self.stage_bound,
            inputs: self.inputs.clone(),
            authority: self.authority.clone(),
        })
    }

    fn write_canonical(&self, writer: &mut CanonicalWriter) {
        writer.str(&self.name);
        writer.usize(self.stage);
        writer.usize(self.stage_bound);
        let mut inputs = self.inputs.iter().collect::<Vec<_>>();
        inputs.sort();
        writer.usize(inputs.len());
        for input in inputs {
            writer.str(&input.path);
            writer.str(&input.digest);
        }
        let mut tools = self.authority.tools.clone();
        tools.sort();
        writer.strs(tools.iter().map(String::as_str));
        let mut effects = self.authority.effects.clone();
        effects.sort();
        writer.usize(effects.len());
        for effect in effects {
            writer.str(effect.flag());
        }
        writer.str(&self.authority.platform);
    }
}

/// Exact facts written beside a staged fragment for offline replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedPlanLock {
    pub action_identity: String,
    pub fragment_digest: String,
    pub stage: usize,
    pub stage_bound: usize,
    pub inputs: Vec<PlanInput>,
    pub authority: PlanAuthority,
}

impl StagedPlanLock {
    pub fn encode(&self) -> String {
        let mut out = String::from("jet-staged-plan-lock-v1\n");
        out.push_str(&format!("action_identity={}\n", self.action_identity));
        out.push_str(&format!("fragment_digest={}\n", self.fragment_digest));
        out.push_str(&format!("stage={}\n", self.stage));
        out.push_str(&format!("stage_bound={}\n", self.stage_bound));
        out.push_str(&format!("platform={}\n", self.authority.platform));
        let mut inputs = self.inputs.clone();
        inputs.sort();
        for input in inputs {
            out.push_str(&format!("input={}\t{}\n", input.path, input.digest));
        }
        let mut tools = self.authority.tools.clone();
        tools.sort();
        for tool in tools {
            out.push_str(&format!("tool={tool}\n"));
        }
        let mut effects = self.authority.effects.clone();
        effects.sort();
        for effect in effects {
            out.push_str(&format!("effect={}\n", effect.flag()));
        }
        out
    }
}

/// Validation failures are kept in the model so evaluator and engine cannot
/// disagree about what a finite staged fragment means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StagedPlanError {
    EmptyActionName,
    InvalidStageBound,
    StageOutOfBounds {
        stage: usize,
        bound: usize,
    },
    EmptyPlatform,
    InvalidInput(String, String),
    MissingInputDigest(String),
    DuplicateInput(String),
    DuplicateTool(String),
    DuplicateEffect(String),
    EmptyFragment,
    EmptyFragmentActionName,
    DuplicateFragmentActionName(String),
    EmptyFragmentTool(String),
    UnauthorizedTool {
        action: String,
        tool: String,
    },
    UnauthorizedEffect {
        action: String,
        effect: BuildEffect,
    },
    PlatformMismatch {
        action: String,
        expected: String,
        actual: String,
    },
    UndeclaredInput {
        action: String,
        path: String,
    },
    MissingOutput(String),
    InvalidOutput(String, String),
    OutputConflict {
        first: String,
        first_owner: String,
        second: String,
        second_owner: String,
    },
    InvalidDependency {
        action: String,
        dependency: String,
    },
    UnknownDependency {
        action: String,
        dependency: String,
    },
    DependencyCycle(Vec<String>),
    InvalidEnvironment(String),
    InvalidArguments(String),
}

impl fmt::Display for StagedPlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyActionName => f.write_str("staged plan action name is empty"),
            Self::InvalidStageBound => f.write_str("staged plan stage bound must be positive"),
            Self::StageOutOfBounds { stage, bound } => {
                write!(
                    f,
                    "staged plan stage {stage} is outside finite bound {bound}"
                )
            }
            Self::EmptyPlatform => f.write_str("staged plan platform is empty"),
            Self::InvalidInput(path, reason) => write!(f, "input `{path}` is invalid: {reason}"),
            Self::MissingInputDigest(path) => write!(f, "input `{path}` has no digest"),
            Self::DuplicateInput(path) => write!(f, "input `{path}` is declared more than once"),
            Self::DuplicateTool(tool) => write!(f, "tool `{tool}` is declared more than once"),
            Self::DuplicateEffect(effect) => {
                write!(f, "effect `{effect}` is declared more than once")
            }
            Self::EmptyFragment => f.write_str("staged plan emitted no actions"),
            Self::EmptyFragmentActionName => {
                f.write_str("staged plan emitted an action with no name")
            }
            Self::DuplicateFragmentActionName(name) => {
                write!(f, "staged plan emitted duplicate action `{name}`")
            }
            Self::EmptyFragmentTool(name) => write!(f, "staged action `{name}` has no tool"),
            Self::UnauthorizedTool { action, tool } => {
                write!(f, "staged action `{action}` uses undeclared tool `{tool}`")
            }
            Self::UnauthorizedEffect { action, effect } => {
                write!(
                    f,
                    "staged action `{action}` uses undeclared effect `{}`",
                    effect.flag()
                )
            }
            Self::PlatformMismatch {
                action,
                expected,
                actual,
            } => write!(
                f,
                "staged action `{action}` targets `{actual}`, not declared platform `{expected}`"
            ),
            Self::UndeclaredInput { action, path } => {
                write!(
                    f,
                    "staged action `{action}` reads undeclared input `{path}`"
                )
            }
            Self::MissingOutput(name) => write!(f, "staged action `{name}` has no output"),
            Self::InvalidOutput(path, reason) => write!(f, "output `{path}` is invalid: {reason}"),
            Self::OutputConflict {
                first,
                first_owner,
                second,
                second_owner,
            } => write!(
                f,
                "staged outputs `{first}` ({first_owner}) and `{second}` ({second_owner}) overlap"
            ),
            Self::InvalidDependency { action, dependency } => {
                write!(
                    f,
                    "staged action `{action}` has invalid dependency `{dependency}`"
                )
            }
            Self::UnknownDependency { action, dependency } => {
                write!(
                    f,
                    "staged action `{action}` depends on unknown action `{dependency}`"
                )
            }
            Self::DependencyCycle(chain) => {
                write!(
                    f,
                    "staged action graph contains a cycle: {}",
                    chain.join(" -> ")
                )
            }
            Self::InvalidEnvironment(name) => {
                write!(f, "staged action `{name}` has invalid environment facts")
            }
            Self::InvalidArguments(name) => {
                write!(f, "staged action `{name}` has invalid argument facts")
            }
        }
    }
}

impl std::error::Error for StagedPlanError {}

pub const STAGED_PLAN_FORMAT: &str = "jet.staged-plan.v1";

fn unique_strings(
    values: &[String],
    duplicate: impl Fn(String) -> StagedPlanError,
) -> Result<BTreeSet<String>, StagedPlanError> {
    let mut out = BTreeSet::new();
    for value in values {
        if value.trim().is_empty() || has_control(value) {
            return Err(StagedPlanError::InvalidInput(
                value.clone(),
                "authority name is empty or contains control characters".to_string(),
            ));
        }
        if !out.insert(value.clone()) {
            return Err(duplicate(value.clone()));
        }
    }
    Ok(out)
}

fn unique_effects(values: &[BuildEffect]) -> Result<BTreeSet<BuildEffect>, StagedPlanError> {
    let mut out = BTreeSet::new();
    for effect in values {
        if !out.insert(*effect) {
            return Err(StagedPlanError::DuplicateEffect(effect.flag().to_string()));
        }
    }
    Ok(out)
}

fn validate_relative_path(path: &str) -> Result<(), String> {
    if path.trim().is_empty() || has_control(path) {
        return Err("path is empty or contains control characters".to_string());
    }
    if Path::new(path).is_absolute()
        || Path::new(path).components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err("path must stay below the declared sandbox root".to_string());
    }
    Ok(())
}

fn has_control(value: &str) -> bool {
    value.chars().any(char::is_control)
}

fn paths_overlap(left: &str, right: &str) -> bool {
    left == right
        || left
            .strip_prefix(right)
            .is_some_and(|suffix| suffix.starts_with('/') || suffix.starts_with('\\'))
        || right
            .strip_prefix(left)
            .is_some_and(|suffix| suffix.starts_with('/') || suffix.starts_with('\\'))
}

fn detect_dependency_cycle(
    dependencies: &BTreeMap<String, Vec<String>>,
) -> Result<(), StagedPlanError> {
    fn visit(
        node: &str,
        dependencies: &BTreeMap<String, Vec<String>>,
        states: &mut BTreeMap<String, u8>,
        stack: &mut Vec<String>,
    ) -> Result<(), StagedPlanError> {
        match states.get(node).copied().unwrap_or(0) {
            2 => return Ok(()),
            1 => {
                let start = stack.iter().position(|item| item == node).unwrap_or(0);
                let mut cycle = stack[start..].to_vec();
                cycle.push(node.to_string());
                return Err(StagedPlanError::DependencyCycle(cycle));
            }
            _ => {}
        }
        states.insert(node.to_string(), 1);
        stack.push(node.to_string());
        for dependency in dependencies.get(node).into_iter().flatten() {
            visit(dependency, dependencies, states, stack)?;
        }
        stack.pop();
        states.insert(node.to_string(), 2);
        Ok(())
    }

    let mut states = BTreeMap::new();
    for node in dependencies.keys() {
        visit(node, dependencies, &mut states, &mut Vec::new())?;
    }
    Ok(())
}

impl BuildPlanFragment {
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut writer = CanonicalWriter::new("jet.build-plan-fragment.v1");
        self.write_canonical(&mut writer);
        writer.bytes
    }

    fn write_canonical(&self, writer: &mut CanonicalWriter) {
        let mut actions = self.actions.iter().collect::<Vec<_>>();
        actions.sort_by(|left, right| left.name.cmp(&right.name));
        writer.usize(actions.len());
        for action in actions {
            writer.str(&action.name);
            writer.str(&action.tool);
            writer.strs(action.args.iter().map(String::as_str));
            let mut inputs = action.inputs.clone();
            inputs.sort();
            writer.strs(inputs.iter().map(String::as_str));
            let mut outputs = action.outputs.clone();
            outputs.sort();
            writer.strs(outputs.iter().map(String::as_str));
            writer.usize(action.env.len());
            for (key, value) in &action.env {
                writer.str(key);
                writer.str(value);
            }
            let mut effects = action.effects.clone();
            effects.sort();
            writer.usize(effects.len());
            for effect in effects {
                writer.str(effect.flag());
            }
            let mut dependencies = action.dependencies.clone();
            dependencies.sort();
            writer.strs(dependencies.iter().map(String::as_str));
            writer.str(&action.platform);
        }
    }
}

impl StagedPlanLock {
    // Kept beside the model so the engine writes the exact same facts it keys.
    pub fn canonical_identity(&self) -> &str {
        &self.action_identity
    }
}

impl BuildRecipe {
    /// A stable content hash of the recipe, used by the trust gate.
    pub fn recipe_hash(&self) -> String {
        let mut writer = CanonicalWriter::new("jet.build-recipe.v2");
        writer.usize(self.steps.len());
        for (index, step) in self.steps.iter().enumerate() {
            writer.usize(index);
            match step {
                BuildStep::Fetch { url, sha256 } => {
                    writer.str("fetch");
                    writer.str(url);
                    writer.str(sha256);
                }
                BuildStep::Exec { tool, args } => {
                    writer.str("exec");
                    writer.str(tool);
                    writer.strs(args.iter().map(String::as_str));
                }
                BuildStep::Install { src, dest } => {
                    writer.str("install");
                    writer.str(src);
                    writer.str(dest);
                }
                BuildStep::InstallTree { src, dest } => {
                    writer.str("install-tree");
                    writer.str(src);
                    writer.str(dest);
                }
            }
        }
        format!("sha256-{}", SHA256::sha256_hex(&writer.bytes))
    }

    /// The exact authority requested by this recipe. This is deliberately
    /// derived from the finite step graph, not from a host environment or a
    /// caller-supplied label.
    pub fn declared_capabilities(&self) -> Vec<String> {
        let mut capabilities = Vec::new();
        for step in &self.steps {
            let capability = match step {
                BuildStep::Fetch { .. } => "net.fetch".to_string(),
                BuildStep::Exec { tool, .. } => format!("exec:{tool}"),
                BuildStep::Install { .. } | BuildStep::InstallTree { .. } => "fs.write".to_string(),
            };
            if !capabilities.iter().any(|existing| existing == &capability) {
                capabilities.push(capability);
            }
        }
        capabilities.sort();
        capabilities
    }

    /// Bind a build hook to every fact that can change its authority or
    /// result. The returned identity is suitable for both the action-cache key
    /// and a reviewed trust grant: package, provider/source, staged source,
    /// platform, recipe, and the complete declared capability set all
    /// participate.
    pub fn build_identity(&self, package: &str, source_digest: &str, platform: &str) -> String {
        self.build_identity_for_source(package, "", source_digest, platform)
    }

    /// Build the canonical identity used by a provider-backed hook. The
    /// compatibility-shaped [`Self::build_identity`] entry point remains for
    /// callers that do not have a provider/source label; production providers
    /// must use this source-bound form.
    pub fn build_identity_for_source(
        &self,
        package: &str,
        provider_source: &str,
        source_digest: &str,
        platform: &str,
    ) -> String {
        self.build_identity_for_source_with_dependencies(
            package,
            provider_source,
            source_digest,
            platform,
            &[],
        )
    }

    /// Build the approval subject for a hook whose tool dependencies are
    /// declared by the surrounding adapter plan. Dependency refs are part of
    /// authority: changing a tool's provider/source must invalidate both the
    /// cache identity and the exact trust grant, even when the recipe bytes
    /// stay unchanged.
    pub fn build_identity_for_source_with_dependencies(
        &self,
        package: &str,
        provider_source: &str,
        source_digest: &str,
        platform: &str,
        dependencies: &[String],
    ) -> String {
        let mut dependencies = dependencies.to_vec();
        dependencies.sort();
        let mut writer = CanonicalWriter::new("jet-build-hook.v3");
        writer.str(package);
        writer.str(provider_source);
        writer.str(source_digest);
        writer.str(platform);
        writer.str(&self.recipe_hash());
        writer.strs(self.declared_capabilities().iter().map(String::as_str));
        writer.strs(dependencies.iter().map(String::as_str));
        format!("build-sha256:{}", SHA256::sha256_hex(&writer.bytes))
    }
}

/// Length-frame every field so delimiters, newlines, and empty values cannot
/// make two different declared plans share one identity.
struct CanonicalWriter {
    bytes: Vec<u8>,
}

impl CanonicalWriter {
    fn new(domain: &str) -> Self {
        let mut writer = Self { bytes: Vec::new() };
        writer.str(domain);
        writer
    }

    fn usize(&mut self, value: usize) {
        self.bytes.extend_from_slice(&(value as u64).to_be_bytes());
    }

    fn str(&mut self, value: &str) {
        self.bytes
            .extend_from_slice(&(value.len() as u64).to_be_bytes());
        self.bytes.extend_from_slice(value.as_bytes());
    }

    fn strs<'a>(&mut self, values: impl IntoIterator<Item = &'a str>) {
        let values = values.into_iter().collect::<Vec<_>>();
        self.usize(values.len());
        for value in values {
            self.str(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BuildEffect, BuildPlanFragment, BuildRecipe, BuildStep, PlanAuthority, PlanFragmentAction,
        PlanInput, StagedPlanAction,
    };

    fn staged_fragment(platform: &str) -> BuildPlanFragment {
        let mut action = PlanFragmentAction::new("compile", "planner");
        action.inputs = vec!["manifest".to_string()];
        action.outputs = vec!["result.bin".to_string()];
        action.effects = vec![BuildEffect::Exec];
        action.platform = platform.to_string();
        BuildPlanFragment {
            actions: vec![action],
        }
    }

    fn staged_action(platform: &str) -> StagedPlanAction {
        StagedPlanAction::new(
            "discover",
            0,
            1,
            vec![PlanInput::new("manifest", "sha256-input")],
            PlanAuthority {
                tools: vec!["planner".to_string()],
                effects: vec![BuildEffect::Exec],
                platform: platform.to_string(),
            },
        )
    }

    #[test]
    fn staged_identity_binds_finite_inputs_authority_and_fragment() {
        let action = staged_action("linux-x86_64");
        let fragment = staged_fragment("linux-x86_64");
        let identity = action.identity(&fragment).unwrap();
        assert_eq!(identity, action.identity(&fragment).unwrap());
        assert_eq!(
            action.lock(&fragment).unwrap().canonical_identity(),
            identity
        );

        let mut changed_stage = action.clone();
        changed_stage.stage = 1;
        changed_stage.stage_bound = 2;
        assert_ne!(identity, changed_stage.identity(&fragment).unwrap());

        let mut changed_input = action.clone();
        changed_input.inputs[0].digest = "sha256-other".to_string();
        assert_ne!(identity, changed_input.identity(&fragment).unwrap());

        let mut changed_authority = action.clone();
        changed_authority.authority.effects.push(BuildEffect::FS);
        assert_ne!(identity, changed_authority.identity(&fragment).unwrap());

        let mut changed_fragment = fragment.clone();
        changed_fragment.actions[0].outputs = vec!["other.bin".to_string()];
        assert_ne!(identity, action.identity(&changed_fragment).unwrap());
    }

    #[test]
    fn staged_validation_rejects_cycles_and_overlapping_outputs() {
        let action = staged_action("linux-x86_64");
        let mut first = staged_fragment("linux-x86_64").actions.remove(0);
        first.dependencies = vec!["second".to_string()];
        let mut second = PlanFragmentAction::new("second", "planner");
        second.inputs = vec!["manifest".to_string()];
        second.outputs = vec!["other.bin".to_string()];
        second.dependencies = vec!["compile".to_string()];
        second.effects = vec![BuildEffect::Exec];
        second.platform = "linux-x86_64".to_string();
        let error = action
            .validate(&BuildPlanFragment {
                actions: vec![first, second],
            })
            .unwrap_err();
        assert!(error.to_string().contains("cycle"));

        let mut overlapping = staged_fragment("linux-x86_64");
        let mut nested = PlanFragmentAction::new("nested", "planner");
        nested.inputs = vec!["manifest".to_string()];
        nested.outputs = vec!["result.bin/child".to_string()];
        nested.effects = vec![BuildEffect::Exec];
        nested.platform = "linux-x86_64".to_string();
        overlapping.actions.push(nested);
        let error = action.validate(&overlapping).unwrap_err();
        assert!(error.to_string().contains("overlap"));
    }

    #[test]
    fn hook_identity_binds_provider_source() {
        let recipe = BuildRecipe {
            steps: vec![BuildStep::InstallTree {
                src: ".".to_string(),
                dest: ".".to_string(),
            }],
        };
        let local = recipe.build_identity_for_source(
            "tool",
            "./vendor/tool",
            "sha256-source",
            "linux-x86_64",
        );
        let remote = recipe.build_identity_for_source(
            "tool",
            "github:owner/tool",
            "sha256-source",
            "linux-x86_64",
        );
        assert_ne!(local, remote);
    }

    #[test]
    fn hook_identity_binds_capability_set() {
        let copy = BuildRecipe {
            steps: vec![BuildStep::InstallTree {
                src: ".".to_string(),
                dest: ".".to_string(),
            }],
        };
        let exec = BuildRecipe {
            steps: vec![BuildStep::Exec {
                tool: "cc".to_string(),
                args: vec!["-c".to_string(), "main.c".to_string()],
            }],
        };
        let copy_id = copy.build_identity_for_source("tool", "./tool", "source", "linux-x86_64");
        let exec_id = exec.build_identity_for_source("tool", "./tool", "source", "linux-x86_64");
        assert_ne!(copy_id, exec_id);
        assert_eq!(copy.declared_capabilities(), vec!["fs.write"]);
        assert_eq!(exec.declared_capabilities(), vec!["exec:cc"]);
    }

    #[test]
    fn hook_identity_binds_all_authority_facts() {
        let recipe = BuildRecipe {
            steps: vec![BuildStep::Install {
                src: "a".to_string(),
                dest: "bin/a".to_string(),
            }],
        };
        let identity = |package, source, digest, platform| {
            recipe.build_identity_for_source(package, source, digest, platform)
        };
        let base = identity("tool", "registry:stable", "source-a", "linux-x86_64");

        assert_ne!(
            base,
            identity("other", "registry:stable", "source-a", "linux-x86_64")
        );
        assert_ne!(
            base,
            identity("tool", "registry:next", "source-a", "linux-x86_64")
        );
        assert_ne!(
            base,
            identity("tool", "registry:stable", "source-b", "linux-x86_64")
        );
        assert_ne!(
            base,
            identity("tool", "registry:stable", "source-a", "darwin-arm64")
        );
        assert_ne!(
            base,
            BuildRecipe {
                steps: vec![BuildStep::InstallTree {
                    src: "a".to_string(),
                    dest: "bin/a".to_string(),
                }],
            }
            .build_identity_for_source(
                "tool",
                "registry:stable",
                "source-a",
                "linux-x86_64"
            )
        );
    }

    #[test]
    fn hook_identity_binds_declared_tool_dependencies() {
        let recipe = BuildRecipe {
            steps: vec![BuildStep::Exec {
                tool: "cc".to_string(),
                args: vec!["main.c".to_string()],
            }],
        };
        let identity = |dependencies: &[String]| {
            recipe.build_identity_for_source_with_dependencies(
                "tool",
                "./vendor/tool",
                "source",
                "linux-x86_64",
                dependencies,
            )
        };
        let stable = identity(&["cc@default".to_string()]);
        assert_ne!(stable, identity(&["cc@trusted".to_string()]));
        assert_eq!(
            stable,
            identity(&["cc@default".to_string()]),
            "repeated builds must derive one identity from the same declared facts"
        );
        assert_eq!(
            identity(&["cc@default".to_string(), "ar@default".to_string()]),
            identity(&["ar@default".to_string(), "cc@default".to_string()]),
            "dependency ordering must not make repeated builds diverge"
        );
    }

    #[test]
    fn canonical_hash_frames_steps_and_fields() {
        let first = BuildRecipe {
            steps: vec![BuildStep::Exec {
                tool: "cc\0ar".to_string(),
                args: vec!["main".to_string()],
            }],
        };
        let second = BuildRecipe {
            steps: vec![BuildStep::Exec {
                tool: "cc".to_string(),
                args: vec!["ar\0main".to_string()],
            }],
        };
        assert_ne!(
            first.recipe_hash(),
            second.recipe_hash(),
            "field boundaries must be part of recipe identity"
        );
        assert_eq!(first.recipe_hash(), first.recipe_hash());
    }

    #[test]
    fn canonical_hook_identity_frames_authority_fields() {
        let recipe = BuildRecipe::default();
        let first = recipe.build_identity_for_source_with_dependencies(
            "pkg\nsource",
            "provider",
            "digest",
            "linux",
            &["tool\nref".to_string()],
        );
        let second = recipe.build_identity_for_source_with_dependencies(
            "pkg",
            "source\nprovider",
            "digest",
            "linux",
            &["tool\nref".to_string()],
        );
        assert_ne!(first, second);
    }
}
