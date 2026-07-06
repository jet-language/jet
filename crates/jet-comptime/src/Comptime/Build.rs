//! Build-plan graph foundation for D-BUILDTARGET1 and D-BUILDACTION1.
//!
//! This is the typed Rust substrate the future `BuildContext` comptime method
//! router will call. It intentionally contains no user-facing syntax and no
//! scheduling/cache execution policy.

use crate::SHA256;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_CONTEXT: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TargetId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActionId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ToolchainId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SigningIdentityId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProbeId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PluginId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GeneratedModuleId(pub usize);

macro_rules! target_handle {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name {
            id: TargetId,
            context: u64,
        }

        impl $name {
            pub fn id(self) -> TargetId {
                self.id
            }
        }

        impl From<$name> for TargetRef {
            fn from(value: $name) -> TargetRef {
                TargetRef {
                    id: value.id,
                    context: value.context,
                }
            }
        }
    };
}

target_handle!(ExecutableTarget);
target_handle!(LibraryTarget);
target_handle!(TestTarget);
target_handle!(BenchTarget);
target_handle!(AssetBundleTarget);
target_handle!(DocTarget);
target_handle!(InstallTarget);
target_handle!(PackageTarget);
target_handle!(PublishTarget);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TargetRef {
    id: TargetId,
    context: u64,
}

