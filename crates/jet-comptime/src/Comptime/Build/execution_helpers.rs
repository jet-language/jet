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
    let mut indegree = prereqs
        .iter()
        .map(|(action, dependencies)| (*action, dependencies.len()))
        .collect::<BTreeMap<_, _>>();
    let mut dependents = BTreeMap::<ActionId, Vec<ActionId>>::new();
    for (action, dependencies) in prereqs {
        for dependency in dependencies {
            dependents.entry(*dependency).or_default().push(*action);
        }
    }
    for actions in dependents.values_mut() {
        actions.sort();
    }
    let mut ready = indegree
        .iter()
        .filter_map(|(action, count)| (*count == 0).then_some(*action))
        .collect::<BTreeSet<_>>();
    let mut stages = Vec::new();
    while !ready.is_empty() {
        let current = ready.into_iter().collect::<Vec<_>>();
        for action in &current {
            remaining.remove(action);
        }
        let mut next_ready = BTreeSet::new();
        for action in &current {
            for dependent in dependents.get(action).into_iter().flatten() {
                let count = indegree
                    .get_mut(dependent)
                    .expect("dependency graph node must have an indegree");
                *count -= 1;
                if *count == 0 {
                    next_ready.insert(*dependent);
                }
            }
        }
        stages.push(BuildExecutionStage {
            index: stages.len(),
            actions: current,
        });
        ready = next_ready;
    }
    if !remaining.is_empty() {
        return Err(action_cycle_error(plan, &remaining, prereqs));
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
    use super::super::actions_policy::ActionSpec;
    use super::super::context::BuildContext;
    use super::super::errors_keys::BuildError;
    use super::super::handles::TargetRef;
    use super::super::plan_graph::MAX_ACTIONS;
    use super::super::targets::TargetSpec;

    #[test]
    fn action_admission_budget_accepts_the_limit_and_rejects_the_next_action() {
        let mut context = BuildContext::new();
        let mut target_spec = TargetSpec::new();
        for index in 0..MAX_ACTIONS {
            let mut spec = ActionSpec::cached(["true"]).with_outputs([format!("out/{index}")]);
            if index > 0 {
                spec = spec.with_inputs([format!("out/{}", index - 1)]);
            }
            let action = context
                .action(format!("scale-{index}"), spec)
                .expect("the declared action budget must be usable");
            target_spec = target_spec.with_action(action);
        }

        let error = context
            .action(
                "scale-overflow",
                ActionSpec::cached(["true"]).with_outputs(["out/overflow"]),
            )
            .expect_err("the action graph must fail closed at its scale budget");
        assert!(matches!(
            error,
            BuildError::PackagedPlugin(message) if message.contains("100000")
        ));
        let target = context
            .add_library("scale", target_spec)
            .expect("the limit-sized graph must remain buildable");
        let plan = context.plan_with_default(target).unwrap();
        let model = plan
            .execution_model()
            .expect("the limit-sized graph must schedule");
        assert_eq!(model.metrics.actions_total, MAX_ACTIONS);
        assert_eq!(model.stages.len(), MAX_ACTIONS);
        assert!(model.stages.iter().all(|stage| stage.actions.len() == 1));
    }

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
