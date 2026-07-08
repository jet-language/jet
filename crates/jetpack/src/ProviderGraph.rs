//! Federated provider facts under Jetpack authority (D-WD6).
//!
//! External provider prefixes and trust-root config remain owner-gated. This
//! module models provider metadata/fetch/lock/sandbox/signature/audit facts.

pub use super::Replacement::ReplacementCandidate as ReplacementOverlay;
use super::JSON::{self, Json};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProviderFamily {
    Core,
    Nix,
    Path,
    Github,
    Npm,
    PyPI,
    Cargo,
    SwiftPM,
    Binary,
}

impl ProviderFamily {
    pub fn label(&self) -> &'static str {
        match self {
            ProviderFamily::Core => "core",
            ProviderFamily::Nix => "nix",
            ProviderFamily::Path => "path",
            ProviderFamily::Github => "github",
            ProviderFamily::Npm => "npm",
            ProviderFamily::PyPI => "pypi",
            ProviderFamily::Cargo => "cargo",
            ProviderFamily::SwiftPM => "swiftpm",
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
    pub fn full(family: ProviderFamily) -> ProviderContract {
        ProviderContract {
            family,
            parses_refs: true,
            probes_metadata: true,
            resolves_channels: true,
            fetches_bytes: true,
            verifies_hash_signature: true,
            exposes_audit_facts: true,
            reports_offline_satisfiability: true,
        }
    }
}

pub fn built_in_contracts() -> Vec<ProviderContract> {
    vec![
        ProviderContract::full(ProviderFamily::Core),
        ProviderContract::full(ProviderFamily::Nix),
        ProviderContract::full(ProviderFamily::Path),
        ProviderContract::full(ProviderFamily::Github),
    ]
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
    if let Some(Json::Object(deps)) = obj.and_then(|m| m.get("dependencies")) {
        facts.dependencies = deps.keys().cloned().collect();
    }
    if let Some(Json::Object(scripts)) = obj.and_then(|m| m.get("scripts")) {
        facts.scripts = scripts.keys().cloned().collect();
    }
    if let Some(Json::Object(bin)) = obj.and_then(|m| m.get("bin")) {
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
