use super::actions_policy::{ActionCache, BuildAction, BuildCapability, BuildResourcePoolSpec};
use super::cache_cas::{ActionCacheStatus, ActionInputSnapshot, ActionKey, ActionOutcome, ContentDigest};
use super::errors_keys::{BuildError, canonical_action_key, canonical_effective_action_key};
use super::execution_helpers::{
    action_pools, cache_status_reason, collect_target_actions, default_resource_pools,
    execution_metrics, execution_stages,
};
use super::execution_runtime::{BuildProbeFact, read_last_rebuild_record};
use super::handles::{
    ActionHandle, ActionId, ProbeHandle, ProbeId, SigningIdentityHandle, TargetId, TargetRef,
    ToolchainHandle, ToolchainId,
};
use super::plan_graph::{
    BuildExecutionEvent, BuildExecutionModel, BuildExecutionNode, BuildExecutionReport,
    BuildExplanation, BuildGraph, BuildGraphAction, BuildGraphFile, BuildGraphSubject,
    BuildGraphTarget, BuildPlan, CompilerPackageSpec, FileOwnership, RebuildExplanation,
};
use super::plugins_modules::{BuildGeneratedModule, BuildPlugin};
use super::provenance_toolchains::{BuildProbe, BuildSigningIdentity, BuildToolchain};
use super::targets::{BuildPath, BuildTarget, TargetKind};
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

impl BuildPlan {
    pub fn default_target(&self) -> Option<TargetRef> {
        self.default
    }

    pub fn targets(&self) -> &[BuildTarget] {
        &self.targets
    }

    pub fn actions(&self) -> &[BuildAction] {
        &self.actions
    }

    pub fn toolchains(&self) -> &[BuildToolchain] {
        &self.toolchains
    }

    pub fn signing_identities(&self) -> &[BuildSigningIdentity] {
        &self.signing_identities
    }

    pub fn probes(&self) -> &[BuildProbe] {
        &self.probes
    }

    pub fn plugins(&self) -> &[BuildPlugin] {
        &self.plugins
    }

    pub fn generated_modules(&self) -> &[BuildGeneratedModule] {
        &self.generated_modules
    }

    /// Generated modules reachable from the selected target's explicit source
    /// list or selected action outputs. Merely registering `b.generate` does
    /// not make an unselected module part of the runtime program.
    pub fn selected_generated_modules(&self) -> Result<Vec<&BuildGeneratedModule>, BuildError> {
        let mut paths = self.selected_sources()?.into_iter()
            .map(|path| path.as_str().to_string())
            .collect::<BTreeSet<_>>();
        let actions = self.selected_action_ids()?;
        for action in self.actions.iter().filter(|action| actions.contains(&action.id)) {
            paths.extend(action.outputs.iter().map(|path| path.as_str().to_string()));
        }
        let mut selected = self.generated_modules.iter()
            .filter(|module| paths.contains(module.path.as_str()))
            .collect::<Vec<_>>();
        selected.sort_by(|left, right| {
            left.path
                .as_str()
                .cmp(right.path.as_str())
                .then_with(|| left.name.cmp(&right.name))
        });
        Ok(selected)
    }

    /// Actions reachable from the selected default target. A plan without a
    /// default intentionally selects every registered target.
    pub fn selected_action_ids(&self) -> Result<BTreeSet<ActionId>, BuildError> {
        let mut selected = BTreeSet::new();
        if let Some(default) = self.default {
            collect_target_actions(self, default.id, &mut BTreeSet::new(), &mut selected)?;
        } else {
            for target in &self.targets {
                collect_target_actions(self, target.id, &mut BTreeSet::new(), &mut selected)?;
            }
        }
        let prereqs = self.action_prereqs()?;
        let mut pending = selected.iter().copied().collect::<Vec<_>>();
        while let Some(action) = pending.pop() {
            for dependency in prereqs.get(&action).into_iter().flatten() {
                if selected.insert(*dependency) {
                    pending.push(*dependency);
                }
            }
        }
        Ok(selected)
    }

