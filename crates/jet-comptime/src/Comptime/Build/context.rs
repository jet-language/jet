use super::NEXT_CONTEXT;
use super::actions_policy::{ActionSpec, BuildAction, BuildPolicy, PolicyExplanation, PolicySetting};
use super::cache_cas::ContentDigest;
use super::errors_keys::{BuildError, NameKind};
use super::handles::{
    ActionHandle, ActionId, AssetBundleTarget, BenchTarget, DocTarget, ExecutableTarget,
    GeneratedModuleHandle, GeneratedModuleId, InstallTarget, LibraryTarget, PackageTarget,
    PluginHandle, PluginId, ProbeHandle, ProbeId, PublishTarget, SigningIdentityHandle,
    SigningIdentityId, TargetId, TargetRef, TestTarget, ToolchainHandle, ToolchainId,
};
use super::plan_graph::BuildPlan;
use super::plugins_modules::{
    BUILD_PLUGIN_API_VERSION, BuildGeneratedModule, BuildPlugin, GeneratedModuleSpec,
    PluginApplication, PluginContribution, WasmComponentPluginSpec,
};
use super::provenance_toolchains::{
    BuildProbe, BuildProvenance, BuildSigningIdentity, BuildToolchain, ProbeSpec,
    SigningIdentitySpec, ToolchainRole, ToolchainSpec,
};
use super::targets::{BuildTarget, TargetKind, TargetSpec};
use super::validation::{
    cap_name, check_name, validate_action, validate_action_output_owners,
    validate_generated_module, validate_identity, validate_paths, validate_plugin_spec,
    validate_probe, validate_toolchain,
};
use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::Ordering;

#[derive(Debug, Clone)]
pub struct BuildContext {
    pub(super) context: u64,
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
    /// Policy captured at build-session creation. Every typed bridge method
    /// observes this same policy; no method silently widens it.
    policy: BuildPolicy,
    /// D-BUILDCTX-FLAGS1=A: project default profile name (`release`, `debug`, …).
    default_profile: Option<String>,
    /// D-BUILDCTX-FLAGS1=A: `--allow-*` grants applied when CLI omits them.
    default_allows: HashSet<String>,
}

impl BuildContext {
    pub fn new() -> Self {
        Self::new_with_policy(BuildPolicy::allow_all())
    }

    pub fn new_with_policy(policy: BuildPolicy) -> Self {
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
                sysroot: None,
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
            policy,
            default_profile: None,
            default_allows: HashSet::new(),
        }
    }

    pub fn policy(&self) -> &BuildPolicy {
        &self.policy
    }

    /// D-BUILDCTX-FLAGS1=A: set the project default profile (CLI `--profile`/`--release` wins).
    pub fn default_profile(&mut self, profile: impl Into<String>) {
        self.default_profile = Some(profile.into());
    }

    /// D-BUILDCTX-FLAGS1=A: declare default `--allow-*` grants when CLI omits them.
    pub fn default_allow(&mut self, effects: impl IntoIterator<Item = impl Into<String>>) {
        for effect in effects {
            self.default_allows.insert(effect.into());
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
            sysroot: spec.sysroot,
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

    /// D-BUILDGEN1: register one additive generated Jet module. Materializing
    /// and re-checking it belongs to the driver; the graph stores source and
    /// digest so cache/query/provenance use the same canonical value.
    pub fn generate(
        &mut self,
        name: impl Into<String>,
        path: impl Into<String>,
        source: impl Into<String>,
    ) -> Result<GeneratedModuleHandle, BuildError> {
        let module = GeneratedModuleSpec::new(name, path, source);
        validate_generated_module(&module)?;
        self.push_generated_module(module, None)
    }

    fn push_generated_module(
        &mut self,
        module: GeneratedModuleSpec,
        plugin: Option<PluginHandle>,
    ) -> Result<GeneratedModuleHandle, BuildError> {
        if self
            .actions
            .iter()
            .any(|action| action.outputs.iter().any(|output| output == &module.path))
        {
            return Err(BuildError::GeneratedModuleCycle {
                module: module.name.clone(),
                path: module.path.as_str().to_string(),
            });
        }
        if self.generated_modules.iter().any(|old| old.name == module.name) {
            return Err(BuildError::DuplicateGeneratedModuleName(module.name));
        }
        if self.generated_modules.iter().any(|old| old.path == module.path) {
            return Err(BuildError::DuplicateGeneratedModulePath(
                module.path.as_str().to_string(),
            ));
        }
        let id = GeneratedModuleId(self.generated_modules.len());
        self.generated_modules.push(BuildGeneratedModule {
            id,
            name: module.name,
            path: module.path,
            source_digest: ContentDigest::from_bytes(module.source.as_bytes()),
            source: module.source,
            plugin,
        });
        Ok(GeneratedModuleHandle {
            id,
            context: self.context,
        })
    }

    pub fn apply_wasm_component_plugin(
        &mut self,
        spec: WasmComponentPluginSpec,
        contribution: PluginContribution,
        policy: &BuildPolicy,
    ) -> Result<PluginApplication, BuildError> {
        let snapshot = self.clone();
        let result = self.apply_wasm_component_plugin_inner(spec, contribution, policy);
        if result.is_err() {
            *self = snapshot;
        }
        result
    }

    pub fn apply_packaged_wasm_component_plugin(
        &mut self,
        manifest_path: impl AsRef<Path>,
        component_path: impl AsRef<Path>,
        contribution: PluginContribution,
        policy: &BuildPolicy,
    ) -> Result<PluginApplication, BuildError> {
        let spec = WasmComponentPluginSpec::load_packaged(manifest_path, component_path)
            .map_err(BuildError::PackagedPlugin)?;
        self.apply_wasm_component_plugin(spec, contribution, policy)
    }

    fn apply_wasm_component_plugin_inner(
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
            let handle = self.push_generated_module(module, Some(plugin))?;
            let id = handle.id();
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
        if let Some(module) = self.generated_modules.iter().find(|module| {
            spec.outputs
                .iter()
                .any(|output| output == &module.path)
        }) {
            return Err(BuildError::GeneratedModuleCycle {
                module: module.name.clone(),
                path: module.path.as_str().to_string(),
            });
        }
        let toolchain = spec.toolchain.unwrap_or(self.default_toolchain);
        let id = ActionId(self.actions.len());
        self.actions.push(BuildAction {
            id,
            name,
            inputs: spec.inputs,
            outputs: spec.outputs,
            argv: spec.argv,
            env: spec.env,
            env_allowlist: spec.env_allowlist,
            caps: spec.caps,
            cache: spec.cache,
            kind: spec.kind,
            toolchain,
            probes: spec.probes,
            signing_identity: spec.signing_identity,
            labels: spec.labels,
            helper_versions: spec.helper_versions,
            resource_pools: spec.resource_pools,
            legacy_wrapper: spec.legacy_wrapper,
            plugin,
            variant_identity: spec.variant_identity,
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
        for module in &self.generated_modules {
            if self.actions.iter().any(|action| {
                action
                    .outputs
                    .iter()
                    .any(|output| output == &module.path)
            }) {
                return Err(BuildError::GeneratedModuleCycle {
                    module: module.name.clone(),
                    path: module.path.as_str().to_string(),
                });
            }
        }
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
            default_profile: self.default_profile.clone(),
            default_allows: {
                let mut allows: Vec<String> = self.default_allows.iter().cloned().collect();
                allows.sort();
                allows
            },
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
