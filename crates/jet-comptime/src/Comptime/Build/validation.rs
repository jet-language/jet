use super::actions_policy::{ActionCache, ActionSpec, BuildAction, BuildCapability};
use super::cache_cas::ContentDigest;
use super::errors_keys::{BuildError, NameKind};
use super::plugins_modules::{GeneratedModuleSpec, WasmComponentPluginSpec};
use super::provenance_toolchains::{
    BuildProvenance, ProbeKind, ProbeSpec, ProvenanceSource, ToolchainSpec,
};
use super::targets::BuildPath;
use std::collections::{BTreeMap, HashSet};
use std::io;
use std::path::{Component, Path, PathBuf};

pub(super) fn resolve_under(base: &Path, rel: &str) -> io::Result<PathBuf> {
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

pub(super) fn cap_name(cap: &BuildCapability) -> String {
    cap.name().to_string()
}

pub(super) fn check_name(name: String, kind: NameKind) -> Result<String, BuildError> {
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

pub(super) fn validate_plugin_spec(spec: &WasmComponentPluginSpec) -> Result<(), BuildError> {
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
    ContentDigest::parse(&spec.component_digest)
        .map_err(|error| BuildError::InvalidPluginDigest(error.to_string()))?;
    Ok(())
}

pub(super) fn validate_generated_module(module: &GeneratedModuleSpec) -> Result<(), BuildError> {
    if module.name.trim().is_empty()
        || module.path.as_str().trim().is_empty()
        || module.source.trim().is_empty()
    {
        return Err(BuildError::EmptyGeneratedModuleField(module.name.clone()));
    }
    let path = Path::new(module.path.as_str());
    let components = path.components().collect::<Vec<_>>();
    let valid_root = components.len() >= 3
        && matches!(components[0], Component::Normal(part) if part == ".jet")
        && matches!(components[1], Component::Normal(part) if part == "generated")
        && components[2..]
            .iter()
            .all(|component| matches!(component, Component::Normal(_)));
    if !valid_root || path.extension().and_then(|extension| extension.to_str()) != Some("jet") {
        return Err(BuildError::InvalidGeneratedModulePath(
            module.path.as_str().to_string(),
        ));
    }
    Ok(())
}

pub(super) fn validate_toolchain(name: &str, spec: &ToolchainSpec) -> Result<(), BuildError> {
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

pub(super) fn validate_identity(
    name: &str,
    field: &str,
    provenance: &BuildProvenance,
) -> Result<(), BuildError> {
    if field.trim().is_empty() {
        return Err(BuildError::EmptyIdentityField(name.to_string()));
    }
    validate_provenance(name, provenance)
}

pub(super) fn validate_probe(name: &str, spec: &ProbeSpec) -> Result<(), BuildError> {
    match &spec.kind {
        ProbeKind::FindProgram { program } if program.trim().is_empty() => {
            return Err(BuildError::EmptyProbeField(name.to_string()));
        }
        ProbeKind::PkgConfig { package, .. } if package.trim().is_empty() => {
            return Err(BuildError::EmptyProbeField(name.to_string()));
        }
        ProbeKind::HeaderCheck { header }
            if header.trim().is_empty() || !valid_header_name(header) =>
        {
            return Err(BuildError::EmptyProbeField(name.to_string()));
        }
        ProbeKind::CompileCheck {
            name: check,
            includes,
            code,
        } if check.trim().is_empty()
            || code.trim().is_empty()
            || includes.iter().any(|include| !valid_header_name(include)) =>
        {
            return Err(BuildError::EmptyProbeField(name.to_string()));
        }
        _ => {}
    }
    validate_provenance(name, &spec.provenance)
}

fn valid_header_name(header: &str) -> bool {
    !header.is_empty()
        && !header.chars().any(|character| {
            matches!(character, '\0' | '\n' | '\r' | '"' | '<' | '>')
        })
        && Path::new(header).is_relative()
        && Path::new(header)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

pub(super) fn validate_provenance(name: &str, provenance: &BuildProvenance) -> Result<(), BuildError> {
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

pub(super) fn validate_action(name: &str, spec: &ActionSpec) -> Result<(), BuildError> {
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
    for key in &spec.env_allowlist {
        if key.trim().is_empty() {
            return Err(BuildError::EmptyEnvName(name.to_string()));
        }
        if !spec.env.contains_key(key) {
            return Err(BuildError::UndeclaredEnvName {
                action: name.to_string(),
                key: key.clone(),
            });
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

pub(super) fn validate_action_output_owners(actions: &[BuildAction]) -> Result<(), BuildError> {
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

pub(super) fn validate_paths(paths: &[BuildPath]) -> Result<(), BuildError> {
    for path in paths {
        if path.as_str().trim().is_empty() {
            return Err(BuildError::EmptyPath);
        }
        let path_value = Path::new(path.as_str());
        if path_value.is_absolute()
            || path_value.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(BuildError::InvalidPath(path.as_str().to_string()));
        }
    }
    Ok(())
}
