//! Federated provider facts under Jetpack authority (D-WD6).
//!
//! External provider prefixes and trust-root config remain owner-gated. This
//! module models provider metadata/fetch/lock/sandbox/signature/audit facts.

pub use super::Replacement::ReplacementCandidate as ReplacementOverlay;
use super::JSON::{self, JSONValue};
use jet_pkg_model::ProviderFacts::{ProviderFactValue, ProviderFacts};

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
    pub native_format: String,
    pub native_document: String,
}

impl ProviderFactReport {
    pub fn is_lossless(&self) -> bool {
        self.losses.is_empty() && self.conflicts.is_empty() && self.shared_facts().is_lossless()
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.conflicts.is_empty() {
            return Err(format!(
                "provider facts conflict: {}",
                self.conflicts.join("; ")
            ));
        }
        if !self.losses.is_empty() {
            return Err(format!(
                "provider facts are lossy: {}",
                self.losses.join("; ")
            ));
        }
        if self.facts.name.is_empty() {
            return Err("provider facts need a package name".to_string());
        }
        if metadata_identity_selector(&self.facts).1.is_empty() {
            return Err("provider facts need an exact version, revision, or digest".to_string());
        }
        if self.facts.source_identity.is_empty() {
            return Err("provider facts need a resolved source identity".to_string());
        }
        if self.native_format.is_empty() || self.native_document.is_empty() {
            return Err("provider facts need the native document and its format".to_string());
        }
        self.shared_facts().validate()?;
        Ok(())
    }

    /// Lower the provider report into the one carrier used by plan, lock, and
    /// explain. The native document remains byte-for-byte available on that
    /// carrier; the typed projection is additive and never a replacement.
    pub fn shared_facts_for(&self, reference: &str) -> ProviderFacts {
        let mut shared = ProviderFacts::for_reference(self.facts.family.label(), reference);
        shared.set_resolved_source(&self.facts.source_identity);
        shared.set_native_document(&self.native_format, &self.native_document);
        add_metadata_facts(&mut shared, &self.facts);
        let selector_identity = if !shared.selector.version.is_empty() {
            Some(shared.selector.version.clone())
        } else if !shared.selector.revision.is_empty() {
            Some(shared.selector.revision.clone())
        } else if !shared.selector.digest.is_empty() {
            Some(shared.selector.digest.clone())
        } else {
            None
        };
        let (_, expected_identity) = metadata_identity_selector(&self.facts);
        if selector_identity.as_deref() != Some(expected_identity.as_str()) {
            shared.add_conflict(
                "provider.selector.identity",
                &expected_identity,
                selector_identity.as_deref().unwrap_or("<missing>"),
                "provider.metadata",
            );
        }
        for (index, loss) in self.losses.iter().enumerate() {
            shared.add_loss(
                &format!("provider.loss.{index}"),
                loss,
                "provider.native_document",
            );
        }
        for (index, conflict) in self.conflicts.iter().enumerate() {
            shared.add_conflict(
                &format!("provider.conflict.{index}"),
                "<provider-native-left-unavailable>",
                conflict,
                "provider.native_document",
            );
        }
        shared
    }

    pub fn shared_facts(&self) -> ProviderFacts {
        let (selector_key, selector_value) = metadata_identity_selector(&self.facts);
        let reference = format!(
            "{}#{}={}@{}",
            self.facts.name,
            selector_key,
            selector_value,
            self.facts.family.label()
        );
        self.shared_facts_for(&reference)
    }

    pub fn export_json(&self) -> String {
        self.shared_facts().to_json()
    }

