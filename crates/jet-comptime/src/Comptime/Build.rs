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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildPlan {
    context: u64,
    targets: Vec<BuildTarget>,
    actions: Vec<BuildAction>,
    toolchains: Vec<BuildToolchain>,
    signing_identities: Vec<BuildSigningIdentity>,
    probes: Vec<BuildProbe>,
    default: Option<TargetRef>,
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
}

#[derive(Debug, Clone)]
pub struct BuildContext {
    context: u64,
    targets: Vec<BuildTarget>,
    actions: Vec<BuildAction>,
    toolchains: Vec<BuildToolchain>,
    signing_identities: Vec<BuildSigningIdentity>,
    probes: Vec<BuildProbe>,
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
