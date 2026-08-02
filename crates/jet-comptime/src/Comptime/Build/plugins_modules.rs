use super::actions_policy::{ActionSpec, BuildCapability};
use super::cache_cas::ContentDigest;
use super::handles::{
    ActionHandle, GeneratedModuleHandle, GeneratedModuleId, PluginHandle, PluginId, TargetRef,
};
use super::targets::{BuildPath, TargetKind, TargetSpec};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::Path;

pub const BUILD_PLUGIN_API_VERSION: &str = "jet.build.plugin.v1";
/// Hidden sibling-process entry point. Wasmtime stays in jetpack-bin; the
/// compiler seam only carries this bounded, typed wire contract.
pub const BUILD_PLUGIN_HOST_SUBCOMMAND: &str = "__build-plugin-v1";
pub const BUILD_PLUGIN_MAX_REQUEST_BYTES: usize = 64 * 1024;
pub const BUILD_PLUGIN_MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
pub const BUILD_PLUGIN_MAX_MANIFEST_BYTES: usize = 64 * 1024;
pub const BUILD_PLUGIN_MAX_COMPONENT_BYTES: usize = 64 * 1024 * 1024;
const BUILD_PLUGIN_MAX_WIRE_ITEMS: usize = 100_000;

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
        Self::load_packaged_with_manifest_digest(manifest_path, component_path)
            .map(|(spec, _)| spec)
    }

    /// Load both package files once and return the manifest identity that the
    /// sibling host must recheck before it instantiates the component.
    pub fn load_packaged_with_manifest_digest(
        manifest_path: impl AsRef<Path>,
        component_path: impl AsRef<Path>,
    ) -> Result<(Self, String), String> {
        let manifest_path = manifest_path.as_ref();
        let component_path = component_path.as_ref();
        reject_plugin_link(manifest_path, "manifest")?;
        reject_plugin_link(component_path, "component")?;
        let manifest = read_packaged_file_bounded(
            manifest_path,
            "manifest",
            BUILD_PLUGIN_MAX_MANIFEST_BYTES,
        )?;
        let component = read_packaged_file_bounded(
            component_path,
            "component",
            BUILD_PLUGIN_MAX_COMPONENT_BYTES,
        )?;
        let spec = Self::load_packaged_bytes(&manifest, &component)?;
        let manifest_digest = ContentDigest::from_bytes(&manifest).as_str().to_string();
        Ok((spec, manifest_digest))
    }

    /// Validate a manifest/component byte pair without reading either path.
    /// The host uses this after reading the files so the bytes it verifies are
    /// the exact bytes it gives to Wasmtime.
    pub fn load_packaged_bytes(manifest: &[u8], component: &[u8]) -> Result<Self, String> {
        if manifest.len() > BUILD_PLUGIN_MAX_MANIFEST_BYTES {
            return Err(format!(
                "plugin manifest exceeds {} bytes",
                BUILD_PLUGIN_MAX_MANIFEST_BYTES
            ));
        }
        if component.len() > BUILD_PLUGIN_MAX_COMPONENT_BYTES {
            return Err(format!(
                "plugin component exceeds {} bytes",
                BUILD_PLUGIN_MAX_COMPONENT_BYTES
            ));
        }
        let manifest = std::str::from_utf8(manifest)
            .map_err(|_| "plugin manifest is not UTF-8".to_string())?;
        let spec = Self::from_manifest_text(manifest)?;
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
        if text.len() > BUILD_PLUGIN_MAX_MANIFEST_BYTES {
            return Err(format!(
                "plugin manifest exceeds {} bytes",
                BUILD_PLUGIN_MAX_MANIFEST_BYTES
            ));
        }
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

/// Read a packaged plugin file only after checking its regular-file identity
/// and declared byte bound. Both the compiler loader and the sibling host use
/// this helper so a large or replaced package cannot become an unbounded read.
pub fn read_packaged_file_bounded(
    path: &Path,
    label: &str,
    limit: usize,
) -> Result<Vec<u8>, String> {
    reject_plugin_link(path, label)?;
    let metadata = fs::metadata(path)
        .map_err(|error| format!("could not stat plugin {label}: {error}"))?;
    if metadata.len() > limit as u64 {
        return Err(format!(
            "plugin {label} exceeds {limit} bytes"
        ));
    }
    let file = fs::File::open(path)
        .map_err(|error| format!("could not read plugin {label}: {error}"))?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(limit.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("could not read plugin {label}: {error}"))?;
    if bytes.len() > limit {
        return Err(format!("plugin {label} exceeds {limit} bytes"));
    }
    Ok(bytes)
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

/// Host-facing contribution produced by a packaged component. References use
/// names because a guest cannot manufacture BuildContext handles. The context
/// resolves them against the current graph only after the complete response
/// has been decoded.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PackagedPluginContribution {
    pub actions: Vec<PackagedPluginAction>,
    pub targets: Vec<PackagedPluginTarget>,
    pub generated_modules: Vec<GeneratedModuleSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackagedPluginAction {
    pub name: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub argv: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub env_allowlist: BTreeSet<String>,
    pub caps: BTreeSet<String>,
    pub cache: String,
    pub kind: String,
    pub toolchain: Option<String>,
    pub probes: Vec<String>,
    pub signing_identity: Option<String>,
    pub labels: BTreeMap<String, String>,
    pub helper_versions: BTreeMap<String, String>,
    pub resource_pools: BTreeSet<String>,
    pub legacy_wrapper: Option<String>,
    pub variant_identity: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackagedPluginTarget {
    pub kind: String,
    pub name: String,
    pub sources: Vec<String>,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub deps: Vec<String>,
    pub actions: Vec<String>,
    pub probes: Vec<String>,
    pub toolchain: Option<String>,
    pub signing_identity: Option<String>,
    pub metadata: BTreeMap<String, String>,
}

pub type PackagedPluginRunner = fn(
    &Path,
    &Path,
    &WasmComponentPluginSpec,
    &str,
) -> Result<PackagedPluginContribution, String>;

thread_local! {
    static PACKAGED_PLUGIN_RUNNER: std::cell::Cell<Option<PackagedPluginRunner>> =
        const { std::cell::Cell::new(None) };
}

/// Install the production sibling-host runner for the duration of one build
/// evaluation. The previous runner is restored even when the closure returns
/// an error, so a failed or nested build cannot leak authority across sessions.
pub fn with_packaged_plugin_runner<R>(runner: PackagedPluginRunner, body: impl FnOnce() -> R) -> R {
    let previous = PACKAGED_PLUGIN_RUNNER.with(|slot| slot.replace(Some(runner)));
    struct RestoreRunner(Option<PackagedPluginRunner>);
    impl Drop for RestoreRunner {
        fn drop(&mut self) {
            PACKAGED_PLUGIN_RUNNER.with(|slot| slot.set(self.0.take()));
        }
    }
    let _restore = RestoreRunner(previous);
    body()
}

pub fn run_packaged_plugin(
    manifest_path: &Path,
    component_path: &Path,
    spec: &WasmComponentPluginSpec,
    manifest_digest: &str,
) -> Result<PackagedPluginContribution, String> {
    PACKAGED_PLUGIN_RUNNER.with(|slot| {
        slot.get()
            .ok_or_else(|| "packaged build-plugin host is not installed".to_string())?
            (manifest_path, component_path, spec, manifest_digest)
    })
}

/// Request bytes are deterministic and contain no host handles or mutable
/// compiler state. Guests may inspect package identity/capabilities, but the
/// host still validates every graph field on the return path.
pub fn encode_build_plugin_request(spec: &WasmComponentPluginSpec) -> Vec<u8> {
    format!(
        "version=1\nname={}\nplugin_version={}\napi_version={}\ncomponent_digest={}\ncapabilities={}\n",
        wire_scalar(&spec.name),
        wire_scalar(&spec.version),
        wire_scalar(&spec.api_version),
        wire_scalar(&spec.component_digest),
        wire_list(spec.requested_caps.iter().map(|cap| cap.flag())),
    )
    .into_bytes()
}

/// Decode the bounded, versioned response from the packaged component. A
/// response is either a complete graph contribution or a typed failure; no
/// partially decoded object is returned to the context.
pub fn decode_build_plugin_response(
    bytes: &[u8],
) -> Result<PackagedPluginContribution, String> {
    if bytes.len() > BUILD_PLUGIN_MAX_RESPONSE_BYTES {
        return Err(format!(
            "build plugin response exceeds {} bytes",
            BUILD_PLUGIN_MAX_RESPONSE_BYTES
        ));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| "build plugin response is not UTF-8".to_string())?;
    let mut version = None;
    let mut status = None;
    let mut error = None;
    let mut action_count = None;
    let mut target_count = None;
    let mut generated_count = None;
    let mut actions = Vec::new();
    let mut targets = Vec::new();
    let mut generated_modules = Vec::new();
    let mut top_level_fields = BTreeSet::new();
    for line in text.lines() {
        let top_level = [
            "version=",
            "status=",
            "error=",
            "actions=",
            "targets=",
            "generated=",
        ]
        .iter()
        .find_map(|prefix| line.strip_prefix(prefix).map(|_| *prefix));
        if let Some(field) = top_level {
            if !top_level_fields.insert(field) {
                return Err(format!(
                    "build plugin response field {} is declared more than once",
                    field.trim_end_matches('=')
                ));
            }
        }
        if let Some(value) = line.strip_prefix("version=") {
            version = Some(parse_wire_count(value, "version")?);
        } else if let Some(value) = line.strip_prefix("status=") {
            status = Some(value);
        } else if let Some(value) = line.strip_prefix("error=") {
            error = Some(wire_unscalar(value, "plugin error")?);
        } else if let Some(value) = line.strip_prefix("actions=") {
            action_count = Some(parse_wire_count(value, "actions")?);
        } else if let Some(value) = line.strip_prefix("targets=") {
            target_count = Some(parse_wire_count(value, "targets")?);
        } else if let Some(value) = line.strip_prefix("generated=") {
            generated_count = Some(parse_wire_count(value, "generated")?);
        } else if let Some(value) = line.strip_prefix("action\t") {
            actions.push(decode_packaged_action(value)?);
        } else if let Some(value) = line.strip_prefix("target\t") {
            targets.push(decode_packaged_target(value)?);
        } else if let Some(value) = line.strip_prefix("generated\t") {
            let fields = value.split('\t').collect::<Vec<_>>();
            if fields.len() != 3 {
                return Err("generated plugin entry needs name, path, and source".to_string());
            }
            generated_modules.push(GeneratedModuleSpec::new(
                wire_unscalar(fields[0], "generated name")?,
                wire_unscalar(fields[1], "generated path")?,
                wire_unscalar(fields[2], "generated source")?,
            ));
        } else if !line.trim().is_empty() {
            return Err(format!("unknown build plugin response field: {line}"));
        }
    }
    if version != Some(1) {
        return Err("unsupported build plugin response version".to_string());
    }
    if status != Some("ok") {
        return Err(error.unwrap_or_else(|| "build plugin returned a failure".to_string()));
    }
    if error.is_some() {
        return Err("build plugin success response contains an error field".to_string());
    }
    for (name, count) in [
        ("actions", action_count),
        ("targets", target_count),
        ("generated", generated_count),
    ] {
        if count.is_some_and(|count| count > BUILD_PLUGIN_MAX_WIRE_ITEMS) {
            return Err(format!(
                "build plugin {name} count exceeds {BUILD_PLUGIN_MAX_WIRE_ITEMS}"
            ));
        }
    }
    if action_count != Some(actions.len())
        || target_count != Some(targets.len())
        || generated_count != Some(generated_modules.len())
    {
        return Err("build plugin response count does not match its entries".to_string());
    }
    Ok(PackagedPluginContribution {
        actions,
        targets,
        generated_modules,
    })
}

fn decode_packaged_action(value: &str) -> Result<PackagedPluginAction, String> {
    let fields = value.split('\t').collect::<Vec<_>>();
    if fields.len() != 17 {
        return Err("plugin action needs 17 fields".to_string());
    }
    Ok(PackagedPluginAction {
        name: wire_unscalar(fields[0], "action name")?,
        inputs: wire_unlist(fields[1], "action inputs")?,
        outputs: wire_unlist(fields[2], "action outputs")?,
        argv: wire_unlist(fields[3], "action argv")?,
        env: wire_unmap(fields[4], "action env")?,
        env_allowlist: wire_unlist(fields[5], "action env allowlist")?.into_iter().collect(),
        caps: wire_unlist(fields[6], "action capabilities")?.into_iter().collect(),
        cache: wire_unscalar(fields[7], "action cache")?,
        kind: wire_unscalar(fields[8], "action kind")?,
        toolchain: wire_optional(fields[9], "action toolchain")?,
        probes: wire_unlist(fields[10], "action probes")?,
        signing_identity: wire_optional(fields[11], "action signing identity")?,
        labels: wire_unmap(fields[12], "action labels")?,
        helper_versions: wire_unmap(fields[13], "action helper versions")?,
        resource_pools: wire_unlist(fields[14], "action resource pools")?.into_iter().collect(),
        legacy_wrapper: wire_optional(fields[15], "action legacy wrapper")?,
        variant_identity: wire_optional(fields[16], "action variant identity")?,
    })
}

fn decode_packaged_target(value: &str) -> Result<PackagedPluginTarget, String> {
    let fields = value.split('\t').collect::<Vec<_>>();
    if fields.len() != 11 {
        return Err("plugin target needs 11 fields".to_string());
    }
    Ok(PackagedPluginTarget {
        kind: wire_unscalar(fields[0], "target kind")?,
        name: wire_unscalar(fields[1], "target name")?,
        sources: wire_unlist(fields[2], "target sources")?,
        inputs: wire_unlist(fields[3], "target inputs")?,
        outputs: wire_unlist(fields[4], "target outputs")?,
        deps: wire_unlist(fields[5], "target dependencies")?,
        actions: wire_unlist(fields[6], "target actions")?,
        probes: wire_unlist(fields[7], "target probes")?,
        toolchain: wire_optional(fields[8], "target toolchain")?,
        signing_identity: wire_optional(fields[9], "target signing identity")?,
        metadata: wire_unmap(fields[10], "target metadata")?,
    })
}

fn wire_scalar(value: &str) -> String {
    hex_encode(value.as_bytes())
}

fn wire_unscalar(value: &str, field: &str) -> Result<String, String> {
    String::from_utf8(hex_decode(value)?).map_err(|_| format!("{field} is not UTF-8"))
}

fn wire_optional(value: &str, field: &str) -> Result<Option<String>, String> {
    if value.is_empty() {
        Ok(None)
    } else {
        wire_unscalar(value, field).map(Some)
    }
}

fn wire_list<'a>(values: impl IntoIterator<Item = &'a str>) -> String {
    let values = values.into_iter().collect::<Vec<_>>();
    let mut out = values.len().to_string();
    out.push(':');
    for value in values {
        let bytes = value.as_bytes();
        out.push_str(&bytes.len().to_string());
        out.push(':');
        out.push_str(&hex_encode(bytes));
    }
    out
}

fn wire_unlist(value: &str, field: &str) -> Result<Vec<String>, String> {
    let (count, mut rest) = value
        .split_once(':')
        .ok_or_else(|| format!("{field} list has no count"))?;
    let count = count
        .parse::<usize>()
        .map_err(|_| format!("{field} list count is invalid"))?;
    if count > BUILD_PLUGIN_MAX_WIRE_ITEMS {
        return Err(format!(
            "{field} list contains too many items (maximum {BUILD_PLUGIN_MAX_WIRE_ITEMS})"
        ));
    }
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let (len, next) = rest
            .split_once(':')
            .ok_or_else(|| format!("{field} list item has no length"))?;
        let len = len
            .parse::<usize>()
            .map_err(|_| format!("{field} list item length is invalid"))?;
        let encoded_len = len
            .checked_mul(2)
            .ok_or_else(|| format!("{field} list item length overflows"))?;
        if next.len() < encoded_len {
            return Err(format!("{field} list item is truncated"));
        }
        let (encoded, tail) = next.split_at(encoded_len);
        values.push(String::from_utf8(hex_decode(encoded)?).map_err(|_| {
            format!("{field} list item is not UTF-8")
        })?);
        rest = tail;
    }
    if !rest.is_empty() {
        return Err(format!("{field} list has trailing bytes"));
    }
    Ok(values)
}

fn wire_map(map: &BTreeMap<String, String>) -> String {
    let mut values = Vec::with_capacity(map.len() * 2);
    for (key, value) in map {
        values.push(key.as_str());
        values.push(value.as_str());
    }
    wire_list(values)
}

fn wire_unmap(value: &str, field: &str) -> Result<BTreeMap<String, String>, String> {
    let values = wire_unlist(value, field)?;
    if values.len() % 2 != 0 {
        return Err(format!("{field} map has an unpaired value"));
    }
    let mut map = BTreeMap::new();
    for pair in values.chunks_exact(2) {
        if map.insert(pair[0].clone(), pair[1].clone()).is_some() {
            return Err(format!("{field} map contains a duplicate key"));
        }
    }
    Ok(map)
}

fn parse_wire_count(value: &str, field: &str) -> Result<usize, String> {
    value
        .parse::<usize>()
        .map_err(|_| format!("{field} is not a number"))
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn hex_decode(value: &str) -> Result<Vec<u8>, String> {
    if value.len() % 2 != 0 {
        return Err("plugin wire hex has odd length".to_string());
    }
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let high = (pair[0] as char)
            .to_digit(16)
            .ok_or_else(|| "plugin wire hex is invalid".to_string())?;
        let low = (pair[1] as char)
            .to_digit(16)
            .ok_or_else(|| "plugin wire hex is invalid".to_string())?;
        out.push(((high << 4) | low) as u8);
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginApplication {
    pub plugin: PluginHandle,
    pub actions: Vec<ActionHandle>,
    pub targets: Vec<TargetRef>,
    pub generated_modules: Vec<GeneratedModuleHandle>,
}