    /// Make a semantic-lock record without inventing a second provider fact
    /// schema. A lossy report is refused before it can enter the lock.
    pub fn lock_record(
        &self,
        owner_package: &str,
        reference: &str,
        platform: &str,
    ) -> Result<crate::SemanticLock::SemanticRecord, String> {
        let requested = self.shared_facts_for(reference);
        requested.validate()?;
        let shared = self.shared_facts();
        shared.validate()?;
        if requested.qualified_reference() != shared.qualified_reference() {
            return Err(format!(
                "provider lock reference `{reference}` disagrees with metadata identity `{}`",
                shared.qualified_reference()
            ));
        }
        let qualified_reference = shared.qualified_reference();
        let mut record = crate::SemanticLock::SemanticRecord::new(
            crate::SemanticLock::LockIdentity {
                kind: crate::SemanticLock::LockRecordKind::Package,
                key: format!("provider:{qualified_reference}"),
                exact: qualified_reference.clone(),
                hash: shared.digest(),
                platform: platform.to_string(),
            },
            crate::SemanticLock::LockRationale {
                owner_package: owner_package.to_string(),
                reason: "provider-native facts lowered through the shared carrier".to_string(),
                source_ref: reference.to_string(),
                provider: self.facts.family.label().to_string(),
                exact_output: self.facts.source_identity.clone(),
                ..Default::default()
            },
        );
        record
            .future_fields
            .insert("provider-facts".to_string(), shared.to_json());
        record
            .future_fields
            .insert("provider-facts-digest".to_string(), shared.digest());
        Ok(record)
    }
}

fn add_metadata_facts(shared: &mut ProviderFacts, facts: &MetadataFacts) {
    for (key, value) in [
        ("package.name", facts.name.as_str()),
        ("package.version", facts.version.as_str()),
        ("package.source", facts.source_identity.as_str()),
        ("package.integrity", facts.integrity_hash.as_str()),
        ("package.license", facts.license.as_str()),
    ] {
        if !value.is_empty() {
            shared.add_fact(
                key,
                ProviderFactValue::Text(value.to_string()),
                "provider.metadata",
            );
        }
    }
    for (key, values) in [
        ("package.dependencies", &facts.dependencies),
        ("package.dev_dependencies", &facts.dev_dependencies),
        ("package.build_dependencies", &facts.build_dependencies),
        ("package.scripts", &facts.scripts),
        ("package.platforms", &facts.platforms),
        ("package.bins", &facts.bins),
        ("package.trust_roots", &facts.trust_roots),
        ("package.todos", &facts.todos),
    ] {
        if !values.is_empty() {
            shared.add_fact(
                key,
                ProviderFactValue::List(
                    values
                        .iter()
                        .map(|value| ProviderFactValue::Text(value.clone()))
                        .collect(),
                ),
                "provider.metadata",
            );
        }
    }
    for (key, values) in &facts.typed {
        shared.add_fact(
            key,
            ProviderFactValue::List(
                values
                    .iter()
                    .map(|value| ProviderFactValue::Text(value.clone()))
                    .collect(),
            ),
            "provider.native_projection",
        );
    }
    if !facts.replacement_candidates.is_empty() {
        let candidates = facts
            .replacement_candidates
            .iter()
            .map(|candidate| {
                let mut fields = std::collections::BTreeMap::new();
                fields.insert(
                    "foreign_identity".to_string(),
                    ProviderFactValue::Text(candidate.foreign_identity.ref_string()),
                );
                fields.insert(
                    "native_identity".to_string(),
                    ProviderFactValue::Text(candidate.native_identity.ref_string()),
                );
                fields.insert(
                    "license".to_string(),
                    ProviderFactValue::Text(candidate.license.clone()),
                );
                fields.insert(
                    "proof_status".to_string(),
                    ProviderFactValue::Text(format!("{:?}", candidate.proof_status)),
                );
                fields.insert(
                    "proof_digest".to_string(),
                    ProviderFactValue::Text(candidate.proof_digest.clone()),
                );
                for (key, values) in [
                    ("covered_public_symbols", &candidate.covered_public_symbols),
                    ("unsupported_symbols", &candidate.unsupported_symbols),
                    ("platforms", &candidate.platforms),
                    ("proof_inputs", &candidate.proof_inputs),
                ] {
                    fields.insert(
                        key.to_string(),
                        ProviderFactValue::List(
                            values
                                .iter()
                                .map(|value| ProviderFactValue::Text(value.clone()))
                                .collect(),
                        ),
                    );
                }
                ProviderFactValue::Map(fields)
            })
            .collect();
        shared.add_fact(
            "package.replacement_candidates",
            ProviderFactValue::List(candidates),
            "provider.replacement",
        );
    }
}

