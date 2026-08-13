use super::actions_policy::{
    ActionCache, BuildAction, BuildCapability, BuildResourcePool, LegacyWrapperKind,
    PolicyExplanation,
};
use super::cache_cas::{ActionInputSnapshot, ActionKey, ContentDigest};
use super::execution_helpers::action_pools;
use super::execution_runtime::BuildProbeFact;
use super::handles::{ActionId, ProbeId, SigningIdentityId, TargetId, ToolchainId};
use super::plan_graph::BuildPlan;
use super::provenance_toolchains::{
    BuildProbe, BuildProvenance, BuildSigningIdentity, BuildToolchain, ProbeKind,
    ProvenanceSource, ReproducibilityClass, ToolchainRole,
};
use super::targets::BuildPath;
use crate::SHA256;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NameKind {
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
    CompilerPackageDependencyMissing { package: String, dependency: String },
    DuplicateToolchainName(String),
    DuplicateSigningIdentityName(String),
    DuplicateProbeName(String),
    EmptyPath,
    InvalidPath(String),
    EmptyToolchainTriple(String),
    EmptyIdentityField(String),
    MissingLockedProvenance(String),
    EmptyProbeField(String),
    EmptyActionArgv(String),
    EmptyEnvName(String),
    UndeclaredEnvName { action: String, key: String },
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
    LegacyWrapperCommandMismatch {
        wrapper: LegacyWrapperKind,
        actual: String,
    },
    LegacyProjectFileMissing(LegacyWrapperKind),
    LegacyProjectFileInvalid(String),
    PolicyDenied(PolicyExplanation),
    EmptyPluginField(String),
    InvalidPluginDigest(String),
    PackagedPlugin(String),
    PluginVersionMismatch {
        plugin: String,
        expected: String,
        actual: String,
    },
    EmptyGeneratedModuleField(String),
    InvalidGeneratedModulePath(String),
    DuplicateGeneratedModuleName(String),
    DuplicateGeneratedModulePath(String),
    GeneratedModuleCycle { module: String, path: String },
    TargetDependencyCycle,
    ActionDependencyCycle,
}

