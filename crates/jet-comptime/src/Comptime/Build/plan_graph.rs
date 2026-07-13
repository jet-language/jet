use super::actions_policy::{BuildAction, BuildCapability, BuildResourcePool, BuildResourcePoolSpec, LegacyWrapperKind};
use super::cache_cas::{ActionCacheStatus, ActionOutcome};
use super::handles::{ActionId, PluginId, TargetId, TargetRef};
use super::plugins_modules::{BuildGeneratedModule, BuildPlugin};
use super::provenance_toolchains::{BuildProbe, BuildSigningIdentity, BuildToolchain};
use super::targets::{BuildTarget, TargetKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildPlan {
    pub(super) context: u64,
    pub(super) targets: Vec<BuildTarget>,
    pub(super) actions: Vec<BuildAction>,
    pub(super) toolchains: Vec<BuildToolchain>,
    pub(super) signing_identities: Vec<BuildSigningIdentity>,
    pub(super) probes: Vec<BuildProbe>,
    pub(super) plugins: Vec<BuildPlugin>,
    pub(super) generated_modules: Vec<BuildGeneratedModule>,
    pub(super) default: Option<TargetRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildGraph {
    pub targets: Vec<BuildGraphTarget>,
    pub actions: Vec<BuildGraphAction>,
    pub files: Vec<BuildGraphFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildGraphTarget {
    pub id: TargetId,
    pub name: String,
    pub kind: TargetKind,
    pub deps: Vec<TargetId>,
    pub actions: Vec<ActionId>,
    pub files: Vec<String>,
    pub plugin: Option<PluginId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildGraphAction {
    pub id: ActionId,
    pub name: String,
    pub kind: super::actions_policy::ActionKind,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub target: Option<TargetId>,
    pub caps: Vec<BuildCapability>,
    pub pools: Vec<BuildResourcePool>,
    pub legacy_wrapper: Option<LegacyWrapperKind>,
    pub plugin: Option<PluginId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildGraphFile {
    pub path: String,
    pub owner: Option<ActionId>,
    pub consumers: Vec<ActionId>,
    pub targets: Vec<TargetId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildGraphSubject {
    Target(TargetId),
    Action(ActionId),
    File,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildExplanation {
    pub subject: BuildGraphSubject,
    pub label: String,
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileOwnership {
    pub path: String,
    pub owner: Option<ActionId>,
    pub consumers: Vec<ActionId>,
    pub targets: Vec<TargetId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebuildExplanation {
    pub action: ActionId,
    pub action_name: String,
    pub status: ActionCacheStatus,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildExecutionModel {
    pub pools: Vec<BuildResourcePoolSpec>,
    pub nodes: Vec<BuildExecutionNode>,
    pub stages: Vec<BuildExecutionStage>,
    pub events: Vec<BuildExecutionEvent>,
    pub console_order: Vec<ActionId>,
    pub metrics: BuildExecutionMetrics,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildExecutionNode {
    pub action: ActionId,
    pub name: String,
    pub target: Option<TargetId>,
    pub prerequisites: Vec<ActionId>,
    pub pools: Vec<BuildResourcePool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildExecutionStage {
    pub index: usize,
    pub actions: Vec<ActionId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildExecutionEvent {
    Ready {
        action: ActionId,
        stage: usize,
    },
    Finished {
        action: ActionId,
        outcome: ActionOutcome,
    },
    Cancelled {
        action: ActionId,
        failed_prereq: ActionId,
    },
    Pending {
        action: ActionId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BuildExecutionMetrics {
    pub actions_total: usize,
    pub parallel_stages: usize,
    pub max_parallel_actions: usize,
    pub cacheable_actions: usize,
    pub phony_actions: usize,
    pub failed_actions: usize,
    pub cancelled_actions: usize,
    pub cache_restored_actions: usize,
    pub pending_actions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildExecutionReport {
    pub events: Vec<BuildExecutionEvent>,
    pub metrics: BuildExecutionMetrics,
}
