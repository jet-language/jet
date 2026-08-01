use super::errors_keys::BuildError;
use super::handles::{
    ActionHandle, PluginHandle, ProbeHandle, SigningIdentityHandle, TargetId, TargetRef,
    ToolchainHandle,
};
use std::collections::BTreeMap;
use std::path::{Component, Path};

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
pub struct BuildPath(pub(super) String);

impl BuildPath {
    pub fn new(path: impl Into<String>) -> Result<Self, BuildError> {
        let path = path.into();
        if path.trim().is_empty() {
            return Err(BuildError::EmptyPath);
        }
        if Path::new(&path).is_absolute()
            || Path::new(&path).components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(BuildError::InvalidPath(path));
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
