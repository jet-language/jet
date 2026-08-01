use super::actions_policy::{ActionSpec, BuildCapability};
use super::cache_cas::ContentDigest;
use super::handles::{
    ActionHandle, GeneratedModuleHandle, GeneratedModuleId, PluginHandle, PluginId, TargetRef,
};
use super::targets::{BuildPath, TargetKind, TargetSpec};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

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

    /// Load the package manifest and component bytes as one verified unit.
    /// The manifest is intentionally a small, dependency-free format.
    pub fn load_packaged(
        manifest_path: impl AsRef<Path>,
        component_path: impl AsRef<Path>,
    ) -> Result<Self, String> {
        let manifest_path = manifest_path.as_ref();
        let component_path = component_path.as_ref();
        reject_plugin_link(manifest_path, "manifest")?;
        reject_plugin_link(component_path, "component")?;
        let manifest = fs::read_to_string(manifest_path)
            .map_err(|error| format!("could not read plugin manifest: {error}"))?;
        let component = fs::read(component_path)
            .map_err(|error| format!("could not read plugin component: {error}"))?;
        let spec = Self::from_manifest_text(&manifest)?;
        let actual = ContentDigest::from_bytes(&component).as_str().to_string();
        if spec.component_digest != actual {
            return Err(format!(
                "plugin component digest mismatch: manifest declares {}, bytes are {actual}",
                spec.component_digest
            ));
        }
        validate_component_binary(&component)?;
        Ok(spec)
    }

    pub fn from_manifest_text(text: &str) -> Result<Self, String> {
        let mut name = None;
        let mut version = None;
        let mut api_version = None;
        let mut component_digest = None;
        let mut requested_caps = BTreeSet::new();
        let mut fields = BTreeSet::new();
        for raw_line in text.lines() {
            let line = manifest_line_without_comment(raw_line).trim();
            if line.is_empty() {
                continue;
            }
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| format!("plugin manifest line {line} needs key = value"))?;
            let key = key.trim();
            let value = value.trim();
            if !fields.insert(key) {
                return Err(format!("plugin manifest field {key} is declared more than once"));
            }
            match key {
                "name" => name = Some(manifest_string(value, key)?),
                "version" => version = Some(manifest_string(value, key)?),
                "api_version" => api_version = Some(manifest_string(value, key)?),
                "component_digest" => component_digest = Some(manifest_string(value, key)?),
                "capabilities" => requested_caps = manifest_capabilities(value)?,
                _ => return Err(format!("unknown plugin manifest field {key}")),
            }
        }
        let spec = WasmComponentPluginSpec {
            name: name.ok_or_else(|| "plugin manifest is missing name".to_string())?,
            version: version.ok_or_else(|| "plugin manifest is missing version".to_string())?,
            api_version: api_version
                .ok_or_else(|| "plugin manifest is missing api_version".to_string())?,
            component_digest: ContentDigest::parse(
                &component_digest
                    .ok_or_else(|| "plugin manifest is missing component_digest".to_string())?,
            )
            .map_err(|error| format!("invalid plugin component digest: {error}"))?
            .as_str()
            .to_string(),
            requested_caps,
        };
        if spec.api_version != BUILD_PLUGIN_API_VERSION {
            return Err(format!(
                "unsupported build plugin API {}; expected {BUILD_PLUGIN_API_VERSION}",
                spec.api_version
            ));
        }
        Ok(spec)
    }
}

fn manifest_line_without_comment(line: &str) -> &str {
    let mut quoted = false;
    let mut escaped = false;
    for (index, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            '#' if !quoted => return &line[..index],
            _ => {}
        }
    }
    line
}

fn reject_plugin_link(path: &Path, label: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("could not stat plugin {label}: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!("plugin {label} must be a regular non-symlink file"));
    }
    Ok(())
}

/// Check the dependency-free part of the WebAssembly Component Model envelope.
/// Full instantiation stays in the runtime host; the compiler seam still must
/// reject arbitrary bytes before it records a package as a component plugin.
fn validate_component_binary(bytes: &[u8]) -> Result<(), String> {
    const MAGIC: &[u8; 4] = b"\0asm";
    const COMPONENT_VERSION: &[u8; 4] = &[0x0d, 0x00, 0x01, 0x00];
    if bytes.len() < 8 || &bytes[..4] != MAGIC || &bytes[4..8] != COMPONENT_VERSION {
        return Err(
            "plugin component is not a valid WebAssembly Component Model binary".to_string(),
        );
    }

    // Component sections use the same id + length framing as core Wasm. Walk
    // every section so a truncated or overflowing payload cannot pass the
    // package boundary. Unknown future section ids remain rejected until the
    // compiler knows how to classify them.
    let mut offset = 8usize;
    while offset < bytes.len() {
        let id = bytes[offset];
        offset += 1;
        if id > 13 {
            return Err(format!("plugin component uses unknown section id {id}"));
        }
        let length = read_u32_leb(bytes, &mut offset)? as usize;
        let end = offset
            .checked_add(length)
            .ok_or_else(|| "plugin component section length overflows".to_string())?;
        if end > bytes.len() {
            return Err("plugin component section is truncated".to_string());
        }
        offset = end;
    }
    Ok(())
}

fn read_u32_leb(bytes: &[u8], offset: &mut usize) -> Result<u32, String> {
    let mut value = 0u32;
    for shift in (0..35).step_by(7) {
        let byte = *bytes
            .get(*offset)
            .ok_or_else(|| "plugin component section length is truncated".to_string())?;
        *offset += 1;
        if shift == 28 && byte & 0xf0 != 0 {
            return Err("plugin component section length overflows u32".to_string());
        }
        value |= u32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err("plugin component section length uses an overlong integer".to_string())
}

fn manifest_string(value: &str, field: &str) -> Result<String, String> {
    let value = value.trim();
    let value = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .ok_or_else(|| format!("plugin manifest field {field} must be a quoted string"))?;
    if value.is_empty() {
        return Err(format!("plugin manifest field {field} cannot be empty"));
    }
    let mut out = String::with_capacity(value.len());
    let mut escaped = false;
    for ch in value.chars() {
        if escaped {
            match ch {
                '"' | '\\' => out.push(ch),
                _ => {
                    return Err(format!(
                        "plugin manifest field {field} contains an unsupported escape"
                    ));
                }
            }
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Err(format!("plugin manifest field {field} contains an unescaped quote"));
        } else {
            out.push(ch);
        }
    }
    if escaped {
        return Err(format!("plugin manifest field {field} ends with an escape"));
    }
    Ok(out)
}

fn manifest_capabilities(value: &str) -> Result<BTreeSet<BuildCapability>, String> {
    let value = value
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .ok_or_else(|| "plugin manifest capabilities must be a list".to_string())?;
    let mut capabilities = BTreeSet::new();
    for item in value.split(',').map(str::trim).filter(|item| !item.is_empty()) {
        let name = manifest_string(item, "capabilities")?;
        let capability = BuildCapability::parse(&name)
            .ok_or_else(|| format!("unknown build capability {name}"))?;
        capabilities.insert(capability);
    }
    Ok(capabilities)
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