    /// Source closure compiled for the selected target, including target deps.
    pub fn selected_sources(&self) -> Result<Vec<BuildPath>, BuildError> {
        fn collect(
            plan: &BuildPlan,
            id: TargetId,
            visiting: &mut BTreeSet<TargetId>,
            seen: &mut BTreeSet<String>,
            out: &mut Vec<BuildPath>,
        ) -> Result<(), BuildError> {
            if !visiting.insert(id) {
                return Err(BuildError::TargetDependencyCycle);
            }
            let target = plan.targets.get(id.0).ok_or(BuildError::UnknownTarget(id))?;
            for source in &target.sources {
                if seen.insert(source.as_str().to_string()) {
                    out.push(source.clone());
                }
            }
            for dep in &target.deps {
                collect(plan, dep.id, visiting, seen, out)?;
            }
            visiting.remove(&id);
            Ok(())
        }
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        if let Some(default) = self.default {
            collect(self, default.id, &mut BTreeSet::new(), &mut seen, &mut out)?;
        } else {
            for target in &self.targets {
                collect(self, target.id, &mut BTreeSet::new(), &mut seen, &mut out)?;
            }
        }
        Ok(out)
    }

    pub fn selected_probe_ids(&self) -> Result<BTreeSet<ProbeId>, BuildError> {
        let actions = self.selected_action_ids()?;
        let mut probes = self
            .actions
            .iter()
            .filter(|action| actions.contains(&action.id))
            .flat_map(|action| action.probes.iter().map(|probe| probe.id))
            .collect::<BTreeSet<_>>();
        let selected_targets = if let Some(default) = self.default {
            let mut targets = BTreeSet::new();
            fn visit(plan: &BuildPlan, id: TargetId, out: &mut BTreeSet<TargetId>) {
                if out.insert(id) {
                    if let Some(target) = plan.targets.get(id.0) {
                        for dep in &target.deps { visit(plan, dep.id, out); }
                    }
                }
            }
            visit(self, default.id, &mut targets);
            targets
        } else {
            self.targets.iter().map(|target| target.id).collect()
        };
        for target in self.targets.iter().filter(|target| selected_targets.contains(&target.id)) {
            probes.extend(target.probes.iter().map(|probe| probe.id));
        }
        Ok(probes)
    }

    pub fn default_host_toolchain(&self) -> &BuildToolchain {
        &self.toolchains[0]
    }

    pub fn target(&self, target: impl Into<TargetRef>) -> Option<&BuildTarget> {
        let target = target.into();
        if target.context != self.context {
            return None;
        }
        self.targets.get(target.id.0)
    }

    pub fn action(&self, action: ActionHandle) -> Option<&BuildAction> {
        if action.context != self.context {
            return None;
        }
        self.actions.get(action.id.0)
    }

    pub fn action_handle(&self, action: ActionId) -> Option<ActionHandle> {
        self.actions.get(action.0).map(|_| ActionHandle {
            id: action,
            context: self.context,
        })
    }

