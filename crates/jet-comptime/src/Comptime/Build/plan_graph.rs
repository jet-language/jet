use super::actions_policy::{
    BuildAction, BuildCapability, BuildResourcePool, BuildResourcePoolSpec, LegacyWrapperKind,
};
use super::cache_cas::{ActionCacheStatus, ActionKey, ActionOutcome, ContentDigest};
use super::handles::{ActionId, PluginId, TargetId, TargetRef};
use super::plugins_modules::{BuildGeneratedModule, BuildPlugin};
use super::provenance_toolchains::{BuildProbe, BuildSigningIdentity, BuildToolchain};
use super::targets::{BuildTarget, TargetKind};
use jet_foundation::SHA256::sha256_hex;

/// The compiler-owned nodes that share the BuildPlan graph with declared
/// actions. Their keys contain only logical names and content digests, never
/// checkout-specific paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BuildNodeKind {
    Check,
    Compile,
    Link,
}
impl BuildNodeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Compile => "compile",
            Self::Link => "link",
        }
    }
}

/// A static compiler node in a BuildPlan. Runtime duration and cache reason
/// live in the store's BuildRecord; this value is the graph identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildPlanNode {
    pub kind: BuildNodeKind,
    pub key: String,
    pub subject: String,
    pub inputs: Vec<String>,
    pub input_digests: Vec<(String, String)>,
}

impl BuildPlanNode {
    pub fn new(
        kind: BuildNodeKind,
        subject: impl Into<String>,
        mut input_digests: Vec<(String, String)>,
    ) -> Self {
        input_digests.sort();
        input_digests.dedup();
        let inputs = input_digests
            .iter()
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();
        let subject = subject.into();
        let key = compiler_node_key(kind, &subject, &input_digests);
        Self {
            kind,
            key,
            subject,
            inputs,
            input_digests,
        }
    }
}

/// Canonical content key for a compiler-owned BuildPlan node.
pub fn compiler_node_key(
    kind: BuildNodeKind,
    subject: &str,
    input_digests: &[(String, String)],
) -> String {
    let mut inputs = input_digests.to_vec();
    inputs.sort();
    let mut bytes = Vec::new();
    for value in ["jet.action-key.v2", "compiler-node", kind.as_str(), subject] {
        bytes.extend_from_slice(&(value.len() as u64).to_be_bytes());
        bytes.extend_from_slice(value.as_bytes());
    }
    for (name, digest) in inputs {
        for value in [name, digest] {
            bytes.extend_from_slice(&(value.len() as u64).to_be_bytes());
            bytes.extend_from_slice(value.as_bytes());
        }
    }
    sha256_hex(&bytes)
}

pub(super) const MAX_ACTIONS: usize = 100_000;

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
    /// Compiler-owned Check/Compile/Link nodes in deterministic order.
    pub(super) compiler_nodes: Vec<BuildPlanNode>,
    pub(super) default: Option<TargetRef>,
    /// D-CONF-SPLIT1=A: computed fact writers produced by the selected
    /// `fn build`. They remain compile-time data and enter action identity so
    /// a changed contribution cannot reuse an old runtime artifact.
    pub fact_contributions: Vec<jet_foundation::Policy::FactContribution>,
}

impl BuildPlan {
    pub fn fact_contributions(&self) -> &[jet_foundation::Policy::FactContribution] {
        &self.fact_contributions
    }

    pub fn compiler_nodes(&self) -> &[BuildPlanNode] {
        &self.compiler_nodes
    }

    pub fn set_compiler_nodes(&mut self, mut nodes: Vec<BuildPlanNode>) {
        nodes.sort_by(|left, right| {
            left.kind
                .cmp(&right.kind)
                .then_with(|| left.subject.cmp(&right.subject))
                .then_with(|| left.key.cmp(&right.key))
        });
        self.compiler_nodes = nodes;
    }
}

/// One compiler-owned package artifact input to a generated compile action.
/// The source digest is captured after the ordinary loader/parser/sema pass;
/// dependency names become declared artifact-file inputs in the plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilerPackageSpec {
    pub name: String,
    pub source_digest: ContentDigest,
    pub dependencies: Vec<String>,
}

impl CompilerPackageSpec {
    pub fn new(
        name: impl Into<String>,
        source_digest: ContentDigest,
        dependencies: Vec<String>,
    ) -> Self {
        CompilerPackageSpec {
            name: name.into(),
            source_digest,
            dependencies,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildGraph {
    pub targets: Vec<BuildGraphTarget>,
    pub actions: Vec<BuildGraphAction>,
    pub files: Vec<BuildGraphFile>,
    pub nodes: Vec<BuildPlanNode>,
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
    pub compiler_owned: bool,
    /// The deterministic recipe key computed from the checked plan facts.
    ///
    /// This is the plan key, before runtime input snapshots and effective
    /// execution policy are joined by the store.
    pub key: ActionKey,
    /// Runtime cache evidence is absent from a static plan projection.
    /// `None` means unknown, rather than a cache miss.
    pub cache_hit: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildGraphActionKey {
    pub action: String,
    pub key: ActionKey,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildGraphFileDelta {
    pub path: String,
    pub before: Option<BuildGraphFile>,
    pub after: Option<BuildGraphFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildGraphKeyDelta {
    pub action: String,
    pub before: Option<ActionKey>,
    pub after: Option<ActionKey>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildGraphCacheDelta {
    pub action: String,
    pub before: Option<bool>,
    pub after: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BuildGraphDiff {
    /// Files whose declared ownership, consumers, or target membership changed.
    pub file_deltas: Vec<BuildGraphFileDelta>,
    /// Outputs transitively affected by changed action keys or input paths.
    pub affected_files: Vec<String>,
    /// Actions whose stable recipe key was added, removed, or changed.
    pub key_deltas: Vec<BuildGraphKeyDelta>,
    /// Runtime cache evidence changes. `None` remains unknown.
    pub cache_deltas: Vec<BuildGraphCacheDelta>,
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
