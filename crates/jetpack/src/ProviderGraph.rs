//! Federated provider facts under Jetpack authority (D-WD6).
//!
//! External provider prefixes and trust-root config remain owner-gated. This
//! module models provider metadata/fetch/lock/sandbox/signature/audit facts.

pub use super::Replacement::ReplacementCandidate as ReplacementOverlay;
use super::JSON::{self, JSONValue};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProviderFamily {
    Core,
    Nix,
    Path,
    Github,
    JetRegistry,
    Npm,
    PyPI,
    Cargo,
    SwiftPM,
    Maven,
    NuGet,
    Conan,
    Vcpkg,
    Homebrew,
    Binary,
}

impl ProviderFamily {
    pub fn label(&self) -> &'static str {
        match self {
            ProviderFamily::Core => "core",
            ProviderFamily::Nix => "nix",
            ProviderFamily::Path => "path",
            ProviderFamily::Github => "github",
            ProviderFamily::JetRegistry => "jet-registry",
            ProviderFamily::Npm => "npm",
            ProviderFamily::PyPI => "pypi",
            ProviderFamily::Cargo => "cargo",
            ProviderFamily::SwiftPM => "swiftpm",
            ProviderFamily::Maven => "maven",
            ProviderFamily::NuGet => "nuget",
            ProviderFamily::Conan => "conan",
            ProviderFamily::Vcpkg => "vcpkg",
            ProviderFamily::Homebrew => "homebrew",
            ProviderFamily::Binary => "binary",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderContract {
    pub family: ProviderFamily,
    pub parses_refs: bool,
    pub probes_metadata: bool,
    pub resolves_channels: bool,
    pub fetches_bytes: bool,
    pub verifies_hash_signature: bool,
    pub exposes_audit_facts: bool,
    pub reports_offline_satisfiability: bool,
}

impl ProviderContract {
    /// Describe the surface that this build actually owns. Metadata
    /// normalization does not imply that Jet can fetch or lock bytes from a
    /// foreign ecosystem, so those capabilities stay false until a real
    /// provider adapter supplies them.
    pub fn for_family(family: ProviderFamily) -> ProviderContract {
        let local_or_first_party = matches!(
            family,
            ProviderFamily::Core
                | ProviderFamily::Nix
                | ProviderFamily::Path
                | ProviderFamily::JetRegistry
        );
        let binary = matches!(family, ProviderFamily::Binary);
        ProviderContract {
            family,
            parses_refs: true,
            probes_metadata: true,
            resolves_channels: local_or_first_party,
            fetches_bytes: local_or_first_party || binary,
            verifies_hash_signature: local_or_first_party || binary,
            exposes_audit_facts: true,
            reports_offline_satisfiability: local_or_first_party || binary,
        }
    }

    /// Kept as a descriptive constructor for callers that already have a
    /// family. It now returns the honest family contract rather than claiming
    /// unimplemented foreign network support.
    pub fn full(family: ProviderFamily) -> ProviderContract {
        Self::for_family(family)
    }
}

pub fn built_in_contracts() -> Vec<ProviderContract> {
    vec![
        ProviderContract::full(ProviderFamily::Core),
        ProviderContract::full(ProviderFamily::Nix),
        ProviderContract::full(ProviderFamily::Path),
        ProviderContract::full(ProviderFamily::Github),
        ProviderContract::full(ProviderFamily::JetRegistry),
        ProviderContract::full(ProviderFamily::Npm),
        ProviderContract::full(ProviderFamily::PyPI),
        ProviderContract::full(ProviderFamily::Cargo),
        ProviderContract::full(ProviderFamily::SwiftPM),
        ProviderContract::full(ProviderFamily::Maven),
        ProviderContract::full(ProviderFamily::NuGet),
        ProviderContract::full(ProviderFamily::Conan),
        ProviderContract::full(ProviderFamily::Vcpkg),
        ProviderContract::full(ProviderFamily::Homebrew),
        ProviderContract::full(ProviderFamily::Binary),
    ]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderFactReport {
    pub facts: MetadataFacts,
    pub losses: Vec<String>,
    pub conflicts: Vec<String>,
}

impl ProviderFactReport {
    pub fn is_lossless(&self) -> bool {
        self.losses.is_empty() && self.conflicts.is_empty()
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.conflicts.is_empty() {
            return Err(format!("provider facts conflict: {}", self.conflicts.join("; ")));
        }
        if !self.losses.is_empty() {
            return Err(format!("provider facts are lossy: {}", self.losses.join("; ")));
        }
        if self.facts.name.is_empty() || self.facts.version.is_empty() {
            return Err("provider facts need both a name and an exact version".to_string());
        }
        Ok(())
    }
}

/// Normalize one provider-native metadata document into the shared fact model.
/// The report is intentionally separate from `MetadataFacts`: unsupported or
/// ambiguous fields stay visible instead of becoming silent defaults.
pub fn normalize_provider_document(
    family: ProviderFamily,
    document: &str,
) -> ProviderFactReport {
    match family {
        ProviderFamily::Npm => npm_report(document),
        ProviderFamily::Cargo => cargo_report(document),
        ProviderFamily::PyPI => pypi_report(document),
        ProviderFamily::SwiftPM => swiftpm_report(document),
        ProviderFamily::Maven => maven_report(document),
        ProviderFamily::NuGet => nuget_report(document),
        ProviderFamily::Conan => conan_report(document),
        ProviderFamily::Vcpkg => vcpkg_report(document),
        ProviderFamily::Homebrew => homebrew_report(document),
        ProviderFamily::JetRegistry => jet_registry_report(document),
        ProviderFamily::Github => github_report(document),
        ProviderFamily::Binary => binary_report(document),
        ProviderFamily::Core | ProviderFamily::Nix | ProviderFamily::Path => {
            let mut facts = MetadataFacts::empty(family, "");
            facts.source_identity = format!("{}:document", facts.family.label());
            ProviderFactReport {
                facts,
                losses: vec!["this provider has no foreign metadata document shape".to_string()],
                conflicts: Vec::new(),
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataFacts {
    pub family: ProviderFamily,
    pub name: String,
    pub version: String,
    pub source_identity: String,
    pub integrity_hash: String,
    pub dependencies: Vec<String>,
    pub dev_dependencies: Vec<String>,
    pub build_dependencies: Vec<String>,
    pub scripts: Vec<String>,
    pub platforms: Vec<String>,
    pub license: String,
    pub bins: Vec<String>,
    pub trust_roots: Vec<String>,
    pub todos: Vec<String>,
    pub replacement_candidates: Vec<ReplacementOverlay>,
}

impl MetadataFacts {
    pub fn empty(family: ProviderFamily, name: impl Into<String>) -> MetadataFacts {
        MetadataFacts {
            family,
            name: name.into(),
            version: String::new(),
            source_identity: String::new(),
            integrity_hash: String::new(),
            dependencies: Vec::new(),
            dev_dependencies: Vec::new(),
            build_dependencies: Vec::new(),
            scripts: Vec::new(),
            platforms: Vec::new(),
            license: String::new(),
            bins: Vec::new(),
            trust_roots: Vec::new(),
            todos: Vec::new(),
            replacement_candidates: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderObject {
    pub family: ProviderFamily,
    pub ref_key: String,
    pub exact_identity: String,
    pub hash: String,
    pub platform: String,
    pub signature: String,
    pub audit: Vec<String>,
    pub sandbox_effects: Vec<String>,
    pub build_effects: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthorityGraph {
    pub locked_objects: Vec<ProviderObject>,
}

impl AuthorityGraph {
    pub fn add_locked(&mut self, object: ProviderObject) {
        self.locked_objects.push(object);
    }

    pub fn fetch_allowed(&self, request: &ProviderRequest) -> FetchDecision {
        if request.offline {
            if self.locked_objects.iter().any(|obj| {
                obj.family == request.family
                    && obj.ref_key == request.ref_key
                    && obj.exact_identity == request.exact_identity
                    && obj.hash == request.hash
                    && obj.platform == request.platform
            }) {
                FetchDecision::AllowedOfflineSatisfied
            } else {
                FetchDecision::DeniedOfflineMissingLock
            }
        } else {
            FetchDecision::AllowedNetworkWithGrant {
                authority: format!("provider.fetch.{}", request.family.label()),
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRequest {
    pub family: ProviderFamily,
    pub ref_key: String,
    pub exact_identity: String,
    pub hash: String,
    pub platform: String,
    pub offline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchDecision {
    AllowedOfflineSatisfied,
    AllowedNetworkWithGrant { authority: String },
    DeniedOfflineMissingLock,
}

pub fn normalize_npm(package_json: &str) -> MetadataFacts {
    let parsed = JSON::parse(package_json).ok();
    let obj = parsed.as_ref().and_then(|j| j.as_object().ok());
    let name = obj
        .and_then(|m| m.get("name"))
        .and_then(|v| v.as_str().ok())
        .unwrap_or("npm-package")
        .to_string();
    let mut facts = MetadataFacts::empty(ProviderFamily::Npm, name);
    facts.version = obj
        .and_then(|m| m.get("version"))
        .and_then(|v| v.as_str().ok())
        .unwrap_or("")
        .to_string();
    facts.license = obj
        .and_then(|m| m.get("license"))
        .and_then(|v| v.as_str().ok())
        .unwrap_or("")
        .to_string();
    if let Some(JSONValue::Object(deps)) = obj.and_then(|m| m.get("dependencies")) {
        facts.dependencies = deps.keys().cloned().collect();
    }
    if let Some(JSONValue::Object(scripts)) = obj.and_then(|m| m.get("scripts")) {
        facts.scripts = scripts.keys().cloned().collect();
    }
    if let Some(JSONValue::Object(bin)) = obj.and_then(|m| m.get("bin")) {
        facts.bins = bin.keys().cloned().collect();
    }
    facts.source_identity = format!("npm:{}@{}", facts.name, facts.version);
    facts
}

pub fn normalize_cargo(cargo_toml: &str) -> MetadataFacts {
    let name = toml_string(cargo_toml, "name").unwrap_or_else(|| "cargo-package".to_string());
    let mut facts = MetadataFacts::empty(ProviderFamily::Cargo, name);
    facts.version = toml_string(cargo_toml, "version").unwrap_or_default();
    facts.license = toml_string(cargo_toml, "license").unwrap_or_default();
    facts.dependencies = dependency_keys(cargo_toml, "[dependencies]");
    facts.build_dependencies = dependency_keys(cargo_toml, "[build-dependencies]");
    if cargo_toml.contains("build =") {
        facts.scripts.push("build.rs".to_string());
    }
    facts.source_identity = format!("cargo:{}@{}", facts.name, facts.version);
    facts
}

pub fn normalize_pypi(name: &str, version: &str, dynamic_metadata: bool) -> MetadataFacts {
    let mut facts = MetadataFacts::empty(ProviderFamily::PyPI, name);
    facts.version = version.to_string();
    facts.source_identity = format!("pypi:{name}@{version}");
    if dynamic_metadata {
        facts
            .todos
            .push("dynamic metadata must be resolved before build execution".to_string());
    }
    facts
}

pub fn normalize_swiftpm(name: &str, revision: &str) -> MetadataFacts {
    let mut facts = MetadataFacts::empty(ProviderFamily::SwiftPM, name);
    facts.version = revision.to_string();
    facts.source_identity = format!("swiftpm:{name}@{revision}");
    facts.integrity_hash = revision.to_string();
    facts
}

pub fn binary_object(
    name: &str,
    hash: &str,
    platform: &str,
    signature: &str,
) -> Result<ProviderObject, String> {
    if hash.trim().is_empty() {
        return Err("binary provider requires a hash".to_string());
    }
    if platform.trim().is_empty() {
        return Err("binary provider requires a platform".to_string());
    }
    Ok(ProviderObject {
        family: ProviderFamily::Binary,
        ref_key: name.to_string(),
        exact_identity: format!("binary:{name}:{platform}:{hash}"),
        hash: hash.to_string(),
        platform: platform.to_string(),
        signature: signature.to_string(),
        audit: vec!["hash verified before realization".to_string()],
        sandbox_effects: vec!["no build execution".to_string()],
        build_effects: Vec::new(),
    })
}

fn npm_report(document: &str) -> ProviderFactReport {
    let parsed = JSON::parse(document).ok();
    let Some(JSONValue::Object(object)) = parsed else {
        return empty_report(ProviderFamily::Npm, "npm", "package.json is not valid JSON");
    };
    let name = json_string(&object, "name");
    let mut facts = MetadataFacts::empty(ProviderFamily::Npm, name.clone().unwrap_or_default());
    facts.version = json_string(&object, "version").unwrap_or_default();
    facts.license = json_string(&object, "license").unwrap_or_default();
    facts.dependencies = json_keys(&object, "dependencies");
    facts.dev_dependencies = json_keys(&object, "devDependencies");
    facts.scripts = json_keys(&object, "scripts");
    facts.bins = json_keys(&object, "bin");
    facts.source_identity = format!("npm:{}@{}", facts.name, facts.version);
    let mut report = report_with_identity(facts);
    if object.contains_key("optionalDependencies") {
        report
            .losses
            .push("optionalDependencies need an explicit platform projection".to_string());
    }
    report
}

fn cargo_report(document: &str) -> ProviderFactReport {
    let mut facts = MetadataFacts::empty(
        ProviderFamily::Cargo,
        toml_string(document, "name").unwrap_or_default(),
    );
    facts.version = toml_string(document, "version").unwrap_or_default();
    facts.license = toml_string(document, "license").unwrap_or_default();
    facts.dependencies = dependency_keys(document, "[dependencies]");
    facts.dev_dependencies = dependency_keys(document, "[dev-dependencies]");
    facts.build_dependencies = dependency_keys(document, "[build-dependencies]");
    if document.lines().any(|line| line.trim_start().starts_with("build =")) {
        facts.scripts.push("build.rs".to_string());
    }
    facts.source_identity = format!("cargo:{}@{}", facts.name, facts.version);
    report_with_identity(facts)
}

fn pypi_report(document: &str) -> ProviderFactReport {
    let name = metadata_line(document, "name").or_else(|| toml_string(document, "name"));
    let version = metadata_line(document, "version")
        .or_else(|| toml_string(document, "version"));
    let mut facts = MetadataFacts::empty(ProviderFamily::PyPI, name.unwrap_or_default());
    facts.version = version.unwrap_or_default();
    facts.dependencies = metadata_list(document, "requires-dist");
    facts.source_identity = format!("pypi:{}@{}", facts.name, facts.version);
    let mut report = report_with_identity(facts);
    if document.contains("dynamic =") || document.contains("dynamic:") {
        report
            .losses
            .push("dynamic Python metadata must be resolved to an exact lock".to_string());
    }
    report
}

fn swiftpm_report(document: &str) -> ProviderFactReport {
    let parsed = JSON::parse(document).ok();
    let mut facts = MetadataFacts::empty(ProviderFamily::SwiftPM, String::new());
    if let Some(JSONValue::Object(root)) = parsed {
        if let Some(JSONValue::Array(pins)) = root.get("pins") {
            if pins.len() == 1 {
                if let Some(JSONValue::Object(pin)) = pins.first() {
                    facts.name = json_string(pin, "identity").unwrap_or_default();
                    facts.version = json_string(pin, "state")
                        .or_else(|| json_string(pin, "revision"))
                        .unwrap_or_default();
                    facts.integrity_hash = facts.version.clone();
                }
            } else if pins.len() > 1 {
                facts.name = "swiftpm-lock".to_string();
                facts.version = "set".to_string();
                facts.dependencies = pins
                    .iter()
                    .filter_map(|pin| match pin {
                        JSONValue::Object(pin) => json_string(pin, "identity"),
                        _ => None,
                    })
                    .collect();
            }
        }
    }
    facts.source_identity = format!("swiftpm:{}@{}", facts.name, facts.version);
    let mut report = report_with_identity(facts);
    if report.facts.version == "set" {
        report.losses.push(
            "Package.resolved contains multiple pins; normalize each pin before realization"
                .to_string(),
        );
    }
    report
}

fn maven_report(document: &str) -> ProviderFactReport {
    let name = xml_tag(document, "artifactId").unwrap_or_default();
    let group = xml_tag(document, "groupId").unwrap_or_default();
    let version = xml_tag(document, "version").unwrap_or_default();
    let mut facts = MetadataFacts::empty(ProviderFamily::Maven, name);
    facts.version = version;
    facts.source_identity = format!("maven:{}:{}@{}", group, facts.name, facts.version);
    facts.dependencies = xml_tags(document, "artifactId")
        .into_iter()
        .filter(|dependency| dependency != &facts.name)
        .collect();
    report_with_identity(facts)
}

fn nuget_report(document: &str) -> ProviderFactReport {
    let mut facts = MetadataFacts::empty(ProviderFamily::NuGet, String::new());
    let mut packages = Vec::new();
    for line in document.lines() {
        if let Some(name) = xml_attribute(line, "Include") {
            let version = xml_attribute(line, "Version").unwrap_or_default();
            packages.push((name, version));
        }
        if let Some(name) = xml_attribute(line, "id") {
            let version = xml_attribute(line, "version").unwrap_or_default();
            packages.push((name, version));
        }
    }
    packages.sort();
    packages.dedup();
    if packages.len() == 1 {
        let (name, version) = packages.remove(0);
        facts.name = name;
        facts.version = version;
    } else {
        facts.name = "nuget-lock".to_string();
        facts.version = "set".to_string();
        facts.dependencies = packages.iter().map(|(name, _)| name.clone()).collect();
    }
    facts.source_identity = format!("nuget:{}@{}", facts.name, facts.version);
    let mut report = report_with_identity(facts);
    if report.facts.version == "set" {
        report.losses.push(
            "NuGet metadata contains multiple packages; lock each package identity separately"
                .to_string(),
        );
    }
    report
}

fn conan_report(document: &str) -> ProviderFactReport {
    let mut facts = MetadataFacts::empty(
        ProviderFamily::Conan,
        line_value(document, "name").unwrap_or_default(),
    );
    facts.version = line_value(document, "version").unwrap_or_default();
    facts.dependencies = quoted_values_after(document, "requires");
    facts.source_identity = format!("conan:{}@{}", facts.name, facts.version);
    report_with_identity(facts)
}

fn vcpkg_report(document: &str) -> ProviderFactReport {
    let parsed = JSON::parse(document).ok();
    let Some(JSONValue::Object(object)) = parsed else {
        return empty_report(ProviderFamily::Vcpkg, "vcpkg", "vcpkg.json is not valid JSON");
    };
    let mut facts = MetadataFacts::empty(
        ProviderFamily::Vcpkg,
        json_string(&object, "name").unwrap_or_default(),
    );
    facts.version = json_string(&object, "version-string")
        .or_else(|| json_string(&object, "version"))
        .unwrap_or_default();
    facts.dependencies = json_array_strings(&object, "dependencies");
    facts.source_identity = format!("vcpkg:{}@{}", facts.name, facts.version);
    report_with_identity(facts)
}

fn homebrew_report(document: &str) -> ProviderFactReport {
    let parsed = JSON::parse(document).ok();
    let Some(JSONValue::Object(object)) = parsed else {
        return empty_report(ProviderFamily::Homebrew, "homebrew", "formula metadata is not valid JSON");
    };
    let mut facts = MetadataFacts::empty(
        ProviderFamily::Homebrew,
        json_string(&object, "name").unwrap_or_default(),
    );
    facts.version = json_string(&object, "version").unwrap_or_default();
    facts.license = json_string(&object, "license").unwrap_or_default();
    facts.dependencies = json_array_strings(&object, "dependencies");
    facts.source_identity = format!("homebrew:{}@{}", facts.name, facts.version);
    report_with_identity(facts)
}

fn jet_registry_report(document: &str) -> ProviderFactReport {
    let line = document.lines().find(|line| !line.trim().is_empty()).unwrap_or_default();
    let parsed = JSON::parse(line).ok();
    let Some(JSONValue::Object(object)) = parsed else {
        return empty_report(ProviderFamily::JetRegistry, "jet-registry", "registry metadata is not valid JSON");
    };
    let mut facts = MetadataFacts::empty(
        ProviderFamily::JetRegistry,
        json_string(&object, "name").unwrap_or_default(),
    );
    facts.version = json_string(&object, "version").unwrap_or_default();
    facts.integrity_hash = json_string(&object, "content_hash").unwrap_or_default();
    facts.source_identity = format!("jet-registry:{}@{}", facts.name, facts.version);
    let mut report = report_with_identity(facts);
    if report.facts.integrity_hash.is_empty() {
        report.losses.push("registry entry has no content hash".to_string());
    }
    report
}

fn github_report(document: &str) -> ProviderFactReport {
    let parsed = JSON::parse(document).ok();
    let Some(JSONValue::Object(object)) = parsed else {
        return empty_report(ProviderFamily::Github, "github", "release metadata is not valid JSON");
    };
    let mut facts = MetadataFacts::empty(
        ProviderFamily::Github,
        json_string(&object, "name").unwrap_or_default(),
    );
    facts.version = json_string(&object, "tag_name")
        .or_else(|| json_string(&object, "version"))
        .unwrap_or_default();
    facts.source_identity = format!("github:{}@{}", facts.name, facts.version);
    report_with_identity(facts)
}

fn binary_report(document: &str) -> ProviderFactReport {
    let parsed = JSON::parse(document).ok();
    let Some(JSONValue::Object(object)) = parsed else {
        return empty_report(ProviderFamily::Binary, "binary", "binary metadata is not valid JSON");
    };
    let mut facts = MetadataFacts::empty(
        ProviderFamily::Binary,
        json_string(&object, "name").unwrap_or_default(),
    );
    facts.version = json_string(&object, "version").unwrap_or_default();
    facts.integrity_hash = json_string(&object, "hash")
        .or_else(|| json_string(&object, "sha256"))
        .unwrap_or_default();
    facts.platforms = json_array_strings(&object, "platforms");
    facts.source_identity = format!("binary:{}@{}", facts.name, facts.version);
    let mut report = report_with_identity(facts);
    if report.facts.integrity_hash.is_empty() {
        report.losses.push("binary metadata has no content hash".to_string());
    }
    report
}

fn report_with_identity(facts: MetadataFacts) -> ProviderFactReport {
    let mut losses = Vec::new();
    if facts.name.is_empty() {
        losses.push("provider metadata has no package name".to_string());
    }
    if facts.version.is_empty() {
        losses.push("provider metadata has no exact version".to_string());
    }
    ProviderFactReport {
        facts,
        losses,
        conflicts: Vec::new(),
    }
}

fn empty_report(family: ProviderFamily, name: &str, loss: &str) -> ProviderFactReport {
    let facts = MetadataFacts::empty(family, name);
    ProviderFactReport {
        facts,
        losses: vec![loss.to_string()],
        conflicts: Vec::new(),
    }
}

fn json_string(
    object: &std::collections::BTreeMap<String, JSONValue>,
    key: &str,
) -> Option<String> {
    match object.get(key) {
        Some(JSONValue::Str(value)) if !value.trim().is_empty() => Some(value.clone()),
        _ => None,
    }
}

fn json_keys(
    object: &std::collections::BTreeMap<String, JSONValue>,
    key: &str,
) -> Vec<String> {
    match object.get(key) {
        Some(JSONValue::Object(values)) => values.keys().cloned().collect(),
        _ => Vec::new(),
    }
}

fn json_array_strings(
    object: &std::collections::BTreeMap<String, JSONValue>,
    key: &str,
) -> Vec<String> {
    match object.get(key) {
        Some(JSONValue::Array(values)) => values
            .iter()
            .filter_map(|value| match value {
                JSONValue::Str(value) => Some(value.clone()),
                JSONValue::Object(value) => value.get("name").and_then(|value| match value {
                    JSONValue::Str(value) => Some(value.clone()),
                    _ => None,
                }),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn xml_tag(document: &str, tag: &str) -> Option<String> {
    let start = format!("<{tag}>");
    let end = format!("</{tag}>");
    document.split_once(&start)?.1.split_once(&end).map(|value| value.0.trim().to_string())
}

fn xml_tags(document: &str, tag: &str) -> Vec<String> {
    let start = format!("<{tag}>");
    let end = format!("</{tag}>");
    let mut rest = document;
    let mut values = Vec::new();
    while let Some(after_start) = rest.split_once(&start).map(|value| value.1) {
        let Some((value, after_end)) = after_start.split_once(&end) else { break; };
        values.push(value.trim().to_string());
        rest = after_end;
    }
    values
}

fn xml_attribute(line: &str, key: &str) -> Option<String> {
    let marker = format!("{key}=\"");
    line.split_once(&marker)?.1.split_once('"').map(|value| value.0.to_string())
}

fn line_value(document: &str, key: &str) -> Option<String> {
    document.lines().find_map(|line| {
        let (left, right) = line.split_once('=')?;
        (left.trim() == key).then(|| right.trim().trim_matches('"').to_string())
    })
}

fn metadata_line(document: &str, key: &str) -> Option<String> {
    document.lines().find_map(|line| {
        let (left, right) = line.split_once(':')?;
        left.trim().eq_ignore_ascii_case(key).then(|| right.trim().to_string())
    })
}

fn metadata_list(document: &str, key: &str) -> Vec<String> {
    document
        .lines()
        .filter_map(|line| {
            let (left, right) = line.split_once(':')?;
            left.trim().eq_ignore_ascii_case(key).then(|| right.trim().to_string())
        })
        .collect()
}

fn quoted_values_after(document: &str, key: &str) -> Vec<String> {
    document
        .lines()
        .find(|line| line.trim_start().starts_with(key))
        .map(|line| {
            line.split('"')
                .enumerate()
                .filter_map(|(index, value)| (index % 2 == 1).then(|| value.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

fn toml_string(raw: &str, key: &str) -> Option<String> {
    raw.lines().find_map(|line| {
        let line = line.trim();
        let (k, v) = line.split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_string())
    })
}

fn dependency_keys(raw: &str, section: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_section = false;
    for line in raw.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_section = line == section;
            continue;
        }
        if in_section {
            if let Some((key, _)) = line.split_once('=') {
                out.push(key.trim().to_string());
            }
        }
    }
    out
}