    /// Add the compiler-owned sealed package layer to an already evaluated
    /// build plan. The generated actions use the same graph, key, and CAS
    /// machinery as user actions; only their cache-miss execution is supplied
    /// by the driver.
    pub fn add_compiler_package_actions(
        &mut self,
        packages: &[CompilerPackageSpec],
        compiler_identity: impl AsRef<str>,
        target: impl AsRef<str>,
        profile: impl AsRef<str>,
    ) -> Result<Vec<ActionHandle>, BuildError> {
        if packages.is_empty() {
            return Ok(Vec::new());
        }

        let target_id = self
            .default
            .map(|target| target.id)
            .or_else(|| self.targets.first().map(|target| target.id))
            .ok_or(BuildError::UnknownTarget(TargetId(0)))?;
        if self.targets.get(target_id.0).is_none() {
            return Err(BuildError::UnknownTarget(target_id));
        }
        let toolchain_id = self
            .toolchains
            .first()
            .map(|toolchain| toolchain.id)
            .ok_or(BuildError::UnknownToolchain(ToolchainId(0)))?;

        let mut output_by_package = BTreeMap::new();
        let mut package_by_output = BTreeMap::new();
        let mut dependencies_by_package = BTreeMap::new();
        for package in packages {
            if package.name.trim().is_empty() {
                return Err(BuildError::EmptyActionName);
            }
            if output_by_package.contains_key(&package.name) {
                return Err(BuildError::DuplicateActionName(format!(
                    "compile-package:{}",
                    package.name
                )));
            }
            let output = BuildPath::new(format!(
                ".jet/build-cache/package-artifacts/{}.sealed",
                compiler_package_path_name(&package.name)
            ))?;
            if let Some(previous) = package_by_output.insert(output.clone(), package.name.clone()) {
                return Err(BuildError::DuplicateActionName(format!(
                    "compile-package:{} (output also belongs to {})",
                    package.name, previous
                )));
            }
            output_by_package.insert(package.name.clone(), output);
            dependencies_by_package.insert(
                package.name.clone(),
                package.dependencies.iter().cloned().collect::<BTreeSet<_>>(),
            );
        }

        for package in packages {
            let action_name = format!("compile-package:{}", package.name);
            if self.actions.iter().any(|action| action.name == action_name) {
                return Err(BuildError::DuplicateActionName(action_name));
            }

            let output = output_by_package
                .get(&package.name)
                .cloned()
                .ok_or_else(|| BuildError::CompilerPackageDependencyMissing {
                    package: package.name.clone(),
                    dependency: package.name.clone(),
                })?;
            if let Some(existing) = self.actions.iter().find(|action| {
                action.outputs.iter().any(|existing_output| existing_output == &output)
            }) {
                return Err(BuildError::DuplicateBuildOutput {
                    output: output.as_str().to_string(),
                    first_action: existing.name.clone(),
                    second_action: action_name,
                });
            }
            if let Some(module) = self.generated_modules.iter().find(|module| module.path == output) {
                return Err(BuildError::GeneratedModuleCycle {
                    module: module.name.clone(),
                    path: output.as_str().to_string(),
                });
            }
        }

        for (package, dependencies) in &dependencies_by_package {
            for dependency in dependencies {
                if !output_by_package.contains_key(dependency) {
                    return Err(BuildError::CompilerPackageDependencyMissing {
                        package: package.clone(),
                        dependency: dependency.clone(),
                    });
                }
            }
        }

        let mut remaining = dependencies_by_package.keys().cloned().collect::<BTreeSet<_>>();
        while !remaining.is_empty() {
            let ready = remaining
                .iter()
                .filter(|package| {
                    dependencies_by_package
                        .get(*package)
                        .expect("compiler package dependency preflight")
                        .iter()
                        .all(|dependency| !remaining.contains(dependency))
                })
                .cloned()
                .collect::<Vec<_>>();
            if ready.is_empty() {
                return Err(BuildError::ActionDependencyCycle);
            }
            for package in ready {
                remaining.remove(&package);
            }
        }

        let compiler_identity = compiler_identity.as_ref().to_string();
        let target = target.as_ref().to_string();
        let profile = profile.as_ref().to_string();
        let mut handles = Vec::with_capacity(packages.len());
        for package in packages {
            let action_name = format!("compile-package:{}", package.name);
            let inputs = dependencies_by_package[&package.name]
                .iter()
                .map(|dependency| {
                    output_by_package
                        .get(dependency)
                        .cloned()
                        .expect("compiler package dependency output preflight")
                })
                .collect::<Vec<_>>();
            let output = output_by_package
                .get(&package.name)
                .cloned()
                .expect("compiler package output preflight");
            let id = ActionId(self.actions.len());
            let mut labels = BTreeMap::new();
            labels.insert("compiler.owner".to_string(), "jet".to_string());
            labels.insert("compiler.package".to_string(), package.name.clone());
            labels.insert(
                "compiler.source-digest".to_string(),
                package.source_digest.as_str().to_string(),
            );
            labels.insert(
                "compiler.identity".to_string(),
                compiler_identity.clone(),
            );
            labels.insert("compiler.target".to_string(), target.clone());
            labels.insert("compiler.profile".to_string(), profile.clone());
            self.actions.push(BuildAction {
                id,
                name: action_name,
                inputs,
                outputs: vec![output],
                argv: vec!["jet-compiler".to_string(), package.name.clone()],
                env: BTreeMap::new(),
                env_allowlist: BTreeSet::new(),
                caps: BTreeSet::new(),
                cache: ActionCache::Cached,
                kind: super::actions_policy::ActionKind::Compile,
                toolchain: ToolchainHandle {
                    id: toolchain_id,
                    context: self.context,
                },
                probes: Vec::new(),
                signing_identity: None,
                labels,
                helper_versions: BTreeMap::new(),
                resource_pools: BTreeSet::new(),
                legacy_wrapper: None,
                plugin: None,
                variant_identity: None,
                compiler_owned: true,
            });
            let handle = ActionHandle {
                id,
                context: self.context,
            };
            self.targets[target_id.0].actions.push(handle);
            handles.push(handle);
        }
        Ok(handles)
    }