fn metadata_identity_selector(facts: &MetadataFacts) -> (&'static str, String) {
    if facts.family == ProviderFamily::SwiftPM {
        if let Some(revision) = facts
            .typed
            .get("provider.revision")
            .and_then(|values| values.first())
            .filter(|revision| !revision.is_empty())
        {
            return ("revision", revision.clone());
        }
    }
    if !facts.version.is_empty() {
        return ("version", facts.version.clone());
    }
    if !facts.integrity_hash.is_empty() {
        return ("digest", facts.integrity_hash.clone());
    }
    ("version", String::new())
}

/// Normalize one provider-native metadata document into the shared fact model.
/// The report is intentionally separate from `MetadataFacts`: unsupported or
/// ambiguous fields stay visible instead of becoming silent defaults.
pub fn normalize_provider_document(family: ProviderFamily, document: &str) -> ProviderFactReport {
    // Compute the format first: the match below consumes `family` (the
    // Core/Nix/Path arm moves it into `MetadataFacts::empty`).
    let native_format = provider_document_format(&family, document);
    let mut report = match family {
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
                native_format: String::new(),
                native_document: String::new(),
            }
        }
    };
    report.native_format = native_format;
    report.native_document = document.to_string();
    report
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
    pub typed: std::collections::BTreeMap<String, Vec<String>>,
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
            typed: std::collections::BTreeMap::new(),
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
        .unwrap_or_default()
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
    let name = toml_string(cargo_toml, "name").unwrap_or_default();
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
        .typed
        .insert("provider.revision".to_string(), vec![revision.to_string()]);
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
    if document
        .lines()
        .any(|line| line.trim_start().starts_with("build ="))
    {
        facts.scripts.push("build.rs".to_string());
    }
    facts.source_identity = format!("cargo:{}@{}", facts.name, facts.version);
    report_with_identity(facts)
}

fn pypi_report(document: &str) -> ProviderFactReport {
    let name = metadata_line(document, "name").or_else(|| toml_string(document, "name"));
    let version = metadata_line(document, "version").or_else(|| toml_string(document, "version"));
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
                    let (version, revision) = match pin.get("state") {
                        Some(JSONValue::Object(state)) => (
                            json_string(state, "version"),
                            json_string(state, "revision"),
                        ),
                        _ => (None, json_string(pin, "revision")),
                    };
                    facts.version = version
                        .clone()
                        .or_else(|| revision.clone())
                        .unwrap_or_default();
                    if let Some(revision) = revision {
                        facts.integrity_hash = revision.clone();
                        facts
                            .typed
                            .insert("provider.revision".to_string(), vec![revision]);
                    } else {
                        facts.integrity_hash = facts.version.clone();
                    }
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
    let source_revision = facts
        .typed
        .get("provider.revision")
        .and_then(|values| values.first())
        .filter(|revision| !revision.is_empty())
        .cloned()
        .unwrap_or_else(|| facts.version.clone());
    facts.source_identity = format!("swiftpm:{}@{}", facts.name, source_revision);
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
    facts.dependencies = xml_tag_values(document, "artifactId")
        .into_iter()
        .filter(|dependency| dependency != &facts.name)
        .collect();
    report_with_identity(facts)
}

