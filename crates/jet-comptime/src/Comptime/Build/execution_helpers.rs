use super::actions_policy::{ActionCache, BuildAction, BuildResourcePool, BuildResourcePoolSpec};
use super::cache_cas::{ActionCacheStatus, CacheHitReason, CacheMissReason};
use super::errors_keys::{action_cycle_error, target_cycle_error, BuildError};
use super::handles::{ActionId, TargetId};
use super::plan_graph::{BuildExecutionMetrics, BuildExecutionStage, BuildPlan};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn default_resource_pools() -> Vec<BuildResourcePoolSpec> {
    vec![
        BuildResourcePoolSpec::new(BuildResourcePool::CPU, 0),
        BuildResourcePoolSpec::new(BuildResourcePool::Memory, 0),
        BuildResourcePoolSpec::new(BuildResourcePool::Linker, 1),
        BuildResourcePoolSpec::new(BuildResourcePool::Console, 1),
        BuildResourcePoolSpec::new(BuildResourcePool::GPU, 1),
    ]
}

pub(super) fn action_pools(action: &BuildAction) -> Vec<BuildResourcePool> {
    if action.resource_pools.is_empty() {
        vec![BuildResourcePool::CPU]
    } else {
        action.resource_pools.iter().cloned().collect()
    }
}

pub(super) fn cache_status_reason(status: ActionCacheStatus) -> &'static str {
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
        ActionCacheStatus::Miss(CacheMissReason::CacheRecordInvalid) => {
            "local cache record or blob is invalid"
        }
        ActionCacheStatus::Miss(CacheMissReason::CacheRestoreFailed) => {
            "local cache output could not be restored"
        }
        ActionCacheStatus::Miss(CacheMissReason::RemoteDenied) => "remote cache denied by policy",
        ActionCacheStatus::Miss(CacheMissReason::UncachedAction) => "action is uncached",
        ActionCacheStatus::Miss(CacheMissReason::FrontEndIncomplete) => {
            "cache lookup blocked until parser, sema, policy, and diagnostics complete"
        }
    }
}

pub(super) fn execution_stages(
    plan: &BuildPlan,
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
            return Err(action_cycle_error(plan, &remaining, prereqs));
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

pub(super) fn execution_metrics(
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

pub(super) fn collect_target_actions(
    plan: &BuildPlan,
    target: TargetId,
    visiting: &mut Vec<TargetId>,
    out: &mut BTreeSet<ActionId>,
) -> Result<(), BuildError> {
    if visiting.contains(&target) {
        return Err(target_cycle_error(plan, visiting, target));
    }
    visiting.push(target);
    let target_ref = plan
        .targets
        .get(target.0)
        .ok_or(BuildError::UnknownTarget(target))?;
    for dep in &target_ref.deps {
        collect_target_actions(plan, dep.id, visiting, out)?;
    }
    out.extend(target_ref.actions.iter().map(|action| action.id));
    visiting.pop();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::context::BuildContext;
    use super::super::errors_keys::BuildError;
    use super::super::handles::TargetRef;
    use super::super::targets::TargetSpec;

    /// #1522 criterion 4: a target cycle is reported as the whole loop, in
    /// the order the walk found it.
    ///
    /// `fn build` cannot express this — a target dependency needs a target
    /// handle, and a handle only exists once its target is registered — so
    /// the loop is closed on the finished plan. The detection and rendering
    /// under test are the ones every `selected_action_ids` caller reaches.
    #[test]
    fn target_dependency_cycle_names_every_node_in_traversal_order() {
        let mut context = BuildContext::new();
        let core = context.add_library("core", TargetSpec::new()).unwrap();
        let app = context
            .add_executable("app", TargetSpec::new().with_dep(core))
            .unwrap();
        let mut plan = context.plan_with_default(app).unwrap();
        plan.targets[core.id().0].deps.push(TargetRef::from(app));

        match plan.selected_action_ids().unwrap_err() {
            BuildError::TargetDependencyCycle(cycle) => {
                assert_eq!(
                    cycle.nodes().to_vec(),
                    vec!["app", "core", "app"],
                    "the cycle must be the walked path, not the visited set"
                );
                assert_eq!(cycle.chain(), "`app` -> `core` -> `app`");
            }
            other => panic!("a cyclic target graph must report a target cycle: {other:?}"),
        }
    }
}