    pub fn action_key(&self, action: ActionHandle) -> Result<ActionKey, BuildError> {
        self.action_key_with_inputs(action, &[])
    }

    /// Stable fingerprint over every action key in this plan — the complete
    /// recipe identity for Hangar `CacheIdentity.recipe_fingerprint` (E4-JP2).
    pub fn complete_recipe_fingerprint(&self) -> Result<String, BuildError> {
        let mut keys = Vec::new();
        for action in &self.actions {
            let key = self.action_key(ActionHandle {
                id: action.id,
                context: self.context,
            })?;
            keys.push(key.as_str().to_string());
        }
        keys.sort();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"jet.plan-fingerprint.v1\0");
        for key in keys {
            bytes.extend_from_slice(key.as_bytes());
            bytes.push(0);
        }
        Ok(format!(
            "plan-sha256:{}",
            crate::SHA256::sha256_hex(&bytes)
        ))
    }

    pub fn action_key_with_inputs(
        &self,
        action: ActionHandle,
        inputs: &[ActionInputSnapshot],
    ) -> Result<ActionKey, BuildError> {
        if action.context != self.context {
            return Err(BuildError::UnknownAction(action.id));
        }
        let action = self
            .actions
            .get(action.id.0)
            .ok_or(BuildError::UnknownAction(action.id))?;
        Ok(canonical_action_key(self, action, inputs))
    }

    pub fn effective_action_key(
        &self,
        action: ActionHandle,
        inputs: &[ActionInputSnapshot],
        grants: &BTreeSet<BuildCapability>,
        executable: &Path,
        executable_digest: &ContentDigest,
        probe_facts: &[BuildProbeFact],
    ) -> Result<ActionKey, BuildError> {
        let action_ref = self.action(action).ok_or(BuildError::UnknownAction(action.id))?;
        Ok(canonical_effective_action_key(
            self,
            action_ref,
            inputs,
            grants,
            executable,
            executable_digest,
            probe_facts,
        ))
    }

    pub fn toolchain(&self, toolchain: ToolchainHandle) -> Option<&BuildToolchain> {
        if toolchain.context != self.context {
            return None;
        }
        self.toolchains.get(toolchain.id.0)
    }

    pub fn signing_identity(
        &self,
        identity: SigningIdentityHandle,
    ) -> Option<&BuildSigningIdentity> {
        if identity.context != self.context {
            return None;
        }
        self.signing_identities.get(identity.id.0)
    }

    pub fn probe(&self, probe: ProbeHandle) -> Option<&BuildProbe> {
        if probe.context != self.context {
            return None;
        }
        self.probes.get(probe.id.0)
    }

    pub fn targets_by_kind(&self, kind: TargetKind) -> Vec<&BuildTarget> {
        self.targets.iter().filter(|t| t.kind == kind).collect()
    }

    pub fn phony_actions(&self) -> Vec<&BuildAction> {
        self.actions
            .iter()
            .filter(|a| a.cache == ActionCache::UncachedPhony)
            .collect()
    }

    pub fn resource_pools(&self) -> Vec<BuildResourcePoolSpec> {
        let mut pools = default_resource_pools();
        let mut custom_pools = Vec::new();
        for action in &self.actions {
            for pool in action_pools(action) {
                if !pools.iter().any(|spec| spec.pool.as_str() == pool.as_str()) {
                    // Named pools are deliberately conservative: absent an
                    // explicit capacity declaration, one action owns the
                    // pool. This preserves deterministic serialization and
                    // prevents a custom pool from becoming an unbounded
                    // host escape.
                    custom_pools.push(BuildResourcePoolSpec::new(pool, 1));
                }
            }
        }
        custom_pools.sort_by(|left, right| left.pool.as_str().cmp(right.pool.as_str()));
        pools.extend(custom_pools);
        pools
    }

    pub fn graph(&self) -> BuildGraph {
        let action_targets = self.action_targets();
        let file_index = self.file_index(&action_targets);
        BuildGraph {
            targets: self
                .targets
                .iter()
                .map(|target| BuildGraphTarget {
                    id: target.id,
                    name: target.name.clone(),
                    kind: target.kind,
                    deps: target.deps.iter().map(|dep| dep.id).collect(),
                    actions: target.actions.iter().map(|action| action.id).collect(),
                    files: target
                        .sources
                        .iter()
                        .chain(target.inputs.iter())
                        .chain(target.outputs.iter())
                        .map(|path| path.as_str().to_string())
                        .collect(),
                    plugin: target.plugin.map(|plugin| plugin.id),
                })
                .collect(),
            actions: self
                .actions
                .iter()
                .map(|action| BuildGraphAction {
                    id: action.id,
                    name: action.name.clone(),
                    kind: action.kind,
                    inputs: action
                        .inputs
                        .iter()
                        .map(|path| path.as_str().to_string())
                        .collect(),
                    outputs: action
                        .outputs
                        .iter()
                        .map(|path| path.as_str().to_string())
                        .collect(),
                    target: action_targets.get(&action.id).copied(),
                    caps: action.caps.iter().cloned().collect(),
                    pools: action_pools(action),
                    legacy_wrapper: action.legacy_wrapper,
                    plugin: action.plugin.map(|plugin| plugin.id),
                    compiler_owned: action.compiler_owned,
                })
                .collect(),
            files: file_index
                .into_iter()
                .map(|(path, ownership)| BuildGraphFile {
                    path,
                    owner: ownership.owner,
                    consumers: ownership.consumers,
                    targets: ownership.targets,
                })
                .collect(),
        }
    }

    pub fn file_ownership(&self, path: impl AsRef<str>) -> FileOwnership {
        let path = path.as_ref().to_string();
        self.file_index(&self.action_targets())
            .remove(&path)
            .unwrap_or(FileOwnership {
                path,
                owner: None,
                consumers: Vec::new(),
                targets: Vec::new(),
            })
    }

    pub fn explain_target(&self, target: impl Into<TargetRef>) -> Option<BuildExplanation> {
        let target = self.target(target)?;
        Some(BuildExplanation {
            subject: BuildGraphSubject::Target(target.id),
            label: target.name.clone(),
            provenance: vec![
                format!("kind={:?}", target.kind),
                format!("sources={}", target.sources.len()),
                format!("deps={}", target.deps.len()),
                format!("actions={}", target.actions.len()),
            ],
        })
    }

    pub fn explain_target_named(&self, name: &str) -> Option<BuildExplanation> {
        let target = self.targets.iter().find(|target| target.name == name)?;
        self.explain_target(TargetRef { id: target.id, context: self.context })
    }

    pub fn explain_action(&self, action: ActionHandle) -> Option<BuildExplanation> {
        let action = self.action(action)?;
        let mut provenance = vec![
            format!("argv={}", action.argv.join(" ")),
            format!("inputs={}", action.inputs.len()),
            format!("outputs={}", action.outputs.len()),
            format!("cache={:?}", action.cache),
        ];
        if let Some(wrapper) = action.legacy_wrapper {
            provenance.push(format!("legacy={}", wrapper.as_str()));
        }
        if let Some(plugin) = action.plugin {
            provenance.push(format!("plugin={}", self.plugins[plugin.id.0].name));
        }
        Some(BuildExplanation {
            subject: BuildGraphSubject::Action(action.id),
            label: action.name.clone(),
            provenance,
        })
    }

    pub fn explain_action_named(&self, name: &str) -> Option<BuildExplanation> {
        let action = self.actions.iter().find(|action| action.name == name)?;
        self.explain_action(ActionHandle { id: action.id, context: self.context })
    }

    pub fn explain_file(&self, path: impl AsRef<str>) -> BuildExplanation {
        let ownership = self.file_ownership(path.as_ref());
        let mut provenance = vec![
            format!("owner={:?}", ownership.owner),
            format!("consumers={:?}", ownership.consumers),
            format!("targets={:?}", ownership.targets),
        ];
        if let Some(module) = self
            .generated_modules
            .iter()
            .find(|module| module.path.as_str() == ownership.path)
        {
            provenance.push(format!("generated={}", module.name));
            provenance.push(format!("digest={}", module.source_digest.as_str()));
        }
        BuildExplanation {
            subject: BuildGraphSubject::File,
            label: ownership.path,
            provenance,
        }
    }

    pub fn why_rebuilt(
        &self,
        action: ActionHandle,
        status: ActionCacheStatus,
    ) -> Result<RebuildExplanation, BuildError> {
        if action.context != self.context {
            return Err(BuildError::UnknownAction(action.id));
        }
        let action_ref = self
            .actions
            .get(action.id.0)
            .ok_or(BuildError::UnknownAction(action.id))?;
        Ok(RebuildExplanation {
            action: action.id,
            action_name: action_ref.name.clone(),
            status,
            reason: cache_status_reason(status).to_string(),
        })
    }

    /// Explain the most recent real execution of a named action. Inspection
    /// reads execution provenance only; it never runs an action or probes the
    /// ambient machine.
    pub fn last_rebuild_explanation(
        &self,
        project_root: &Path,
        action_name: &str,
    ) -> io::Result<Option<RebuildExplanation>> {
        let Some(action) = self.actions.iter().find(|action| action.name == action_name) else {
            return Ok(None);
        };
        let Some(record) = read_last_rebuild_record(project_root, action.id, action_name)? else {
            return Ok(None);
        };
        let mut explanation = self.why_rebuilt(
            ActionHandle {
                id: action.id,
                context: self.context,
            },
            record.status,
        )
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, format!("{error:?}")))?;
        if let Some(code) = record.failed_exit_code {
            explanation.reason =
                format!("action failed with exit code {code} after {}", explanation.reason);
        }
        Ok(Some(explanation))
    }

    pub fn execution_model(&self) -> Result<BuildExecutionModel, BuildError> {
        let selected = self.selected_action_ids()?;
        let prereqs = self.action_prereqs_for(&selected)?;
        let stages = execution_stages(&prereqs)?;
        let action_targets = self.action_targets();
        let nodes = self
            .actions
            .iter()
            .filter(|action| selected.contains(&action.id))
            .map(|action| BuildExecutionNode {
                action: action.id,
                name: action.name.clone(),
                target: action_targets.get(&action.id).copied(),
                prerequisites: prereqs.get(&action.id).cloned().unwrap_or_default(),
                pools: action_pools(action),
            })
            .collect::<Vec<_>>();
        let events = stages
            .iter()
            .flat_map(|stage| {
                stage
                    .actions
                    .iter()
                    .map(move |action| BuildExecutionEvent::Ready {
                        action: *action,
                        stage: stage.index,
                    })
            })
            .collect();
        let selected_actions = self.actions.iter().filter(|action| selected.contains(&action.id)).cloned().collect::<Vec<_>>();
        let metrics = execution_metrics(&selected_actions, &stages);
        Ok(BuildExecutionModel {
            pools: self.resource_pools(),
            nodes,
            stages,
            events,
            console_order: self.actions.iter().filter(|action| selected.contains(&action.id)).map(|action| action.id).collect(),
            metrics,
        })
    }

    pub fn execution_report(
        &self,
        outcomes: &[(ActionHandle, ActionOutcome)],
    ) -> Result<BuildExecutionReport, BuildError> {
        let selected = self.selected_action_ids()?;
        let prereqs = self.action_prereqs_for(&selected)?;
        let stages = execution_stages(&prereqs)?;
        let mut supplied = BTreeMap::new();
        for (action, outcome) in outcomes {
            if action.context != self.context || action.id.0 >= self.actions.len() {
                return Err(BuildError::UnknownAction(action.id));
            }
            supplied.insert(action.id, *outcome);
        }

        let mut failed = BTreeSet::new();
        let mut finished = BTreeSet::new();
        let mut events = Vec::new();
        let selected_actions = self.actions.iter().filter(|action| selected.contains(&action.id)).cloned().collect::<Vec<_>>();
        let mut metrics = execution_metrics(&selected_actions, &stages);
        for stage in &stages {
            for action in &stage.actions {
                if let Some(failed_prereq) = prereqs
                    .get(action)
                    .into_iter()
                    .flat_map(|deps| deps.iter())
                    .find(|dep| failed.contains(*dep))
                {
                    failed.insert(*action);
                    metrics.cancelled_actions += 1;
                    events.push(BuildExecutionEvent::Cancelled {
                        action: *action,
                        failed_prereq: *failed_prereq,
                    });
                    continue;
                }
                match supplied.get(action) {
                    Some(outcome) => {
                        finished.insert(*action);
                        if matches!(outcome, ActionOutcome::Failed { .. }) {
                            failed.insert(*action);
                            metrics.failed_actions += 1;
                        }
                        if matches!(outcome, ActionOutcome::RestoredFromCache) {
                            metrics.cache_restored_actions += 1;
                        }
                        events.push(BuildExecutionEvent::Finished {
                            action: *action,
                            outcome: *outcome,
                        });
                    }
                    None => {
                        metrics.pending_actions += 1;
                        events.push(BuildExecutionEvent::Pending { action: *action });
                    }
                }
            }
        }
        Ok(BuildExecutionReport { events, metrics })
    }

    pub(super) fn action_targets(&self) -> BTreeMap<ActionId, TargetId> {
        let mut out = BTreeMap::new();
        for target in &self.targets {
            for action in &target.actions {
                out.entry(action.id).or_insert(target.id);
            }
        }
        out
    }

    fn file_index(
        &self,
        action_targets: &BTreeMap<ActionId, TargetId>,
    ) -> BTreeMap<String, FileOwnership> {
        let mut files = BTreeMap::<String, FileOwnership>::new();
        for action in &self.actions {
            for output in &action.outputs {
                let entry = files
                    .entry(output.as_str().to_string())
                    .or_insert(FileOwnership {
                        path: output.as_str().to_string(),
                        owner: None,
                        consumers: Vec::new(),
                        targets: Vec::new(),
                    });
                entry.owner = Some(action.id);
            }
            for input in &action.inputs {
                let entry = files
                    .entry(input.as_str().to_string())
                    .or_insert(FileOwnership {
                        path: input.as_str().to_string(),
                        owner: None,
                        consumers: Vec::new(),
                        targets: Vec::new(),
                    });
                entry.consumers.push(action.id);
            }
        }
        for target in &self.targets {
            for path in target
                .sources
                .iter()
                .chain(target.inputs.iter())
                .chain(target.outputs.iter())
            {
                let entry = files
                    .entry(path.as_str().to_string())
                    .or_insert(FileOwnership {
                        path: path.as_str().to_string(),
                        owner: None,
                        consumers: Vec::new(),
                        targets: Vec::new(),
                    });
                entry.targets.push(target.id);
            }
            for action in &target.actions {
                if let Some(target_id) = action_targets.get(&action.id) {
                    for path in self.actions[action.id.0]
                        .inputs
                        .iter()
                        .chain(self.actions[action.id.0].outputs.iter())
                    {
                        let entry =
                            files
                                .entry(path.as_str().to_string())
                                .or_insert(FileOwnership {
                                    path: path.as_str().to_string(),
                                    owner: None,
                                    consumers: Vec::new(),
                                    targets: Vec::new(),
                                });
                        if !entry.targets.contains(target_id) {
                            entry.targets.push(*target_id);
                        }
                    }
                }
            }
        }
        files
    }

    fn action_prereqs(&self) -> Result<BTreeMap<ActionId, Vec<ActionId>>, BuildError> {
        let mut out = self
            .actions
            .iter()
            .map(|action| (action.id, BTreeSet::<ActionId>::new()))
            .collect::<BTreeMap<_, _>>();
        for target in &self.targets {
            let mut deps = BTreeSet::new();
            for dep in &target.deps {
                collect_target_actions(self, dep.id, &mut BTreeSet::new(), &mut deps)?;
            }
            for action in &target.actions {
                out.entry(action.id)
                    .or_default()
                    .extend(deps.iter().copied());
            }
        }
        let action_targets = self.action_targets();
        for ownership in self.file_index(&action_targets).values() {
            let Some(owner) = ownership.owner else {
                continue;
            };
            for consumer in &ownership.consumers {
                if *consumer != owner {
                    out.entry(*consumer).or_default().insert(owner);
                }
            }
        }
        Ok(out
            .into_iter()
            .map(|(action, deps)| (action, deps.into_iter().collect()))
            .collect())
    }

    fn action_prereqs_for(
        &self,
        selected: &BTreeSet<ActionId>,
    ) -> Result<BTreeMap<ActionId, Vec<ActionId>>, BuildError> {
        Ok(self
            .action_prereqs()?
            .into_iter()
            .filter(|(action, _)| selected.contains(action))
            .map(|(action, deps)| {
                (action, deps.into_iter().filter(|dep| selected.contains(dep)).collect())
            })
            .collect())
    }
}

fn compiler_package_path_name(name: &str) -> String {
    let mut out = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if out.is_empty() {
        out.push_str("package");
    }
    out
}