fn nuget_report(document: &str) -> ProviderFactReport {
    if let Ok(JSONValue::Object(object)) = JSON::parse(document) {
        return nuget_json_report(&object);
    }
    let mut facts = MetadataFacts::empty(ProviderFamily::NuGet, String::new());
    let mut packages = Vec::new();
    let root_package = match (xml_tag(document, "id"), xml_tag(document, "version")) {
        (Some(name), Some(version)) => {
            let package = (name, version);
            packages.push(package.clone());
            Some(package)
        }
        _ => None,
    };
    for tag in xml_opening_tags(document, "PackageReference") {
        if let Some(name) = xml_attribute(&tag, "Include") {
            packages.push((name, xml_attribute(&tag, "Version").unwrap_or_default()));
        }
    }
    let dependency_values = xml_opening_tags(document, "dependency")
        .into_iter()
        .filter_map(|tag| {
            let name = xml_attribute(&tag, "id")?;
            let version = xml_attribute(&tag, "version").unwrap_or_default();
            let dependency = format_dependency(&name, &version);
            packages.push((name, version));
            Some(dependency)
        })
        .collect::<Vec<_>>();
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
    facts.license = xml_tag(document, "license")
        .or_else(|| xml_tag(document, "licenseUrl"))
        .unwrap_or_default();
    facts.platforms = xml_opening_tags(document, "group")
        .into_iter()
        .filter_map(|tag| xml_attribute(&tag, "targetFramework"))
        .collect();
    facts.dependencies = if !dependency_values.is_empty() {
        dependency_values
    } else if root_package.is_none() && packages.len() > 1 {
        packages
            .iter()
            .map(|(name, version)| format_dependency(name, version))
            .collect()
    } else {
        Vec::new()
    };
    if let Some(repository) = xml_opening_tags(document, "repository")
        .into_iter()
        .find_map(|tag| xml_attribute(&tag, "url"))
    {
        facts
            .typed
            .insert("provider.repository".to_string(), vec![repository]);
    }
    facts.integrity_hash = xml_tag(document, "contentHash").unwrap_or_default();
    facts.source_identity = format!("nuget:{}@{}", facts.name, facts.version);
    let mut report = report_with_identity(facts);
    if packages.len() > 1 || report.facts.version == "set" {
        report.losses.push(
            "NuGet metadata contains multiple packages; lock each package identity separately"
                .to_string(),
        );
    }
    report
}

fn nuget_json_report(object: &std::collections::BTreeMap<String, JSONValue>) -> ProviderFactReport {
    let mut packages = Vec::new();
    if let Some(JSONValue::Object(dependencies)) = object.get("dependencies") {
        for packages_for_framework in dependencies.values() {
            if let JSONValue::Object(entries) = packages_for_framework {
                for (name, value) in entries {
                    if let JSONValue::Object(entry) = value {
                        let version = json_string(entry, "resolved")
                            .or_else(|| json_string(entry, "version"))
                            .or_else(|| json_string(entry, "requested"))
                            .unwrap_or_default();
                        packages.push((name.clone(), version));
                    }
                }
            }
        }
    }
    if let Some(JSONValue::Object(package)) = object.get("package") {
        if let (Some(name), Some(version)) = (
            json_string(package, "id").or_else(|| json_string(package, "name")),
            json_string(package, "version"),
        ) {
            packages.push((name, version));
        }
    }
    packages.sort();
    packages.dedup();
    let mut facts = MetadataFacts::empty(
        ProviderFamily::NuGet,
        json_string(object, "id")
            .or_else(|| json_string(object, "name"))
            .unwrap_or_default(),
    );
    if packages.len() == 1 {
        facts.name = packages[0].0.clone();
        facts.version = packages[0].1.clone();
    } else if packages.len() > 1 {
        facts.name = "nuget-lock".to_string();
        facts.version = "set".to_string();
        facts.dependencies = packages
            .iter()
            .map(|(name, version)| format_dependency(name, version))
            .collect();
    } else {
        facts.version = json_string(object, "version").unwrap_or_default();
    }
    facts.integrity_hash = json_string(object, "contentHash")
        .or_else(|| json_string(object, "hash"))
        .unwrap_or_default();
    facts.source_identity = format!("nuget:{}@{}", facts.name, facts.version);
    let mut report = report_with_identity(facts);
    if packages.len() > 1 {
        report.losses.push(
            "NuGet lock metadata contains multiple package identities; lock each package separately"
                .to_string(),
        );
    }
    report
}

