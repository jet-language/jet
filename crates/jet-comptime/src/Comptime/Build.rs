//! Build-plan graph foundation for D-BUILDTARGET1 and D-BUILDACTION1.
//!
//! This is the typed Rust substrate the future `BuildContext` comptime method
//! router will call. It intentionally contains no user-facing syntax and no
//! scheduling/cache execution policy.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_CONTEXT: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TargetId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActionId(pub usize);

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildPlan {
    context: u64,
    targets: Vec<BuildTarget>,
    actions: Vec<BuildAction>,
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
    target_names: HashSet<String>,
    action_names: HashSet<String>,
}

impl BuildContext {
    pub fn new() -> Self {
        BuildContext {
            context: NEXT_CONTEXT.fetch_add(1, Ordering::Relaxed),
            targets: Vec::new(),
            actions: Vec::new(),
            target_names: HashSet::new(),
            action_names: HashSet::new(),
        }
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
        validate_action(&name, &spec)?;
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
            default,
        })
    }

    fn validate_target_spec(&self, spec: &TargetSpec) -> Result<(), BuildError> {
        validate_paths(&spec.sources)?;
        validate_paths(&spec.inputs)?;
        validate_paths(&spec.outputs)?;
        self.validate_refs(&spec.deps, &spec.actions)
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
    EmptyTargetName,
    EmptyActionName,
    DuplicateTargetName(String),
    DuplicateActionName(String),
    EmptyPath,
    EmptyActionArgv(String),
    EmptyEnvName(String),
    CachedActionWithoutOutputs(String),
    PhonyActionWithoutCaps(String),
    PhonyActionWithOutputs(String),
    DuplicateActionOutput { action: String, output: String },
    DuplicateBuildOutput {
        output: String,
        first_action: String,
        second_action: String,
    },
    UnknownTarget(TargetId),
    UnknownAction(ActionId),
}

fn check_name(name: String, kind: NameKind) -> Result<String, BuildError> {
    if name.trim().is_empty() {
        return match kind {
            NameKind::Target => Err(BuildError::EmptyTargetName),
            NameKind::Action => Err(BuildError::EmptyActionName),
        };
    }
    Ok(name)
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
