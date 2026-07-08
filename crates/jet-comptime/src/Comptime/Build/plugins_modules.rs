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