fn conan_report(document: &str) -> ProviderFactReport {
    if let Ok(JSONValue::Object(object)) = JSON::parse(document) {
        return conan_json_report(&object);
    }
    let names = assignment_values(document, "name");
    let versions = assignment_values(document, "version");
    let mut facts = MetadataFacts::empty(
        ProviderFamily::Conan,
        names.first().cloned().unwrap_or_default(),
    );
    facts.version = versions.first().cloned().unwrap_or_default();
    facts.license = assignment_values(document, "license")
        .first()
        .cloned()
        .unwrap_or_default();
    facts.dependencies = quoted_values_after_all(document, "requires");
    facts.build_dependencies = quoted_values_after_all(document, "tool_requires");
    facts
        .build_dependencies
        .extend(quoted_values_after_all(document, "build_requires"));
    facts.platforms = quoted_values_after_all(document, "settings");
    for key in ["options", "generators", "settings"] {
        let values = quoted_values_after_all(document, key);
        if !values.is_empty() {
            facts.typed.insert(format!("conan.{key}"), values);
        }
    }
    facts.source_identity = format!("conan:{}@{}", facts.name, facts.version);
    let mut report = report_with_identity(facts);
    if names.windows(2).any(|values| values[0] != values[1]) {
        report
            .conflicts
            .push("Conan recipe declares conflicting package names".to_string());
    }
    if versions.windows(2).any(|values| values[0] != values[1]) {
        report
            .conflicts
            .push("Conan recipe declares conflicting package versions".to_string());
    }
    if document.lines().any(|line| {
        let line = line.trim();
        line.starts_with("def build")
            || line.contains("self.run(")
            || line.starts_with("python_requires")
    }) {
        report.losses.push(
            "Conan recipe contains executable build or Python hook semantics; transport must be verified before realization"
                .to_string(),
        );
    }
    report
}

fn conan_json_report(object: &std::collections::BTreeMap<String, JSONValue>) -> ProviderFactReport {
    let mut facts = MetadataFacts::empty(
        ProviderFamily::Conan,
        json_string(object, "name").unwrap_or_default(),
    );
    facts.version = json_string(object, "version")
        .or_else(|| json_string(object, "ref").and_then(|value| conan_ref_part(&value, 1)))
        .unwrap_or_default();
    facts.license = json_string(object, "license").unwrap_or_default();
    facts.dependencies = json_array_strings(object, "requires");
    facts.build_dependencies = json_array_strings(object, "tool_requires");
    facts.source_identity = format!("conan:{}@{}", facts.name, facts.version);
    let mut report = report_with_identity(facts);
    if object.contains_key("requires") && report.facts.dependencies.is_empty() {
        report
            .losses
            .push("Conan lock requires are not a string array".to_string());
    }
    report
}

fn vcpkg_report(document: &str) -> ProviderFactReport {
    let parsed = JSON::parse(document).ok();
    let Some(JSONValue::Object(object)) = parsed else {
        return empty_report(
            ProviderFamily::Vcpkg,
            "vcpkg",
            "vcpkg.json is not valid JSON",
        );
    };
    let mut facts = MetadataFacts::empty(
        ProviderFamily::Vcpkg,
        json_string(&object, "name").unwrap_or_default(),
    );
    let version_fields = [
        "version",
        "version-string",
        "version-semver",
        "version-date",
    ];
    let versions = version_fields
        .iter()
        .filter_map(|key| json_string(&object, key).map(|value| (*key, value)))
        .collect::<Vec<_>>();
    facts.version = versions
        .first()
        .map(|(_, version)| version.clone())
        .unwrap_or_default();
    facts.dependencies = vcpkg_dependencies(&object, &mut facts.typed);
    facts.platforms = json_string(&object, "supports")
        .map(|value| vec![value])
        .unwrap_or_default();
    facts.license = json_string(&object, "license").unwrap_or_default();
    for key in ["builtin-baseline", "overrides", "features"] {
        if let Some(value) = object.get(key) {
            facts
                .typed
                .insert(format!("vcpkg.{key}"), vec![json_value_text(value)]);
        }
    }
    facts.source_identity = format!("vcpkg:{}@{}", facts.name, facts.version);
    let mut report = report_with_identity(facts);
    if versions.windows(2).any(|values| values[0].1 != values[1].1) {
        report
            .conflicts
            .push("vcpkg manifest declares conflicting version fields".to_string());
    }
    report
}

fn homebrew_report(document: &str) -> ProviderFactReport {
    let parsed = JSON::parse(document).ok();
    let Some(JSONValue::Object(object)) = parsed else {
        return empty_report(
            ProviderFamily::Homebrew,
            "homebrew",
            "formula metadata is not valid JSON",
        );
    };
    let mut facts = MetadataFacts::empty(
        ProviderFamily::Homebrew,
        json_string(&object, "name").unwrap_or_default(),
    );
    facts.version = json_string(&object, "version")
        .or_else(|| {
            object
                .get("versions")
                .and_then(|value| value.as_object().ok())
                .and_then(|versions| json_string(versions, "stable"))
        })
        .unwrap_or_default();
    facts.license = json_string(&object, "license").unwrap_or_default();
    facts.dependencies = json_array_strings(&object, "dependencies");
    facts.source_identity = format!("homebrew:{}@{}", facts.name, facts.version);
    report_with_identity(facts)
}

