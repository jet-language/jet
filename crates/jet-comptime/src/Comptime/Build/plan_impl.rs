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

    pub fn action_key(&self, action: ActionHandle) -> Result<ActionKey, BuildError> {
        self.action_key_with_inputs(action, &[])
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
        default_resource_pools()
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

    pub fn explain_file(&self, path: impl AsRef<str>) -> BuildExplanation {
        let ownership = self.file_ownership(path.as_ref());
        BuildExplanation {
            subject: BuildGraphSubject::File,
            label: ownership.path,
            provenance: vec![
                format!("owner={:?}", ownership.owner),
                format!("consumers={:?}", ownership.consumers),
                format!("targets={:?}", ownership.targets),
            ],
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

    pub fn execution_model(&self) -> Result<BuildExecutionModel, BuildError> {
        let prereqs = self.action_prereqs()?;
        let stages = execution_stages(&prereqs)?;
        let action_targets = self.action_targets();
        let nodes = self
            .actions
            .iter()
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
        let metrics = execution_metrics(&self.actions, &stages);
        Ok(BuildExecutionModel {
            pools: self.resource_pools(),
            nodes,
            stages,
            events,
            console_order: self.actions.iter().map(|action| action.id).collect(),
            metrics,
        })
    }

    pub fn execution_report(
        &self,
        outcomes: &[(ActionHandle, ActionOutcome)],
    ) -> Result<BuildExecutionReport, BuildError> {
        let prereqs = self.action_prereqs()?;
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
        let mut metrics = execution_metrics(&self.actions, &stages);
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

    fn action_targets(&self) -> BTreeMap<ActionId, TargetId> {
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
}