pub(super) fn canonical_action_key(
    plan: &BuildPlan,
    action: &BuildAction,
    inputs: &[ActionInputSnapshot],
) -> ActionKey {
    let mut w = KeyWriter::new();
    w.str("jet.action-key.v2");
    w.str("kind");
    w.str(action.kind.as_str());
    w.str("observe-exact-source");
    w.bool(action.kind.observes_exact_source());
    w.str("compiler-owned");
    w.bool(action.compiler_owned);
    w.str("argv");
    w.vec_str(action.argv.iter().map(String::as_str));
    w.str("env-allowlist");
    let allowlisted_env = allowlisted_env(action);
    w.map_str(&allowlisted_env);
    w.str("inputs");
    w.vec_str(action.inputs.iter().map(BuildPath::as_str));
    w.str("input-snapshots");
    let mut snapshots = inputs.iter().collect::<Vec<_>>();
    snapshots.sort_by(|a, b| a.path.cmp(&b.path));
    if action.kind.observes_exact_source() {
        // Exact source bytes stay identity inputs: every declared input must
        // contribute a content digest when the action surface can observe source.
        for path in &action.inputs {
            let present = snapshots.iter().any(|s| s.path.as_str() == path.as_str());
            w.str(path.as_str());
            w.bool(present);
        }
    }
    w.bytes
        .extend_from_slice(&(snapshots.len() as u64).to_be_bytes());
    for snapshot in snapshots {
        w.str(snapshot.path.as_str());
        w.str(snapshot.digest.as_str());
        w.bytes.extend_from_slice(&snapshot.byte_len.to_be_bytes());
    }
    w.str("dependency-artifact-snapshots");
    for input in action
        .inputs
        .iter()
        .filter(|path| path.as_str().starts_with(".jet/build-cache/package-artifacts/"))
    {
        w.str(input.as_str());
        if let Some(snapshot) = inputs
            .iter()
            .find(|snapshot| snapshot.path.as_str() == input.as_str())
        {
            w.bool(true);
            w.str(snapshot.digest.as_str());
            w.bytes.extend_from_slice(&snapshot.byte_len.to_be_bytes());
        } else {
            w.bool(false);
        }
    }
    w.str("outputs");
    w.vec_str(action.outputs.iter().map(BuildPath::as_str));
    w.str("dep-outputs");
    let dep_outputs = declared_dep_outputs(plan, action);
    w.vec_str(dep_outputs.iter().map(String::as_str));
    w.str("caps");
    for cap in &action.caps {
        encode_capability(&mut w, cap);
    }
    w.str("cache");
    encode_action_cache(&mut w, action.cache);
    w.str("compiler-version");
    w.str(env!("CARGO_PKG_VERSION"));
    w.str("helper-versions");
    w.map_str(&action.helper_versions);
    w.str("generated-modules");
    let mut generated = plan.generated_modules.iter().collect::<Vec<_>>();
    generated.sort_by(|a, b| a.path.cmp(&b.path));
    for module in generated {
        w.str(module.path.as_str());
        w.str(module.source_digest.as_str());
        // Exact generated source remains an input when any observing action
        // can surface it through diagnostics / docs / publication.
        if action.kind.observes_exact_source() {
            w.str(&module.source);
        }
    }
    w.str("target");
    if let Some(target) = plan
        .action_targets()
        .get(&action.id)
        .and_then(|id| plan.targets.get(id.0))
    {
        w.bool(true);
        w.str(&target.name);
        w.str(&format!("{:?}", target.kind));
        w.map_str(&target.metadata);
        w.vec_str(target.sources.iter().map(BuildPath::as_str));
    } else {
        w.bool(false);
    }
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
    w.str("variant");
    match &action.variant_identity {
        Some(identity) => {
            w.bool(true);
            w.str(identity);
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

fn allowlisted_env(action: &BuildAction) -> BTreeMap<String, String> {
    if action.env_allowlist.is_empty() {
        return action.env.clone();
    }
    action
        .env
        .iter()
        .filter(|(key, _)| action.env_allowlist.contains(*key))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn declared_dep_outputs(plan: &BuildPlan, action: &BuildAction) -> Vec<String> {
    let Some(target_id) = plan.action_targets().get(&action.id).copied() else {
        return Vec::new();
    };
    let Some(target) = plan.targets.get(target_id.0) else {
        return Vec::new();
    };
    let mut outs = BTreeSet::new();
    for dep in &target.deps {
        let Some(dep_target) = plan.targets.get(dep.id.0) else {
            continue;
        };
        for path in dep_target
            .outputs
            .iter()
            .chain(dep_target.inputs.iter())
        {
            outs.insert(path.as_str().to_string());
        }
        for dep_action in &dep_target.actions {
            if let Some(dep_action) = plan.actions.get(dep_action.id.0) {
                for path in &dep_action.outputs {
                    outs.insert(path.as_str().to_string());
                }
            }
        }
    }
    outs.into_iter().collect()
}

pub(super) fn canonical_effective_action_key(
    plan: &BuildPlan,
    action: &BuildAction,
    inputs: &[ActionInputSnapshot],
    grants: &BTreeSet<BuildCapability>,
    _executable: &Path,
    executable_digest: &ContentDigest,
    probe_facts: &[BuildProbeFact],
) -> ActionKey {
    let base = canonical_action_key(plan, action, inputs);
    let mut w = KeyWriter::new();
    w.str("jet.effective-action-key.v1");
    w.str(base.as_str());
    w.str("effective-policy");
    for grant in grants { encode_capability(&mut w, grant); }
    // The resolved filesystem path is host-local and must not split an
    // otherwise identical remote action identity. The executable bytes remain
    // part of the key through their content digest.
    w.str("resolved-executable-digest");
    w.str(executable_digest.as_str());
    w.str("actual-probe-facts");
    for fact in probe_facts {
        w.str(&fact.name);
        w.bool(fact.success);
        w.str(&fact.detail);
        w.str(&format!("{:?}", fact.reproducibility));
        w.str(&format!("{}:{}", fact.toolchain.context, fact.toolchain.id.0));
        w.str(fact.toolchain_provenance.as_str());
    }
    w.str("compiler-identity");
    w.str(concat!(env!("CARGO_PKG_NAME"), "@", env!("CARGO_PKG_VERSION")));
    ActionKey(format!("act-sha256:{}", SHA256::sha256_hex(&w.bytes)))
}

fn encode_action_cache(w: &mut KeyWriter, cache: ActionCache) {
    match cache {
        ActionCache::Cached => w.str("cached"),
        ActionCache::UncachedPhony => w.str("uncached-phony"),
    }
}

fn encode_capability(w: &mut KeyWriter, cap: &BuildCapability) {
    w.str(cap.flag());
}

fn encode_resource_pool(w: &mut KeyWriter, pool: &BuildResourcePool) {
    match pool {
        BuildResourcePool::Cpu => w.str("cpu"),
        BuildResourcePool::Memory => w.str("memory"),
        BuildResourcePool::Linker => w.str("linker"),
        BuildResourcePool::Console => w.str("console"),
        BuildResourcePool::GPU => w.str("gpu"),
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
    match &toolchain.sysroot {
        Some(sysroot) => {
            w.bool(true);
            w.str(&sysroot.name);
            w.str(&sysroot.path_digest);
            encode_provenance(w, &sysroot.provenance);
        }
        None => w.bool(false),
    }
    w.str("tools");
    w.map_str(&toolchain.tools);
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