fn jet_registry_report(document: &str) -> ProviderFactReport {
    let line = document
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default();
    let parsed = JSON::parse(line).ok();
    let Some(JSONValue::Object(object)) = parsed else {
        return empty_report(
            ProviderFamily::JetRegistry,
            "jet-registry",
            "registry metadata is not valid JSON",
        );
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
        report
            .losses
            .push("registry entry has no content hash".to_string());
    }
    report
}

fn github_report(document: &str) -> ProviderFactReport {
    let parsed = JSON::parse(document).ok();
    let Some(JSONValue::Object(object)) = parsed else {
        return empty_report(
            ProviderFamily::Github,
            "github",
            "release metadata is not valid JSON",
        );
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
        return empty_report(
            ProviderFamily::Binary,
            "binary",
            "binary metadata is not valid JSON",
        );
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
    let identity = if facts.version.is_empty() {
        facts.integrity_hash.clone()
    } else {
        facts.version.clone()
    };
    facts.source_identity = format!("binary:{}@{}", facts.name, identity);
    let mut report = report_with_identity(facts);
    if report.facts.integrity_hash.is_empty() {
        report
            .losses
            .push("binary metadata has no content hash".to_string());
    }
    report
}

fn report_with_identity(facts: MetadataFacts) -> ProviderFactReport {
    let mut losses = Vec::new();
    if facts.name.is_empty() {
        losses.push("provider metadata has no package name".to_string());
    }
    if metadata_identity_selector(&facts).1.is_empty() {
        losses.push("provider metadata has no exact version, revision, or digest".to_string());
    }
    ProviderFactReport {
        facts,
        losses,
        conflicts: Vec::new(),
        native_format: String::new(),
        native_document: String::new(),
    }
}

fn empty_report(family: ProviderFamily, name: &str, loss: &str) -> ProviderFactReport {
    let facts = MetadataFacts::empty(family, name);
    ProviderFactReport {
        facts,
        losses: vec![loss.to_string()],
        conflicts: Vec::new(),
        native_format: String::new(),
        native_document: String::new(),
    }
}

fn provider_document_format(family: &ProviderFamily, document: &str) -> String {
    match family {
        ProviderFamily::Npm
        | ProviderFamily::SwiftPM
        | ProviderFamily::Vcpkg
        | ProviderFamily::Homebrew
        | ProviderFamily::JetRegistry
        | ProviderFamily::Github
        | ProviderFamily::Binary => "json".to_string(),
        ProviderFamily::NuGet if JSON::parse(document).is_ok() => "json".to_string(),
        ProviderFamily::NuGet => "xml".to_string(),
        ProviderFamily::Conan if JSON::parse(document).is_ok() => "json".to_string(),
        ProviderFamily::Conan => "conan".to_string(),
        ProviderFamily::Cargo => "toml".to_string(),
        ProviderFamily::PyPI => "python-metadata".to_string(),
        ProviderFamily::Maven => "xml".to_string(),
        ProviderFamily::Core | ProviderFamily::Nix | ProviderFamily::Path => "provider".to_string(),
    }
}

fn json_string(
    object: &std::collections::BTreeMap<String, JSONValue>,
    key: &str,
) -> Option<String> {
    match object.get(key) {
        Some(JSONValue::String(value)) if !value.trim().is_empty() => Some(value.clone()),
        _ => None,
    }
}

fn json_keys(object: &std::collections::BTreeMap<String, JSONValue>, key: &str) -> Vec<String> {
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
                JSONValue::String(value) => Some(value.clone()),
                JSONValue::Object(value) => value.get("name").and_then(|value| match value {
                    JSONValue::String(value) => Some(value.clone()),
                    _ => None,
                }),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn vcpkg_dependencies(
    object: &std::collections::BTreeMap<String, JSONValue>,
    typed: &mut std::collections::BTreeMap<String, Vec<String>>,
) -> Vec<String> {
    let Some(JSONValue::Array(values)) = object.get("dependencies") else {
        return Vec::new();
    };
    let mut dependencies = Vec::new();
    for value in values {
        match value {
            JSONValue::String(name) => dependencies.push(name.clone()),
            JSONValue::Object(dependency) => {
                let Some(name) = json_string(dependency, "name") else {
                    continue;
                };
                let version = ["version>=", "version>", "version"]
                    .iter()
                    .find_map(|key| json_string(dependency, key))
                    .unwrap_or_default();
                dependencies.push(format_dependency(&name, &version));
                for key in ["features", "platform", "host"] {
                    if let Some(value) = dependency.get(key) {
                        typed.insert(
                            format!("vcpkg.dependency.{name}.{key}"),
                            vec![json_value_text(value)],
                        );
                    }
                }
            }
            _ => {}
        }
    }
    dependencies
}

fn json_value_text(value: &JSONValue) -> String {
    match value {
        JSONValue::Null => "null".to_string(),
        JSONValue::Bool(value) => value.to_string(),
        JSONValue::Number(value) => value.to_string(),
        JSONValue::Flt(value) => value.to_string(),
        JSONValue::String(value) => value.clone(),
        JSONValue::Array(_) | JSONValue::Object(_) => json_value_json(value),
    }
}

fn json_value_json(value: &JSONValue) -> String {
    match value {
        JSONValue::Null => "null".to_string(),
        JSONValue::Bool(value) => value.to_string(),
        JSONValue::Number(value) => value.to_string(),
        JSONValue::Flt(value) => value.to_string(),
        JSONValue::String(value) => JSON::quote(value),
        JSONValue::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(json_value_json)
                .collect::<Vec<_>>()
                .join(",")
        ),
        JSONValue::Object(values) => format!(
            "{{{}}}",
            values
                .iter()
                .map(|(key, value)| format!("{}:{}", JSON::quote(key), json_value_json(value)))
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

// ponytail: bounded tag/attribute scan; add an XML parser if namespaces or
// entity decoding become part of provider identity.
fn xml_tag(document: &str, tag: &str) -> Option<String> {
    xml_tag_values(document, tag).into_iter().next()
}

fn xml_tag_values(document: &str, tag: &str) -> Vec<String> {
    let closing = format!("</{tag}>");
    xml_opening_tag_ranges(document, tag)
        .into_iter()
        .filter_map(|(_, end, opening)| {
            if opening.trim_end().ends_with("/>") {
                return None;
            }
            let value_start = end + 1;
            document
                .get(value_start..)?
                .split_once(&closing)
                .map(|(value, _)| value.trim().to_string())
        })
        .collect()
}

fn xml_opening_tags(document: &str, tag: &str) -> Vec<String> {
    xml_opening_tag_ranges(document, tag)
        .into_iter()
        .map(|(_, _, opening)| opening)
        .collect()
}

fn xml_opening_tag_ranges(document: &str, tag: &str) -> Vec<(usize, usize, String)> {
    let needle = format!("<{tag}");
    let mut ranges = Vec::new();
    let mut cursor = 0;
    while cursor < document.len() {
        let Some(relative_start) = document[cursor..].find(&needle) else {
            break;
        };
        let start = cursor + relative_start;
        let name_end = start + 1 + tag.len();
        let boundary = document[name_end..].chars().next();
        if !matches!(boundary, Some(value) if value.is_whitespace() || value == '>' || value == '/')
        {
            cursor = name_end;
            continue;
        }
        let Some(relative_end) = xml_opening_tag_end(&document[start..]) else {
            break;
        };
        let end = start + relative_end;
        ranges.push((start, end, document[start..=end].to_string()));
        cursor = end + 1;
    }
    ranges
}

fn xml_opening_tag_end(opening: &str) -> Option<usize> {
    let mut quote = None;
    for (index, character) in opening.char_indices() {
        match (quote, character) {
            (Some(expected), value) if expected == value => quote = None,
            (None, '\'' | '"') => quote = Some(character),
            (None, '>') => return Some(index),
            _ => {}
        }
    }
    None
}

fn xml_attribute(opening: &str, key: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let marker = format!("{key}={quote}");
        let mut cursor = 0;
        while cursor < opening.len() {
            let Some(relative) = opening[cursor..].find(&marker) else {
                break;
            };
            let start = cursor + relative;
            let valid_boundary = start == 0
                || opening[..start]
                    .chars()
                    .next_back()
                    .is_some_and(|value| value.is_whitespace() || value == '<' || value == '/');
            if valid_boundary {
                let value_start = start + marker.len();
                if let Some(value) = opening[value_start..].split_once(quote) {
                    return Some(value.0.to_string());
                }
            }
            cursor = start + marker.len();
        }
    }
    None
}

fn metadata_line(document: &str, key: &str) -> Option<String> {
    document.lines().find_map(|line| {
        let (left, right) = line.split_once(':')?;
        left.trim()
            .eq_ignore_ascii_case(key)
            .then(|| right.trim().to_string())
    })
}

fn metadata_list(document: &str, key: &str) -> Vec<String> {
    document
        .lines()
        .filter_map(|line| {
            let (left, right) = line.split_once(':')?;
            left.trim()
                .eq_ignore_ascii_case(key)
                .then(|| right.trim().to_string())
        })
        .collect()
}

fn format_dependency(name: &str, version: &str) -> String {
    let name = name.trim();
    let version = version.trim();
    match (name.is_empty(), version.is_empty()) {
        (true, _) => version.to_string(),
        (_, true) => name.to_string(),
        (false, false) => format!("{name}@{version}"),
    }
}

fn assignment_values(document: &str, key: &str) -> Vec<String> {
    document
        .lines()
        .filter_map(|line| {
            let (left, right) = line.split_once('=')?;
            let left = left.trim().strip_prefix("self.").unwrap_or(left.trim());
            if left != key {
                return None;
            }
            quoted_values_in(right).into_iter().next().or_else(|| {
                let value = right
                    .split('#')
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .trim_end_matches(',');
                (!value.is_empty()).then(|| value.to_string())
            })
        })
        .collect()
}

fn quoted_values_after_all(document: &str, key: &str) -> Vec<String> {
    let self_key = format!("self.{key}");
    let mut values = Vec::new();
    for line in document.lines() {
        let line = line.trim_start();
        let rest = line
            .strip_prefix(key)
            .or_else(|| line.strip_prefix(&self_key));
        let Some(rest) = rest else {
            continue;
        };
        if !matches!(
            rest.chars().next(),
            None | Some(' ' | '\t' | '=' | '(' | ':')
        ) {
            continue;
        }
        values.extend(quoted_values_in(line));
    }
    values
}

fn quoted_values_in(text: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut quote = None;
    let mut value_start = 0;
    for (index, character) in text.char_indices() {
        match (quote, character) {
            (Some(expected), value) if expected == value => {
                values.push(text[value_start..index].to_string());
                quote = None;
            }
            (None, '\'' | '"') => {
                quote = Some(character);
                value_start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    values
}

fn conan_ref_part(reference: &str, index: usize) -> Option<String> {
    let value = reference.split('@').next()?.split('/').nth(index)?.trim();
    (!value.is_empty()).then(|| value.to_string())
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

#[cfg(test)]
mod tests {
    use super::{normalize_provider_document, ProviderFamily};
    use jet_pkg_model::ProviderFacts::ProviderFacts;

    #[test]
    fn lock_uses_canonical_carrier_and_retains_raw_reference() {
        let native = r#"{"name":"web","version":"1.0.0","dependencies":{"vite":"5"}}"#;
        let report = normalize_provider_document(ProviderFamily::Npm, native);
        report.validate().expect("lossless provider report");
        let shared = report.shared_facts();
        let lock = report
            .lock_record("app", "web#1.0.0@npm", "any")
            .expect("canonical provider lock");
        let locked = ProviderFacts::from_json(
            lock.future_fields
                .get("provider-facts")
                .expect("provider facts in lock"),
        )
        .expect("locked provider facts JSON");
        assert_eq!(locked, shared);
        assert_eq!(lock.rationales[0].source_ref, "web#1.0.0@npm".to_string());
    }
}
