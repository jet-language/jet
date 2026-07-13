use super::errors_keys::BuildError;
use super::handles::{ActionId, PluginHandle, ProbeHandle, SigningIdentityHandle, ToolchainHandle};
use super::targets::BuildPath;
use std::collections::{BTreeMap, BTreeSet};

pub type BuildCapability = crate::BuildEffect;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BuildResourcePool {
    Cpu,
    Memory,
    Linker,
    Console,
    Gpu,
    Custom(String),
}

impl BuildResourcePool {
    pub fn as_str(&self) -> &str {
        match self {
            BuildResourcePool::Cpu => "cpu",
            BuildResourcePool::Memory => "memory",
            BuildResourcePool::Linker => "linker",
            BuildResourcePool::Console => "console",
            BuildResourcePool::Gpu => "gpu",
            BuildResourcePool::Custom(name) => name.as_str(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildResourcePoolSpec {
    pub pool: BuildResourcePool,
    pub slots: usize,
}

impl BuildResourcePoolSpec {
    pub fn new(pool: BuildResourcePool, slots: usize) -> Self {
        BuildResourcePoolSpec { pool, slots }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyWrapperKind {
    CMake,
    Make,
    Gradle,
    Npm,
    Cargo,
}

impl LegacyWrapperKind {
    pub fn as_str(self) -> &'static str {
        match self {
            LegacyWrapperKind::CMake => "cmake",
            LegacyWrapperKind::Make => "make",
            LegacyWrapperKind::Gradle => "gradle",
            LegacyWrapperKind::Npm => "npm",
            LegacyWrapperKind::Cargo => "cargo",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionCache {
    Cached,
    UncachedPhony,
}

/// Distinct executable identities under one BuildPlan (E4-JP2 / #419).
/// Compile / docs / debug / source-archive never share a cache key even when
/// argv and declared paths match — each surface observes different outputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ActionKind {
    Compile,
    Docs,
    Debug,
    SourceArchive,
    Generic,
}

impl ActionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ActionKind::Compile => "compile",
            ActionKind::Docs => "docs",
            ActionKind::Debug => "debug",
            ActionKind::SourceArchive => "source-archive",
            ActionKind::Generic => "generic",
        }
    }

    /// Exact source bytes remain identity inputs when these surfaces can
    /// observe them (docs, doctests, diagnostics/line maps, debug info,
    /// publication / source archives).
    pub fn observes_exact_source(self) -> bool {
        matches!(
            self,
            ActionKind::Compile
                | ActionKind::Docs
                | ActionKind::Debug
                | ActionKind::SourceArchive
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionSpec {
    pub inputs: Vec<BuildPath>,
    pub outputs: Vec<BuildPath>,
    pub argv: Vec<String>,
    pub env: BTreeMap<String, String>,
    /// Only allowlisted env keys enter the action identity. Empty means the
    /// declared `env` map itself is the allowlist (no ambient leakage).
    pub env_allowlist: BTreeSet<String>,
    pub caps: BTreeSet<BuildCapability>,
    pub cache: ActionCache,
    pub kind: ActionKind,
    pub toolchain: Option<ToolchainHandle>,
    pub probes: Vec<ProbeHandle>,
    pub signing_identity: Option<SigningIdentityHandle>,
    pub labels: BTreeMap<String, String>,
    /// Helper tool versions (formatter, docgen, archive helper, …) keyed into
    /// the complete CAS identity.
    pub helper_versions: BTreeMap<String, String>,
    pub resource_pools: BTreeSet<BuildResourcePool>,
    pub legacy_wrapper: Option<LegacyWrapperKind>,
}

impl ActionSpec {
    pub fn cached<I, S>(argv: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        ActionSpec {
            inputs: Vec::new(),
            outputs: Vec::new(),
            argv: argv.into_iter().map(Into::into).collect(),
            env: BTreeMap::new(),
            env_allowlist: BTreeSet::new(),
            caps: BTreeSet::new(),
            cache: ActionCache::Cached,
            kind: ActionKind::Generic,
            toolchain: None,
            probes: Vec::new(),
            signing_identity: None,
            labels: BTreeMap::new(),
            helper_versions: BTreeMap::new(),
            resource_pools: BTreeSet::new(),
            legacy_wrapper: None,
        }
    }

    pub fn uncached_phony<I, S>(argv: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        ActionSpec {
            cache: ActionCache::UncachedPhony,
            ..Self::cached(argv)
        }
    }

    pub fn with_inputs<I, S>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.inputs
            .extend(paths.into_iter().map(|p| BuildPath(p.into())));
        self
    }

    pub fn with_outputs<I, S>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.outputs
            .extend(paths.into_iter().map(|p| BuildPath(p.into())));
        self
    }

    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    pub fn with_env_allowlist<I, S>(mut self, keys: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.env_allowlist
            .extend(keys.into_iter().map(Into::into));
        self
    }

    pub fn with_kind(mut self, kind: ActionKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn with_helper_version(
        mut self,
        helper: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        self.helper_versions.insert(helper.into(), version.into());
        self
    }

    pub fn with_cap(mut self, cap: BuildCapability) -> Self {
        self.caps.insert(cap);
        self
    }

    pub fn with_toolchain(mut self, toolchain: ToolchainHandle) -> Self {
        self.toolchain = Some(toolchain);
        self
    }

    pub fn with_probe(mut self, probe: ProbeHandle) -> Self {
        self.probes.push(probe);
        self
    }

    pub fn with_signing_identity(mut self, identity: SigningIdentityHandle) -> Self {
        self.signing_identity = Some(identity);
        self
    }

    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    pub fn with_pool(mut self, pool: BuildResourcePool) -> Self {
        self.resource_pools.insert(pool);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildAction {
    pub id: ActionId,
    pub name: String,
    pub inputs: Vec<BuildPath>,
    pub outputs: Vec<BuildPath>,
    pub argv: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub env_allowlist: BTreeSet<String>,
    pub caps: BTreeSet<BuildCapability>,
    pub cache: ActionCache,
    pub kind: ActionKind,
    pub toolchain: ToolchainHandle,
    pub probes: Vec<ProbeHandle>,
    pub signing_identity: Option<SigningIdentityHandle>,
    pub labels: BTreeMap<String, String>,
    pub helper_versions: BTreeMap<String, String>,
    pub resource_pools: BTreeSet<BuildResourcePool>,
    pub legacy_wrapper: Option<LegacyWrapperKind>,
    pub plugin: Option<PluginHandle>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicySetting {
    Allow,
    Deny(String),
}

impl PolicySetting {
    pub fn deny(reason: impl Into<String>) -> Self {
        PolicySetting::Deny(reason.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildPolicy {
    pub legacy_wrappers: PolicySetting,
    pub wasm_plugins: PolicySetting,
    pub plugin_grants: BTreeMap<String, BTreeSet<BuildCapability>>,
}

impl BuildPolicy {
    pub fn allow_all() -> Self {
        BuildPolicy {
            legacy_wrappers: PolicySetting::Allow,
            wasm_plugins: PolicySetting::Allow,
            plugin_grants: BTreeMap::new(),
        }
    }

    pub fn deny_legacy_wrappers(reason: impl Into<String>) -> Self {
        BuildPolicy {
            legacy_wrappers: PolicySetting::deny(reason),
            ..Self::allow_all()
        }
    }

    pub fn deny_wasm_plugins(reason: impl Into<String>) -> Self {
        BuildPolicy {
            wasm_plugins: PolicySetting::deny(reason),
            ..Self::allow_all()
        }
    }

    pub fn with_plugin_grant(mut self, plugin: impl Into<String>, cap: BuildCapability) -> Self {
        self.plugin_grants
            .entry(plugin.into())
            .or_default()
            .insert(cap);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyExplanation {
    pub subject: String,
    pub allowed: bool,
    pub reason: String,
    pub required_caps: Vec<BuildCapability>,
}

impl PolicyExplanation {
    fn allowed(subject: impl Into<String>, caps: Vec<BuildCapability>) -> Self {
        PolicyExplanation {
            subject: subject.into(),
            allowed: true,
            reason: "policy allows this declared authority".to_string(),
            required_caps: caps,
        }
    }

    pub(super) fn denied(
        subject: impl Into<String>,
        reason: impl Into<String>,
        caps: Vec<BuildCapability>,
    ) -> Self {
        PolicyExplanation {
            subject: subject.into(),
            allowed: false,
            reason: reason.into(),
            required_caps: caps,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyWrapperSpec {
    pub kind: LegacyWrapperKind,
    pub argv: Vec<String>,
    pub inputs: Vec<BuildPath>,
    pub outputs: Vec<BuildPath>,
    pub caps: BTreeSet<BuildCapability>,
    pub env: BTreeMap<String, String>,
    pub labels: BTreeMap<String, String>,
}

impl LegacyWrapperSpec {
    pub fn cmake<I, S>(argv: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::new(LegacyWrapperKind::CMake, argv)
    }

    pub fn make<I, S>(argv: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::new(LegacyWrapperKind::Make, argv)
    }

    pub fn gradle<I, S>(argv: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::new(LegacyWrapperKind::Gradle, argv)
    }

    pub fn npm<I, S>(argv: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::new(LegacyWrapperKind::Npm, argv)
    }

    pub fn cargo<I, S>(argv: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::new(LegacyWrapperKind::Cargo, argv)
    }

    fn new<I, S>(kind: LegacyWrapperKind, argv: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        LegacyWrapperSpec {
            kind,
            argv: argv.into_iter().map(Into::into).collect(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            caps: BTreeSet::new(),
            env: BTreeMap::new(),
            labels: BTreeMap::new(),
        }
    }

    pub fn with_inputs<I, S>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.inputs
            .extend(paths.into_iter().map(|p| BuildPath(p.into())));
        self
    }

    pub fn with_outputs<I, S>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.outputs
            .extend(paths.into_iter().map(|p| BuildPath(p.into())));
        self
    }

    pub fn with_cap(mut self, cap: BuildCapability) -> Self {
        self.caps.insert(cap);
        self
    }

    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    pub fn explain(&self, policy: &BuildPolicy) -> PolicyExplanation {
        let subject = format!("legacy wrapper {}", self.kind.as_str());
        let caps = self.caps.iter().cloned().collect();
        match &policy.legacy_wrappers {
            PolicySetting::Allow => PolicyExplanation::allowed(subject, caps),
            PolicySetting::Deny(reason) => PolicyExplanation::denied(subject, reason, caps),
        }
    }

    pub fn into_action_spec(self, policy: &BuildPolicy) -> Result<ActionSpec, BuildError> {
        if let PolicySetting::Deny(_) = &policy.legacy_wrappers {
            return Err(BuildError::PolicyDenied(self.explain(policy)));
        }
        if self.inputs.is_empty() {
            return Err(BuildError::LegacyWrapperWithoutInputs(self.kind));
        }
        if self.outputs.is_empty() {
            return Err(BuildError::LegacyWrapperWithoutOutputs(self.kind));
        }
        if self.caps.is_empty() {
            return Err(BuildError::LegacyWrapperWithoutCaps(self.kind));
        }
        let mut labels = self.labels;
        labels.insert("legacy.wrapper".to_string(), self.kind.as_str().to_string());
        Ok(ActionSpec {
            inputs: self.inputs,
            outputs: self.outputs,
            argv: self.argv,
            env: self.env,
            env_allowlist: BTreeSet::new(),
            caps: self.caps,
            cache: ActionCache::Cached,
            kind: ActionKind::Generic,
            toolchain: None,
            probes: Vec::new(),
            signing_identity: None,
            labels,
            helper_versions: BTreeMap::new(),
            resource_pools: BTreeSet::new(),
            legacy_wrapper: Some(self.kind),
        })
    }
}
