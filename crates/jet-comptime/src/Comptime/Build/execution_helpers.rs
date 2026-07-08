fn default_resource_pools() -> Vec<BuildResourcePoolSpec> {
    vec![
        BuildResourcePoolSpec::new(BuildResourcePool::Cpu, 0),
        BuildResourcePoolSpec::new(BuildResourcePool::Memory, 0),
        BuildResourcePoolSpec::new(BuildResourcePool::Linker, 1),
        BuildResourcePoolSpec::new(BuildResourcePool::Console, 1),
        BuildResourcePoolSpec::new(BuildResourcePool::Gpu, 1),
    ]
}

fn action_pools(action: &BuildAction) -> Vec<BuildResourcePool> {
    if action.resource_pools.is_empty() {
        vec![BuildResourcePool::Cpu]
    } else {
        action.resource_pools.iter().cloned().collect()
    }
}

fn cache_status_reason(status: ActionCacheStatus) -> &'static str {
    match status {
        ActionCacheStatus::Hit(CacheHitReason::LocalActionRecordMatched) => {
            "local action record matched"
        }
        ActionCacheStatus::Hit(CacheHitReason::DeclaredOutputsRestored) => {
            "declared outputs restored"
        }
        ActionCacheStatus::Miss(CacheMissReason::NoLocalActionRecord) => "no local action record",
        ActionCacheStatus::Miss(CacheMissReason::ActionKeyChanged) => "action key changed",
        ActionCacheStatus::Miss(CacheMissReason::DeclaredOutputMissing) => {
            "declared output missing"
        }
        ActionCacheStatus::Miss(CacheMissReason::RemoteDenied) => "remote cache denied by policy",
        ActionCacheStatus::Miss(CacheMissReason::UncachedAction) => "action is uncached",
    }
}

fn execution_stages(
    prereqs: &BTreeMap<ActionId, Vec<ActionId>>,
) -> Result<Vec<BuildExecutionStage>, BuildError> {
    let mut remaining = prereqs.keys().copied().collect::<BTreeSet<_>>();
    let mut done = BTreeSet::new();
    let mut stages = Vec::new();
    while !remaining.is_empty() {
        let ready = remaining
            .iter()
            .copied()
            .filter(|action| {
                prereqs
                    .get(action)
                    .into_iter()
                    .flat_map(|deps| deps.iter())
                    .all(|dep| done.contains(dep))
            })
            .collect::<Vec<_>>();
        if ready.is_empty() {
            return Err(BuildError::ActionDependencyCycle);
        }
        for action in &ready {
            remaining.remove(action);
            done.insert(*action);
        }
        stages.push(BuildExecutionStage {
            index: stages.len(),
            actions: ready,
        });
    }
    Ok(stages)
}

fn execution_metrics(
    actions: &[BuildAction],
    stages: &[BuildExecutionStage],
) -> BuildExecutionMetrics {
    BuildExecutionMetrics {
        actions_total: actions.len(),
        parallel_stages: stages.len(),
        max_parallel_actions: stages
            .iter()
            .map(|stage| stage.actions.len())
            .max()
            .unwrap_or(0),
        cacheable_actions: actions
            .iter()
            .filter(|action| action.cache == ActionCache::Cached)
            .count(),
        phony_actions: actions
            .iter()
            .filter(|action| action.cache == ActionCache::UncachedPhony)
            .count(),
        ..BuildExecutionMetrics::default()
    }
}

fn collect_target_actions(
    plan: &BuildPlan,
    target: TargetId,
    visiting: &mut BTreeSet<TargetId>,
    out: &mut BTreeSet<ActionId>,
) -> Result<(), BuildError> {
    if !visiting.insert(target) {
        return Err(BuildError::TargetDependencyCycle);
    }
    let target_ref = plan
        .targets
        .get(target.0)
        .ok_or(BuildError::UnknownTarget(target))?;
    for dep in &target_ref.deps {
        collect_target_actions(plan, dep.id, visiting, out)?;
    }
    out.extend(target_ref.actions.iter().map(|action| action.id));
    visiting.remove(&target);
    Ok(())
}