impl TargetRef {
    pub fn id(self) -> TargetId {
        self.id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActionHandle {
    id: ActionId,
    context: u64,
}

impl ActionHandle {
    pub fn id(self) -> ActionId {
        self.id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ToolchainHandle {
    id: ToolchainId,
    context: u64,
}

impl ToolchainHandle {
    pub fn id(self) -> ToolchainId {
        self.id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SigningIdentityHandle {
    id: SigningIdentityId,
    context: u64,
}

impl SigningIdentityHandle {
    pub fn id(self) -> SigningIdentityId {
        self.id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProbeHandle {
    id: ProbeId,
    context: u64,
}

impl ProbeHandle {
    pub fn id(self) -> ProbeId {
        self.id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PluginHandle {
    id: PluginId,
    context: u64,
}

impl PluginHandle {
    pub fn id(self) -> PluginId {
        self.id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GeneratedModuleHandle {
    id: GeneratedModuleId,
    context: u64,
}

impl GeneratedModuleHandle {
    pub fn id(self) -> GeneratedModuleId {
        self.id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TargetKind {
    Executable,
    Library,
    Test,
    Bench,
    AssetBundle,
    Doc,
    Install,
    Package,
    Publish,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BuildPath(String);

impl BuildPath {
    pub fn new(path: impl Into<String>) -> Result<Self, BuildError> {
        let path = path.into();
        if path.trim().is_empty() {
            return Err(BuildError::EmptyPath);
        }
        Ok(BuildPath(path))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetSpec {
    pub sources: Vec<BuildPath>,
    pub inputs: Vec<BuildPath>,
    pub outputs: Vec<BuildPath>,
    pub deps: Vec<TargetRef>,
    pub actions: Vec<ActionHandle>,
    pub probes: Vec<ProbeHandle>,
    pub toolchain: Option<ToolchainHandle>,
    pub signing_identity: Option<SigningIdentityHandle>,
    pub metadata: BTreeMap<String, String>,
}

impl TargetSpec {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_source(mut self, path: impl Into<String>) -> Self {
        self.sources.push(BuildPath(path.into()));
        self
    }

    pub fn with_input(mut self, path: impl Into<String>) -> Self {
        self.inputs.push(BuildPath(path.into()));
        self
    }

    pub fn with_output(mut self, path: impl Into<String>) -> Self {
        self.outputs.push(BuildPath(path.into()));
        self
    }

    pub fn with_dep(mut self, target: impl Into<TargetRef>) -> Self {
        self.deps.push(target.into());
        self
    }

    pub fn with_action(mut self, action: ActionHandle) -> Self {
        self.actions.push(action);
        self
    }

    pub fn with_probe(mut self, probe: ProbeHandle) -> Self {
        self.probes.push(probe);
        self
    }

    pub fn with_toolchain(mut self, toolchain: ToolchainHandle) -> Self {
        self.toolchain = Some(toolchain);
        self
    }

    pub fn with_signing_identity(mut self, identity: SigningIdentityHandle) -> Self {
        self.signing_identity = Some(identity);
        self
    }

    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

impl Default for TargetSpec {
    fn default() -> Self {
        TargetSpec {
            sources: Vec::new(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            deps: Vec::new(),
            actions: Vec::new(),
            probes: Vec::new(),
            toolchain: None,
            signing_identity: None,
            metadata: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildTarget {
    pub id: TargetId,
    pub name: String,
    pub kind: TargetKind,
    pub sources: Vec<BuildPath>,
    pub inputs: Vec<BuildPath>,
    pub outputs: Vec<BuildPath>,
    pub deps: Vec<TargetRef>,
    pub actions: Vec<ActionHandle>,
    pub probes: Vec<ProbeHandle>,
    pub toolchain: ToolchainHandle,
    pub signing_identity: Option<SigningIdentityHandle>,
    pub metadata: BTreeMap<String, String>,
    pub plugin: Option<PluginHandle>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BuildCapability {
    Fs,
    Exec,
    Net,
    Env,
    Toolchain,
    Cache,
    Custom(String),
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionSpec {
    pub inputs: Vec<BuildPath>,
    pub outputs: Vec<BuildPath>,
    pub argv: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub caps: BTreeSet<BuildCapability>,
    pub cache: ActionCache,
    pub toolchain: Option<ToolchainHandle>,
    pub probes: Vec<ProbeHandle>,
    pub signing_identity: Option<SigningIdentityHandle>,
    pub labels: BTreeMap<String, String>,
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
            caps: BTreeSet::new(),
            cache: ActionCache::Cached,
            toolchain: None,
            probes: Vec::new(),
            signing_identity: None,
            labels: BTreeMap::new(),
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
    pub caps: BTreeSet<BuildCapability>,
    pub cache: ActionCache,
    pub toolchain: ToolchainHandle,
    pub probes: Vec<ProbeHandle>,
    pub signing_identity: Option<SigningIdentityHandle>,
    pub labels: BTreeMap<String, String>,
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

    fn denied(
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
            caps: self.caps,
            cache: ActionCache::Cached,
            toolchain: None,
            probes: Vec::new(),
            signing_identity: None,
            labels,
            resource_pools: BTreeSet::new(),
            legacy_wrapper: Some(self.kind),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActionKey(String);

impl ActionKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContentDigest(String);

impl ContentDigest {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        ContentDigest(format!("sha256:{}", SHA256::sha256_hex(bytes)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionOutcome {
    Succeeded { exit_code: i32 },
    Failed { exit_code: i32 },
    RestoredFromCache,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheHitReason {
    LocalActionRecordMatched,
    DeclaredOutputsRestored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheMissReason {
    NoLocalActionRecord,
    ActionKeyChanged,
    DeclaredOutputMissing,
    RemoteDenied,
    UncachedAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionCacheStatus {
    Hit(CacheHitReason),
    Miss(CacheMissReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionCacheProvenance {
    pub status: ActionCacheStatus,
    pub remote_policy: RemoteCachePolicy,
}

impl ActionCacheProvenance {
    pub fn hit(reason: CacheHitReason) -> Self {
        ActionCacheProvenance {
            status: ActionCacheStatus::Hit(reason),
            remote_policy: RemoteCachePolicy::disabled_until_grant_and_sandbox_proof(),
        }
    }

    pub fn miss(reason: CacheMissReason) -> Self {
        ActionCacheProvenance {
            status: ActionCacheStatus::Miss(reason),
            remote_policy: RemoteCachePolicy::disabled_until_grant_and_sandbox_proof(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteActionRequest {
    CacheRead,
    CacheWrite,
    Execute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteDeniedReason {
    MissingGrantAndSandboxProof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteCacheDenied {
    pub request: RemoteActionRequest,
    pub reason: RemoteDeniedReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteCachePolicy {
    DisabledUntilGrantAndSandboxProof,
}

impl RemoteCachePolicy {
    pub fn disabled_until_grant_and_sandbox_proof() -> Self {
        RemoteCachePolicy::DisabledUntilGrantAndSandboxProof
    }

    pub fn check(self, request: RemoteActionRequest) -> Result<(), RemoteCacheDenied> {
        match self {
            RemoteCachePolicy::DisabledUntilGrantAndSandboxProof => Err(RemoteCacheDenied {
                request,
                reason: RemoteDeniedReason::MissingGrantAndSandboxProof,
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionOutputRecord {
    pub path: BuildPath,
    pub digest: ContentDigest,
    pub byte_len: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionInputSnapshot {
    pub path: BuildPath,
    pub digest: ContentDigest,
    pub byte_len: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionResultRecord {
    pub key: ActionKey,
    pub outcome: ActionOutcome,
    pub outputs: Vec<ActionOutputRecord>,
    pub provenance: ActionCacheProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalCas {
    root: PathBuf,
}

impl LocalCas {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        LocalCas { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn put_blob(&self, bytes: &[u8]) -> io::Result<ContentDigest> {
        let digest = ContentDigest::from_bytes(bytes);
        let path = self.blob_path(&digest);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        if path.exists() {
            let existing = fs::read(&path)?;
            if ContentDigest::from_bytes(&existing) != digest {
                fs::write(path, bytes)?;
            }
        } else {
            fs::write(path, bytes)?;
        }
        Ok(digest)
    }

    pub fn read_blob(&self, digest: &ContentDigest) -> io::Result<Vec<u8>> {
        let bytes = fs::read(self.blob_path(digest))?;
        let actual = ContentDigest::from_bytes(&bytes);
        if &actual != digest {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("CAS blob digest mismatch: expected {}", digest.as_str()),
            ));
        }
        Ok(bytes)
    }

    pub fn snapshot_declared_inputs(
        &self,
        base: &Path,
        action: &BuildAction,
    ) -> io::Result<Vec<ActionInputSnapshot>> {
        let mut inputs = Vec::new();
        for input in &action.inputs {
            let path = resolve_under(base, input.as_str())?;
            let bytes = fs::read(path)?;
            let digest = self.put_blob(&bytes)?;
            inputs.push(ActionInputSnapshot {
                path: input.clone(),
                digest,
                byte_len: bytes.len() as u64,
            });
        }
        Ok(inputs)
    }

    pub fn capture_declared_outputs(
        &self,
        base: &Path,
        action: &BuildAction,
        key: ActionKey,
        outcome: ActionOutcome,
        provenance: ActionCacheProvenance,
    ) -> io::Result<ActionResultRecord> {
        let mut outputs = Vec::new();
        for output in &action.outputs {
            let path = resolve_under(base, output.as_str())?;
            let bytes = fs::read(path)?;
            let digest = self.put_blob(&bytes)?;
            outputs.push(ActionOutputRecord {
                path: output.clone(),
                digest,
                byte_len: bytes.len() as u64,
            });
        }
        Ok(ActionResultRecord {
            key,
            outcome,
            outputs,
            provenance,
        })
    }

    pub fn restore_declared_outputs(
        &self,
        base: &Path,
        record: &ActionResultRecord,
    ) -> io::Result<()> {
        for output in &record.outputs {
            let path = resolve_under(base, output.path.as_str())?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let bytes = self.read_blob(&output.digest)?;
            let tmp = path.with_extension(format!("jet-cache-restore-{}.tmp", std::process::id()));
            fs::write(&tmp, bytes)?;
            match fs::symlink_metadata(&path) {
                Ok(meta) if meta.file_type().is_dir() => {
                    fs::remove_dir_all(&path)?;
                }
                Ok(_) => {
                    fs::remove_file(&path)?;
                }
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(err),
            }
            fs::rename(tmp, path)?;
        }
        Ok(())
    }

    fn blob_path(&self, digest: &ContentDigest) -> PathBuf {
        let hex = digest.0.strip_prefix("sha256:").unwrap_or(digest.as_str());
        let (prefix, rest) = hex.split_at(2);
        self.root
            .join("blobs")
            .join("sha256")
            .join(prefix)
            .join(rest)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockRecord {
    pub key: String,
    pub digest: String,
}

impl LockRecord {
    pub fn new(key: impl Into<String>, digest: impl Into<String>) -> Self {
        LockRecord {
            key: key.into(),
            digest: digest.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProvenanceSource {
    InferredHost,
    JetpackDependency(String),
    AmbientRecord(String),
    UserDeclared(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildProvenance {
    pub source: ProvenanceSource,
    pub lock: Option<LockRecord>,
}

impl BuildProvenance {
    pub fn inferred_host() -> Self {
        BuildProvenance {
            source: ProvenanceSource::InferredHost,
            lock: None,
        }
    }

    pub fn jetpack_dependency(dep: impl Into<String>, lock: LockRecord) -> Self {
        BuildProvenance {
            source: ProvenanceSource::JetpackDependency(dep.into()),
            lock: Some(lock),
        }
    }

    pub fn ambient_record(record: impl Into<String>) -> Self {
        BuildProvenance {
            source: ProvenanceSource::AmbientRecord(record.into()),
            lock: None,
        }
    }

    pub fn user_declared(source: impl Into<String>, lock: Option<LockRecord>) -> Self {
        BuildProvenance {
            source: ProvenanceSource::UserDeclared(source.into()),
            lock,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkIdentity {
    pub name: String,
    pub version: String,
    pub provenance: BuildProvenance,
}

impl SdkIdentity {
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
        provenance: BuildProvenance,
    ) -> Self {
        SdkIdentity {
            name: name.into(),
            version: version.into(),
            provenance,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkerIdentity {
    pub name: String,
    pub provenance: BuildProvenance,
}

impl LinkerIdentity {
    pub fn new(name: impl Into<String>, provenance: BuildProvenance) -> Self {
        LinkerIdentity {
            name: name.into(),
            provenance,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigningIdentitySpec {
    pub label: String,
    pub provenance: BuildProvenance,
}

impl SigningIdentitySpec {
    pub fn new(label: impl Into<String>, provenance: BuildProvenance) -> Self {
        SigningIdentitySpec {
            label: label.into(),
            provenance,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildSigningIdentity {
    pub id: SigningIdentityId,
    pub name: String,
    pub label: String,
    pub provenance: BuildProvenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolchainRole {
    Host,
    Target,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolchainSpec {
    pub role: ToolchainRole,
    pub host_triple: String,
    pub target_triple: String,
    pub sdk: Option<SdkIdentity>,
    pub linker: Option<LinkerIdentity>,
    pub provenance: BuildProvenance,
}

impl ToolchainSpec {
    pub fn target(target_triple: impl Into<String>, provenance: BuildProvenance) -> Self {
        ToolchainSpec {
            role: ToolchainRole::Target,
            host_triple: "host".to_string(),
            target_triple: target_triple.into(),
            sdk: None,
            linker: None,
            provenance,
        }
    }

    pub fn host(host_triple: impl Into<String>, provenance: BuildProvenance) -> Self {
        let host_triple = host_triple.into();
        ToolchainSpec {
            role: ToolchainRole::Host,
            target_triple: host_triple.clone(),
            host_triple,
            sdk: None,
            linker: None,
            provenance,
        }
    }

    pub fn with_host_triple(mut self, host_triple: impl Into<String>) -> Self {
        self.host_triple = host_triple.into();
        self
    }

    pub fn with_sdk(mut self, sdk: SdkIdentity) -> Self {
        self.sdk = Some(sdk);
        self
    }

    pub fn with_linker(mut self, linker: LinkerIdentity) -> Self {
        self.linker = Some(linker);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildToolchain {
    pub id: ToolchainId,
    pub name: String,
    pub role: ToolchainRole,
    pub host_triple: String,
    pub target_triple: String,
    pub sdk: Option<SdkIdentity>,
    pub linker: Option<LinkerIdentity>,
    pub provenance: BuildProvenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReproducibilityClass {
    Reproducible,
    Ambient,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeKind {
    FindProgram {
        program: String,
    },
    PkgConfig {
        package: String,
        min_version: Option<String>,
    },
    HeaderCheck {
        header: String,
    },
    CompileCheck {
        name: String,
        includes: Vec<String>,
        code: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeSpec {
    pub kind: ProbeKind,
    pub reproducibility: ReproducibilityClass,
    pub provenance: BuildProvenance,
    pub toolchain: Option<ToolchainHandle>,
}

impl ProbeSpec {
    pub fn find_program(program: impl Into<String>) -> Self {
        ProbeSpec {
            kind: ProbeKind::FindProgram {
                program: program.into(),
            },
            reproducibility: ReproducibilityClass::Ambient,
            provenance: BuildProvenance::ambient_record("find_program"),
            toolchain: None,
        }
    }

    pub fn pkg_config(package: impl Into<String>) -> Self {
        ProbeSpec {
            kind: ProbeKind::PkgConfig {
                package: package.into(),
                min_version: None,
            },
            reproducibility: ReproducibilityClass::Ambient,
            provenance: BuildProvenance::ambient_record("pkg_config"),
            toolchain: None,
        }
    }

    pub fn header_check(header: impl Into<String>) -> Self {
        ProbeSpec {
            kind: ProbeKind::HeaderCheck {
                header: header.into(),
            },
            reproducibility: ReproducibilityClass::Ambient,
            provenance: BuildProvenance::ambient_record("header_check"),
            toolchain: None,
        }
    }

    pub fn compile_check(
        name: impl Into<String>,
        includes: impl IntoIterator<Item = impl Into<String>>,
        code: impl Into<String>,
    ) -> Self {
        ProbeSpec {
            kind: ProbeKind::CompileCheck {
                name: name.into(),
                includes: includes.into_iter().map(Into::into).collect(),
                code: code.into(),
            },
            reproducibility: ReproducibilityClass::Ambient,
            provenance: BuildProvenance::ambient_record("compile_check"),
            toolchain: None,
        }
    }

    pub fn with_min_version(mut self, version: impl Into<String>) -> Self {
        if let ProbeKind::PkgConfig { min_version, .. } = &mut self.kind {
            *min_version = Some(version.into());
        }
        self
    }

    pub fn with_toolchain(mut self, toolchain: ToolchainHandle) -> Self {
        self.toolchain = Some(toolchain);
        self
    }

    pub fn with_reproducibility(mut self, reproducibility: ReproducibilityClass) -> Self {
        self.reproducibility = reproducibility;
        self
    }

    pub fn with_provenance(mut self, provenance: BuildProvenance) -> Self {
        self.provenance = provenance;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildProbe {
    pub id: ProbeId,
    pub name: String,
    pub kind: ProbeKind,
    pub reproducibility: ReproducibilityClass,
    pub provenance: BuildProvenance,
    pub toolchain: ToolchainHandle,
}

pub const BUILD_PLUGIN_API_VERSION: &str = "jet.build.plugin.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasmComponentPluginSpec {
    pub name: String,
    pub version: String,
    pub api_version: String,
    pub component_digest: String,
    pub requested_caps: BTreeSet<BuildCapability>,
}

impl WasmComponentPluginSpec {
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
        component_digest: impl Into<String>,
    ) -> Self {
        WasmComponentPluginSpec {
            name: name.into(),
            version: version.into(),
            api_version: BUILD_PLUGIN_API_VERSION.to_string(),
            component_digest: component_digest.into(),
            requested_caps: BTreeSet::new(),
        }
    }

    pub fn with_api_version(mut self, api_version: impl Into<String>) -> Self {
        self.api_version = api_version.into();
        self
    }

    pub fn with_capability(mut self, cap: BuildCapability) -> Self {
        self.requested_caps.insert(cap);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildPlugin {
    pub id: PluginId,
    pub name: String,
    pub version: String,
    pub api_version: String,
    pub component_digest: String,
    pub grants: BTreeSet<BuildCapability>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedModuleSpec {
    pub name: String,
    pub path: BuildPath,
    pub source: String,
}

impl GeneratedModuleSpec {
    pub fn new(
        name: impl Into<String>,
        path: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        GeneratedModuleSpec {
            name: name.into(),
            path: BuildPath(path.into()),
            source: source.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildGeneratedModule {
    pub id: GeneratedModuleId,
    pub name: String,
    pub path: BuildPath,
    pub source_digest: ContentDigest,
    pub source: String,
    pub plugin: Option<PluginHandle>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginTargetSpec {
    pub kind: TargetKind,
    pub name: String,
    pub spec: TargetSpec,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginContribution {
    pub actions: Vec<(String, ActionSpec)>,
    pub targets: Vec<PluginTargetSpec>,
    pub generated_modules: Vec<GeneratedModuleSpec>,
}

impl PluginContribution {
    pub fn new() -> Self {
        PluginContribution {
            actions: Vec::new(),
            targets: Vec::new(),
            generated_modules: Vec::new(),
        }
    }

    pub fn with_action(mut self, name: impl Into<String>, spec: ActionSpec) -> Self {
        self.actions.push((name.into(), spec));
        self
    }

    pub fn with_target(
        mut self,
        kind: TargetKind,
        name: impl Into<String>,
        spec: TargetSpec,
    ) -> Self {
        self.targets.push(PluginTargetSpec {
            kind,
            name: name.into(),
            spec,
        });
        self
    }

    pub fn with_generated_module(mut self, module: GeneratedModuleSpec) -> Self {
        self.generated_modules.push(module);
        self
    }
}

impl Default for PluginContribution {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginApplication {
    pub plugin: PluginHandle,
    pub actions: Vec<ActionHandle>,
    pub targets: Vec<TargetRef>,
    pub generated_modules: Vec<GeneratedModuleHandle>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildPlan {
    context: u64,
    targets: Vec<BuildTarget>,
    actions: Vec<BuildAction>,
    toolchains: Vec<BuildToolchain>,
    signing_identities: Vec<BuildSigningIdentity>,
    probes: Vec<BuildProbe>,
    plugins: Vec<BuildPlugin>,
    generated_modules: Vec<BuildGeneratedModule>,
    default: Option<TargetRef>,
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

#[derive(Debug, Clone)]
pub struct BuildContext {
    context: u64,
    targets: Vec<BuildTarget>,
    actions: Vec<BuildAction>,
    toolchains: Vec<BuildToolchain>,
    signing_identities: Vec<BuildSigningIdentity>,
    probes: Vec<BuildProbe>,
    plugins: Vec<BuildPlugin>,
    generated_modules: Vec<BuildGeneratedModule>,
    target_names: HashSet<String>,
    action_names: HashSet<String>,
    toolchain_names: HashSet<String>,
    signing_identity_names: HashSet<String>,
    probe_names: HashSet<String>,
    default_toolchain: ToolchainHandle,
}

impl BuildContext {
    pub fn new() -> Self {
        let context = NEXT_CONTEXT.fetch_add(1, Ordering::Relaxed);
        let default_toolchain = ToolchainHandle {
            id: ToolchainId(0),
            context,
        };
        let mut toolchain_names = HashSet::new();
        toolchain_names.insert("host".to_string());
        BuildContext {
            context,
            targets: Vec::new(),
            actions: Vec::new(),
            toolchains: vec![BuildToolchain {
                id: ToolchainId(0),
                name: "host".to_string(),
                role: ToolchainRole::Host,
                host_triple: "host".to_string(),
                target_triple: "host".to_string(),
                sdk: None,
                linker: None,
                provenance: BuildProvenance::inferred_host(),
            }],
            signing_identities: Vec::new(),
            probes: Vec::new(),
            plugins: Vec::new(),
            generated_modules: Vec::new(),
            target_names: HashSet::new(),
            action_names: HashSet::new(),
            toolchain_names,
            signing_identity_names: HashSet::new(),
            probe_names: HashSet::new(),
            default_toolchain,
        }
    }

    pub fn default_host_toolchain(&self) -> ToolchainHandle {
        self.default_toolchain
    }

    pub fn toolchain(
        &mut self,
        name: impl Into<String>,
        spec: ToolchainSpec,
    ) -> Result<ToolchainHandle, BuildError> {
        let name = check_name(name.into(), NameKind::Toolchain)?;
        if self.toolchain_names.contains(&name) {
            return Err(BuildError::DuplicateToolchainName(name));
        }
        validate_toolchain(&name, &spec)?;
        self.toolchain_names.insert(name.clone());
        let id = ToolchainId(self.toolchains.len());
        self.toolchains.push(BuildToolchain {
            id,
            name,
            role: spec.role,
            host_triple: spec.host_triple,
            target_triple: spec.target_triple,
            sdk: spec.sdk,
            linker: spec.linker,
            provenance: spec.provenance,
        });
        Ok(ToolchainHandle {
            id,
            context: self.context,
        })
    }

    pub fn signing_identity(
        &mut self,
        name: impl Into<String>,
        spec: SigningIdentitySpec,
    ) -> Result<SigningIdentityHandle, BuildError> {
        let name = check_name(name.into(), NameKind::SigningIdentity)?;
        if self.signing_identity_names.contains(&name) {
            return Err(BuildError::DuplicateSigningIdentityName(name));
        }
        validate_identity(&name, &spec.label, &spec.provenance)?;
        self.signing_identity_names.insert(name.clone());
        let id = SigningIdentityId(self.signing_identities.len());
        self.signing_identities.push(BuildSigningIdentity {
            id,
            name,
            label: spec.label,
            provenance: spec.provenance,
        });
        Ok(SigningIdentityHandle {
            id,
            context: self.context,
        })
    }

    pub fn probe(
        &mut self,
        name: impl Into<String>,
        spec: ProbeSpec,
    ) -> Result<ProbeHandle, BuildError> {
        let name = check_name(name.into(), NameKind::Probe)?;
        if self.probe_names.contains(&name) {
            return Err(BuildError::DuplicateProbeName(name));
        }
        let toolchain = spec.toolchain.unwrap_or(self.default_toolchain);
        self.check_toolchain_ref(toolchain)?;
        validate_probe(&name, &spec)?;
        self.probe_names.insert(name.clone());
        let id = ProbeId(self.probes.len());
        self.probes.push(BuildProbe {
            id,
            name,
            kind: spec.kind,
            reproducibility: spec.reproducibility,
            provenance: spec.provenance,
            toolchain,
        });
        Ok(ProbeHandle {
            id,
            context: self.context,
        })
    }

    pub fn find_program(
        &mut self,
        name: impl Into<String>,
        program: impl Into<String>,
    ) -> Result<ProbeHandle, BuildError> {
        self.probe(name, ProbeSpec::find_program(program))
    }

    pub fn pkg_config(
        &mut self,
        name: impl Into<String>,
        package: impl Into<String>,
    ) -> Result<ProbeHandle, BuildError> {
        self.probe(name, ProbeSpec::pkg_config(package))
    }

    pub fn header_check(
        &mut self,
        name: impl Into<String>,
        header: impl Into<String>,
    ) -> Result<ProbeHandle, BuildError> {
        self.probe(name, ProbeSpec::header_check(header))
    }

    pub fn compile_check(
        &mut self,
        name: impl Into<String>,
        check_name: impl Into<String>,
        includes: impl IntoIterator<Item = impl Into<String>>,
        code: impl Into<String>,
    ) -> Result<ProbeHandle, BuildError> {
        self.probe(name, ProbeSpec::compile_check(check_name, includes, code))
    }

    pub fn add_executable(
        &mut self,
        name: impl Into<String>,
        spec: TargetSpec,
    ) -> Result<ExecutableTarget, BuildError> {
        let id = self.push_target(name, TargetKind::Executable, spec)?;
        Ok(ExecutableTarget {
            id,
            context: self.context,
        })
    }

    pub fn add_library(
        &mut self,
        name: impl Into<String>,
        spec: TargetSpec,
    ) -> Result<LibraryTarget, BuildError> {
        let id = self.push_target(name, TargetKind::Library, spec)?;
        Ok(LibraryTarget {
            id,
            context: self.context,
        })
    }

    pub fn add_test(
        &mut self,
        name: impl Into<String>,
        spec: TargetSpec,
    ) -> Result<TestTarget, BuildError> {
        let id = self.push_target(name, TargetKind::Test, spec)?;
        Ok(TestTarget {
            id,
            context: self.context,
        })
    }

    pub fn add_bench(
        &mut self,
        name: impl Into<String>,
        spec: TargetSpec,
    ) -> Result<BenchTarget, BuildError> {
        let id = self.push_target(name, TargetKind::Bench, spec)?;
        Ok(BenchTarget {
            id,
            context: self.context,
        })
    }

    pub fn add_asset_bundle(
        &mut self,
        name: impl Into<String>,
        spec: TargetSpec,
    ) -> Result<AssetBundleTarget, BuildError> {
        let id = self.push_target(name, TargetKind::AssetBundle, spec)?;
        Ok(AssetBundleTarget {
            id,
            context: self.context,
        })
    }

    pub fn add_doc(
        &mut self,
        name: impl Into<String>,
        spec: TargetSpec,
    ) -> Result<DocTarget, BuildError> {
        let id = self.push_target(name, TargetKind::Doc, spec)?;
        Ok(DocTarget {
            id,
            context: self.context,
        })
    }

    pub fn add_install(
        &mut self,
        name: impl Into<String>,
        spec: TargetSpec,
    ) -> Result<InstallTarget, BuildError> {
        let id = self.push_target(name, TargetKind::Install, spec)?;
        Ok(InstallTarget {
            id,
            context: self.context,
        })
    }

    pub fn add_package(
        &mut self,
        name: impl Into<String>,
        spec: TargetSpec,
    ) -> Result<PackageTarget, BuildError> {
        let id = self.push_target(name, TargetKind::Package, spec)?;
        Ok(PackageTarget {
            id,
            context: self.context,
        })
    }

    pub fn add_publish(
        &mut self,
        name: impl Into<String>,
        spec: TargetSpec,
    ) -> Result<PublishTarget, BuildError> {
        let id = self.push_target(name, TargetKind::Publish, spec)?;
        Ok(PublishTarget {
            id,
            context: self.context,
        })
    }

    pub fn action(
        &mut self,
        name: impl Into<String>,
        spec: ActionSpec,
    ) -> Result<ActionHandle, BuildError> {
        self.push_action(name, spec, None)
    }

    pub fn apply_wasm_component_plugin(
        &mut self,
        spec: WasmComponentPluginSpec,
        contribution: PluginContribution,
        policy: &BuildPolicy,
    ) -> Result<PluginApplication, BuildError> {
        validate_plugin_spec(&spec)?;
        if spec.api_version != BUILD_PLUGIN_API_VERSION {
            return Err(BuildError::PluginVersionMismatch {
                plugin: spec.name,
                expected: BUILD_PLUGIN_API_VERSION.to_string(),
                actual: spec.api_version,
            });
        }
        if let PolicySetting::Deny(reason) = &policy.wasm_plugins {
            let caps = spec.requested_caps.iter().cloned().collect();
            return Err(BuildError::PolicyDenied(PolicyExplanation::denied(
                format!("wasm build plugin {}", spec.name),
                reason,
                caps,
            )));
        }
        let grants = policy
            .plugin_grants
            .get(&spec.name)
            .cloned()
            .unwrap_or_default();
        for cap in &spec.requested_caps {
            if !grants.contains(cap) {
                return Err(BuildError::PolicyDenied(PolicyExplanation::denied(
                    format!("wasm build plugin {}", spec.name),
                    format!("missing capability grant {}", cap_name(cap)),
                    spec.requested_caps.iter().cloned().collect(),
                )));
            }
        }
        for (_, action) in &contribution.actions {
            for cap in &action.caps {
                if !grants.contains(cap) {
                    return Err(BuildError::PolicyDenied(PolicyExplanation::denied(
                        format!("wasm build plugin {}", spec.name),
                        format!(
                            "contributed action uses ungranted capability {}",
                            cap_name(cap)
                        ),
                        action.caps.iter().cloned().collect(),
                    )));
                }
            }
        }

        let plugin_id = PluginId(self.plugins.len());
        let plugin = PluginHandle {
            id: plugin_id,
            context: self.context,
        };
        self.plugins.push(BuildPlugin {
            id: plugin_id,
            name: spec.name,
            version: spec.version,
            api_version: spec.api_version,
            component_digest: spec.component_digest,
            grants,
        });

        let mut action_handles = Vec::new();
        for (name, action) in contribution.actions {
            action_handles.push(self.push_action(name, action, Some(plugin))?);
        }

        let mut target_handles = Vec::new();
        for target in contribution.targets {
            let id =
                self.push_target_with_plugin(target.name, target.kind, target.spec, Some(plugin))?;
            target_handles.push(TargetRef {
                id,
                context: self.context,
            });
        }

        let mut module_handles = Vec::new();
        for module in contribution.generated_modules {
            validate_generated_module(&module)?;
            let id = GeneratedModuleId(self.generated_modules.len());
            self.generated_modules.push(BuildGeneratedModule {
                id,
                name: module.name,
                path: module.path,
                source_digest: ContentDigest::from_bytes(module.source.as_bytes()),
                source: module.source,
                plugin: Some(plugin),
            });
            module_handles.push(GeneratedModuleHandle {
                id,
                context: self.context,
            });
        }

        Ok(PluginApplication {
            plugin,
            actions: action_handles,
            targets: target_handles,
            generated_modules: module_handles,
        })
    }

    fn push_action(
        &mut self,
        name: impl Into<String>,
        spec: ActionSpec,
        plugin: Option<PluginHandle>,
    ) -> Result<ActionHandle, BuildError> {
        let name = check_name(name.into(), NameKind::Action)?;
        if !self.action_names.insert(name.clone()) {
            return Err(BuildError::DuplicateActionName(name));
        }
        self.validate_action_spec(&name, &spec)?;
        let toolchain = spec.toolchain.unwrap_or(self.default_toolchain);
        let id = ActionId(self.actions.len());
        self.actions.push(BuildAction {
            id,
            name,
            inputs: spec.inputs,
            outputs: spec.outputs,
            argv: spec.argv,
            env: spec.env,
            caps: spec.caps,
            cache: spec.cache,
            toolchain,
            probes: spec.probes,
            signing_identity: spec.signing_identity,
            labels: spec.labels,
            resource_pools: spec.resource_pools,
            legacy_wrapper: spec.legacy_wrapper,
            plugin,
        });
        Ok(ActionHandle {
            id,
            context: self.context,
        })
    }

    pub fn plan(&self) -> Result<BuildPlan, BuildError> {
        self.snapshot(None)
    }

    pub fn plan_with_default(
        &self,
        default: impl Into<TargetRef>,
    ) -> Result<BuildPlan, BuildError> {
        let default = default.into();
        self.check_target_ref(default)?;
        self.snapshot(Some(default))
    }

    fn push_target(
        &mut self,
        name: impl Into<String>,
        kind: TargetKind,
        spec: TargetSpec,
    ) -> Result<TargetId, BuildError> {
        self.push_target_with_plugin(name, kind, spec, None)
    }

    fn push_target_with_plugin(
        &mut self,
        name: impl Into<String>,
        kind: TargetKind,
        spec: TargetSpec,
        plugin: Option<PluginHandle>,
    ) -> Result<TargetId, BuildError> {
        let name = check_name(name.into(), NameKind::Target)?;
        if !self.target_names.insert(name.clone()) {
            return Err(BuildError::DuplicateTargetName(name));
        }
        self.validate_target_spec(&spec)?;
        let toolchain = spec.toolchain.unwrap_or(self.default_toolchain);
        let id = TargetId(self.targets.len());
        self.targets.push(BuildTarget {
            id,
            name,
            kind,
            sources: spec.sources,
            inputs: spec.inputs,
            outputs: spec.outputs,
            deps: spec.deps,
            actions: spec.actions,
            probes: spec.probes,
            toolchain,
            signing_identity: spec.signing_identity,
            metadata: spec.metadata,
            plugin,
        });
        Ok(id)
    }

    fn snapshot(&self, default: Option<TargetRef>) -> Result<BuildPlan, BuildError> {
        for target in &self.targets {
            self.validate_refs(&target.deps, &target.actions)?;
        }
        validate_action_output_owners(&self.actions)?;
        Ok(BuildPlan {
            context: self.context,
            targets: self.targets.clone(),
            actions: self.actions.clone(),
            toolchains: self.toolchains.clone(),
            signing_identities: self.signing_identities.clone(),
            probes: self.probes.clone(),
            plugins: self.plugins.clone(),
            generated_modules: self.generated_modules.clone(),
            default,
        })
    }

    fn validate_target_spec(&self, spec: &TargetSpec) -> Result<(), BuildError> {
        validate_paths(&spec.sources)?;
        validate_paths(&spec.inputs)?;
        validate_paths(&spec.outputs)?;
        self.validate_refs(&spec.deps, &spec.actions)?;
        for probe in &spec.probes {
            self.check_probe_ref(*probe)?;
        }
        if let Some(toolchain) = spec.toolchain {
            self.check_toolchain_ref(toolchain)?;
        }
        if let Some(identity) = spec.signing_identity {
            self.check_signing_identity_ref(identity)?;
        }
        Ok(())
    }

    fn validate_action_spec(&self, name: &str, spec: &ActionSpec) -> Result<(), BuildError> {
        validate_action(name, spec)?;
        if let Some(toolchain) = spec.toolchain {
            self.check_toolchain_ref(toolchain)?;
        }
        for probe in &spec.probes {
            self.check_probe_ref(*probe)?;
        }
        if let Some(identity) = spec.signing_identity {
            self.check_signing_identity_ref(identity)?;
        }
        Ok(())
    }

    fn validate_refs(
        &self,
        deps: &[TargetRef],
        actions: &[ActionHandle],
    ) -> Result<(), BuildError> {
        for dep in deps {
            self.check_target_ref(*dep)?;
        }
        for action in actions {
            self.check_action_ref(*action)?;
        }
        Ok(())
    }

    fn check_target_ref(&self, target: TargetRef) -> Result<(), BuildError> {
        if target.context != self.context || target.id.0 >= self.targets.len() {
            return Err(BuildError::UnknownTarget(target.id));
        }
        Ok(())
    }

    fn check_action_ref(&self, action: ActionHandle) -> Result<(), BuildError> {
        if action.context != self.context || action.id.0 >= self.actions.len() {
            return Err(BuildError::UnknownAction(action.id));
        }
        Ok(())
    }

    fn check_toolchain_ref(&self, toolchain: ToolchainHandle) -> Result<(), BuildError> {
        if toolchain.context != self.context || toolchain.id.0 >= self.toolchains.len() {
            return Err(BuildError::UnknownToolchain(toolchain.id));
        }
        Ok(())
    }

    fn check_signing_identity_ref(
        &self,
        identity: SigningIdentityHandle,
    ) -> Result<(), BuildError> {
        if identity.context != self.context || identity.id.0 >= self.signing_identities.len() {
            return Err(BuildError::UnknownSigningIdentity(identity.id));
        }
        Ok(())
    }

    fn check_probe_ref(&self, probe: ProbeHandle) -> Result<(), BuildError> {
        if probe.context != self.context || probe.id.0 >= self.probes.len() {
            return Err(BuildError::UnknownProbe(probe.id));
        }
        Ok(())
    }
}

impl Default for BuildContext {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NameKind {
    Target,
    Action,
    Toolchain,
    SigningIdentity,
    Probe,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
    EmptyTargetName,
    EmptyActionName,
    EmptyToolchainName,
    EmptySigningIdentityName,
    EmptyProbeName,
    DuplicateTargetName(String),
    DuplicateActionName(String),
    DuplicateToolchainName(String),
    DuplicateSigningIdentityName(String),
    DuplicateProbeName(String),
    EmptyPath,
    EmptyToolchainTriple(String),
    EmptyIdentityField(String),
    EmptyProbeField(String),
    EmptyActionArgv(String),
    EmptyEnvName(String),
    CachedActionWithoutOutputs(String),
    PhonyActionWithoutCaps(String),
    PhonyActionWithOutputs(String),
    DuplicateActionOutput {
        action: String,
        output: String,
    },
    DuplicateBuildOutput {
        output: String,
        first_action: String,
        second_action: String,
    },
    UnknownTarget(TargetId),
    UnknownAction(ActionId),
    UnknownToolchain(ToolchainId),
    UnknownSigningIdentity(SigningIdentityId),
    UnknownProbe(ProbeId),
    LegacyWrapperWithoutInputs(LegacyWrapperKind),
    LegacyWrapperWithoutOutputs(LegacyWrapperKind),
    LegacyWrapperWithoutCaps(LegacyWrapperKind),
    PolicyDenied(PolicyExplanation),
    EmptyPluginField(String),
    PluginVersionMismatch {
        plugin: String,
        expected: String,
        actual: String,
    },
    EmptyGeneratedModuleField(String),
    TargetDependencyCycle,
    ActionDependencyCycle,
}

fn canonical_action_key(
    plan: &BuildPlan,
    action: &BuildAction,
    inputs: &[ActionInputSnapshot],
) -> ActionKey {
    let mut w = KeyWriter::new();
    w.str("jet.action-key.v1");
    w.str("argv");
    w.vec_str(action.argv.iter().map(String::as_str));
    w.str("env");
    w.map_str(&action.env);
    w.str("inputs");
    w.vec_str(action.inputs.iter().map(BuildPath::as_str));
    w.str("input-snapshots");
    let mut snapshots = inputs.iter().collect::<Vec<_>>();
    snapshots.sort_by(|a, b| a.path.cmp(&b.path));
    w.bytes
        .extend_from_slice(&(snapshots.len() as u64).to_be_bytes());
    for snapshot in snapshots {
        w.str(snapshot.path.as_str());
        w.str(snapshot.digest.as_str());
        w.bytes.extend_from_slice(&snapshot.byte_len.to_be_bytes());
    }
    w.str("outputs");
    w.vec_str(action.outputs.iter().map(BuildPath::as_str));
    w.str("caps");
    for cap in &action.caps {
        encode_capability(&mut w, cap);
    }
    w.str("cache");
    encode_action_cache(&mut w, action.cache);
    w.str("toolchain");
    encode_toolchain(&mut w, &plan.toolchains[action.toolchain.id.0]);
    w.str("probes");
    for probe in &action.probes {
        encode_probe(&mut w, plan, &plan.probes[probe.id.0]);
    }
    w.str("signing");
    match action.signing_identity {
        Some(identity) => {
            w.bool(true);
            encode_signing_identity(&mut w, &plan.signing_identities[identity.id.0]);
        }
        None => w.bool(false),
    }
    w.str("labels");
    w.map_str(&action.labels);
    w.str("resource-pools");
    for pool in action_pools(action) {
        encode_resource_pool(&mut w, &pool);
    }
    w.str("legacy");
    match action.legacy_wrapper {
        Some(wrapper) => {
            w.bool(true);
            w.str(wrapper.as_str());
        }
        None => w.bool(false),
    }
    w.str("plugin");
    match action.plugin {
        Some(plugin) => {
            w.bool(true);
            let plugin = &plan.plugins[plugin.id.0];
            w.str(&plugin.name);
            w.str(&plugin.version);
            w.str(&plugin.api_version);
            w.str(&plugin.component_digest);
            for grant in &plugin.grants {
                encode_capability(&mut w, grant);
            }
        }
        None => w.bool(false),
    }
    ActionKey(format!("act-sha256:{}", SHA256::sha256_hex(&w.bytes)))
}

fn encode_action_cache(w: &mut KeyWriter, cache: ActionCache) {
    match cache {
        ActionCache::Cached => w.str("cached"),
        ActionCache::UncachedPhony => w.str("uncached-phony"),
    }
}

fn encode_capability(w: &mut KeyWriter, cap: &BuildCapability) {
    match cap {
        BuildCapability::Fs => w.str("fs"),
        BuildCapability::Exec => w.str("exec"),
        BuildCapability::Net => w.str("net"),
        BuildCapability::Env => w.str("env"),
        BuildCapability::Toolchain => w.str("toolchain"),
        BuildCapability::Cache => w.str("cache"),
        BuildCapability::Custom(value) => {
            w.str("custom");
            w.str(value);
        }
    }
}

fn encode_resource_pool(w: &mut KeyWriter, pool: &BuildResourcePool) {
    match pool {
        BuildResourcePool::Cpu => w.str("cpu"),
        BuildResourcePool::Memory => w.str("memory"),
        BuildResourcePool::Linker => w.str("linker"),
        BuildResourcePool::Console => w.str("console"),
        BuildResourcePool::Gpu => w.str("gpu"),
        BuildResourcePool::Custom(name) => {
            w.str("custom");
            w.str(name);
        }
    }
}

fn encode_toolchain(w: &mut KeyWriter, toolchain: &BuildToolchain) {
    w.str(&toolchain.name);
    match toolchain.role {
        ToolchainRole::Host => w.str("host"),
        ToolchainRole::Target => w.str("target"),
    }
    w.str(&toolchain.host_triple);
    w.str(&toolchain.target_triple);
    match &toolchain.sdk {
        Some(sdk) => {
            w.bool(true);
            w.str(&sdk.name);
            w.str(&sdk.version);
            encode_provenance(w, &sdk.provenance);
        }
        None => w.bool(false),
    }
    match &toolchain.linker {
        Some(linker) => {
            w.bool(true);
            w.str(&linker.name);
            encode_provenance(w, &linker.provenance);
        }
        None => w.bool(false),
    }
    encode_provenance(w, &toolchain.provenance);
}

fn encode_signing_identity(w: &mut KeyWriter, identity: &BuildSigningIdentity) {
    w.str(&identity.name);
    w.str(&identity.label);
    encode_provenance(w, &identity.provenance);
}

fn encode_probe(w: &mut KeyWriter, plan: &BuildPlan, probe: &BuildProbe) {
    w.str(&probe.name);
    match &probe.kind {
        ProbeKind::FindProgram { program } => {
            w.str("find-program");
            w.str(program);
        }
        ProbeKind::PkgConfig {
            package,
            min_version,
        } => {
            w.str("pkg-config");
            w.str(package);
            match min_version {
                Some(version) => {
                    w.bool(true);
                    w.str(version);
                }
                None => w.bool(false),
            }
        }
        ProbeKind::HeaderCheck { header } => {
            w.str("header-check");
            w.str(header);
        }
        ProbeKind::CompileCheck {
            name,
            includes,
            code,
        } => {
            w.str("compile-check");
            w.str(name);
            w.vec_str(includes.iter().map(String::as_str));
            w.str(code);
        }
    }
    match probe.reproducibility {
        ReproducibilityClass::Reproducible => w.str("reproducible"),
        ReproducibilityClass::Ambient => w.str("ambient"),
    }
    encode_provenance(w, &probe.provenance);
    encode_toolchain(w, &plan.toolchains[probe.toolchain.id.0]);
}

fn encode_provenance(w: &mut KeyWriter, provenance: &BuildProvenance) {
    match &provenance.source {
        ProvenanceSource::InferredHost => w.str("inferred-host"),
        ProvenanceSource::JetpackDependency(dep) => {
            w.str("jetpack-dependency");
            w.str(dep);
        }
        ProvenanceSource::AmbientRecord(record) => {
            w.str("ambient-record");
            w.str(record);
        }
        ProvenanceSource::UserDeclared(source) => {
            w.str("user-declared");
            w.str(source);
        }
    }
    match &provenance.lock {
        Some(lock) => {
            w.bool(true);
            w.str(&lock.key);
            w.str(&lock.digest);
        }
        None => w.bool(false),
    }
}

struct KeyWriter {
    bytes: Vec<u8>,
}

impl KeyWriter {
    fn new() -> Self {
        KeyWriter { bytes: Vec::new() }
    }

    fn str(&mut self, value: &str) {
        self.bytes
            .extend_from_slice(&(value.len() as u64).to_be_bytes());
        self.bytes.extend_from_slice(value.as_bytes());
    }

    fn bool(&mut self, value: bool) {
        self.bytes.push(u8::from(value));
    }

    fn vec_str<'a>(&mut self, values: impl IntoIterator<Item = &'a str>) {
        let values: Vec<&str> = values.into_iter().collect();
        self.bytes
            .extend_from_slice(&(values.len() as u64).to_be_bytes());
        for value in values {
            self.str(value);
        }
    }

    fn map_str(&mut self, map: &BTreeMap<String, String>) {
        self.bytes
            .extend_from_slice(&(map.len() as u64).to_be_bytes());
        for (key, value) in map {
            self.str(key);
            self.str(value);
        }
    }
}

fn resolve_under(base: &Path, rel: &str) -> io::Result<PathBuf> {
    let path = Path::new(rel);
    let mut out = PathBuf::from(base);
    for component in path.components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "build output path escapes base directory",
                ));
            }
        }
    }
    Ok(out)
}

fn cap_name(cap: &BuildCapability) -> String {
    match cap {
        BuildCapability::Fs => "Fs".to_string(),
        BuildCapability::Exec => "Exec".to_string(),
        BuildCapability::Net => "Net".to_string(),
        BuildCapability::Env => "Env".to_string(),
        BuildCapability::Toolchain => "Toolchain".to_string(),
        BuildCapability::Cache => "Cache".to_string(),
        BuildCapability::Custom(name) => name.clone(),
    }
}

fn check_name(name: String, kind: NameKind) -> Result<String, BuildError> {
    if name.trim().is_empty() {
        return match kind {
            NameKind::Target => Err(BuildError::EmptyTargetName),
            NameKind::Action => Err(BuildError::EmptyActionName),
            NameKind::Toolchain => Err(BuildError::EmptyToolchainName),
            NameKind::SigningIdentity => Err(BuildError::EmptySigningIdentityName),
            NameKind::Probe => Err(BuildError::EmptyProbeName),
        };
    }
    Ok(name)
}

fn validate_plugin_spec(spec: &WasmComponentPluginSpec) -> Result<(), BuildError> {
    if spec.name.trim().is_empty() {
        return Err(BuildError::EmptyPluginField("name".to_string()));
    }
    if spec.version.trim().is_empty() {
        return Err(BuildError::EmptyPluginField(spec.name.clone()));
    }
    if spec.api_version.trim().is_empty() {
        return Err(BuildError::EmptyPluginField(spec.name.clone()));
    }
    if spec.component_digest.trim().is_empty() {
        return Err(BuildError::EmptyPluginField(spec.name.clone()));
    }
    Ok(())
}

fn validate_generated_module(module: &GeneratedModuleSpec) -> Result<(), BuildError> {
    if module.name.trim().is_empty()
        || module.path.as_str().trim().is_empty()
        || module.source.trim().is_empty()
    {
        return Err(BuildError::EmptyGeneratedModuleField(module.name.clone()));
    }
    Ok(())
}

fn validate_toolchain(name: &str, spec: &ToolchainSpec) -> Result<(), BuildError> {
    if spec.host_triple.trim().is_empty() || spec.target_triple.trim().is_empty() {
        return Err(BuildError::EmptyToolchainTriple(name.to_string()));
    }
    if let Some(sdk) = &spec.sdk {
        validate_identity(name, &sdk.name, &sdk.provenance)?;
        validate_identity(name, &sdk.version, &sdk.provenance)?;
    }
    if let Some(linker) = &spec.linker {
        validate_identity(name, &linker.name, &linker.provenance)?;
    }
    validate_provenance(name, &spec.provenance)
}

fn validate_identity(
    name: &str,
    field: &str,
    provenance: &BuildProvenance,
) -> Result<(), BuildError> {
    if field.trim().is_empty() {
        return Err(BuildError::EmptyIdentityField(name.to_string()));
    }
    validate_provenance(name, provenance)
}

fn validate_probe(name: &str, spec: &ProbeSpec) -> Result<(), BuildError> {
    match &spec.kind {
        ProbeKind::FindProgram { program } if program.trim().is_empty() => {
            return Err(BuildError::EmptyProbeField(name.to_string()));
        }
        ProbeKind::PkgConfig { package, .. } if package.trim().is_empty() => {
            return Err(BuildError::EmptyProbeField(name.to_string()));
        }
        ProbeKind::HeaderCheck { header } if header.trim().is_empty() => {
            return Err(BuildError::EmptyProbeField(name.to_string()));
        }
        ProbeKind::CompileCheck {
            name: check,
            includes,
            code,
        } if check.trim().is_empty()
            || code.trim().is_empty()
            || includes.iter().any(|include| include.trim().is_empty()) =>
        {
            return Err(BuildError::EmptyProbeField(name.to_string()));
        }
        _ => {}
    }
    validate_provenance(name, &spec.provenance)
}

fn validate_provenance(name: &str, provenance: &BuildProvenance) -> Result<(), BuildError> {
    match &provenance.source {
        ProvenanceSource::JetpackDependency(dep)
        | ProvenanceSource::AmbientRecord(dep)
        | ProvenanceSource::UserDeclared(dep)
            if dep.trim().is_empty() =>
        {
            return Err(BuildError::EmptyIdentityField(name.to_string()));
        }
        _ => {}
    }
    if let Some(lock) = &provenance.lock {
        if lock.key.trim().is_empty() || lock.digest.trim().is_empty() {
            return Err(BuildError::EmptyIdentityField(name.to_string()));
        }
    }
    Ok(())
}

fn validate_action(name: &str, spec: &ActionSpec) -> Result<(), BuildError> {
    if spec.argv.is_empty() || spec.argv.iter().any(|arg| arg.trim().is_empty()) {
        return Err(BuildError::EmptyActionArgv(name.to_string()));
    }
    validate_paths(&spec.inputs)?;
    validate_paths(&spec.outputs)?;
    for key in spec.env.keys() {
        if key.trim().is_empty() {
            return Err(BuildError::EmptyEnvName(name.to_string()));
        }
    }
    match spec.cache {
        ActionCache::Cached if spec.outputs.is_empty() => {
            return Err(BuildError::CachedActionWithoutOutputs(name.to_string()));
        }
        ActionCache::UncachedPhony if !spec.outputs.is_empty() => {
            return Err(BuildError::PhonyActionWithOutputs(name.to_string()));
        }
        ActionCache::UncachedPhony if spec.caps.is_empty() => {
            return Err(BuildError::PhonyActionWithoutCaps(name.to_string()));
        }
        _ => {}
    }

    let mut outputs = HashSet::new();
    for output in &spec.outputs {
        if !outputs.insert(output.as_str()) {
            return Err(BuildError::DuplicateActionOutput {
                action: name.to_string(),
                output: output.as_str().to_string(),
            });
        }
    }
    Ok(())
}

fn validate_action_output_owners(actions: &[BuildAction]) -> Result<(), BuildError> {
    let mut owners: BTreeMap<&str, &str> = BTreeMap::new();
    for action in actions {
        for output in &action.outputs {
            if let Some(first_action) = owners.insert(output.as_str(), action.name.as_str()) {
                return Err(BuildError::DuplicateBuildOutput {
                    output: output.as_str().to_string(),
                    first_action: first_action.to_string(),
                    second_action: action.name.clone(),
                });
            }
        }
    }
    Ok(())
}

fn validate_paths(paths: &[BuildPath]) -> Result<(), BuildError> {
    for path in paths {
        if path.as_str().trim().is_empty() {
            return Err(BuildError::EmptyPath);
        }
    }
    Ok(())
}
